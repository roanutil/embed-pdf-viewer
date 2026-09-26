//! `repository/seam.ts`: the geometry, colour and rotation seam between the
//! engine's PDF-space wire vocabulary and the core's content-space model.
//! Every conversion between the two worlds lives here.

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{
    AnnotationPropsPatch, AnnotationRef, Border, Geom, PdfLinkTarget, PdfRect, Rect, Style,
    content_to_pdf_rect, normalize_deg, pdf_to_content_rect, rotated_aabb,
};
use epdf_geometry::js as num;
use serde_json::{Map, Value, json};

use super::projection::Wire;
use crate::value::{f64_of, get, pdf_rect_of, truthy_num};

pub use epdf_annotation_core::{
    content_to_pdf_point, content_to_pdf_rect as content_to_pdf, pdf_to_content_point,
    pdf_to_content_rect as pdf_to_content,
};

/// `annotationKey(ref)` (engine-core): the one model id of an engine ref,
/// `obj:<n>`, `nm:<page>:<name>` or `idx:<page>:<i>`.
pub fn annotation_key(r: &AnnotationRef) -> String {
    match r {
        AnnotationRef::ObjectNumber { annot_object_number, .. } => {
            format!("obj:{}", num::number_to_string(*annot_object_number))
        }
        AnnotationRef::Nm { page, nm } => format!("nm:{}:{nm}", page.page_object_number),
        AnnotationRef::Index { page, index, .. } => {
            format!("idx:{}:{}", page.page_object_number, num::number_to_string(*index))
        }
    }
}

/// `annotationKey` over a ref still in its wire form.
pub fn annotation_key_of(r: &Value) -> Option<String> {
    serde_json::from_value::<AnnotationRef>(r.clone()).ok().map(|r| annotation_key(&r))
}

// ── colour seam (engine Color <-> CSS hex) ────────────────────────────────────

fn h2(n: f64) -> String {
    let v = num::max(0.0, num::min(255.0, num::round(n)));
    if v.is_nan() {
        return "NaN".into();
    }
    format!("{:02x}", v as i64)
}

/// `colorToCss(c)`.
pub fn color_to_css(c: &Value) -> String {
    format!("#{}{}{}", h2(f64_of(c, "r")), h2(f64_of(c, "g")), h2(f64_of(c, "b")))
}

/// JavaScript's `String.prototype.trim` whitespace: the Unicode spaces plus
/// the BOM and line terminators.
fn js_trim(s: &str) -> &str {
    let ws = |c: char| c.is_whitespace() || c == '\u{feff}';
    s.trim_matches(ws)
}

fn hex_digits(s: &str, len: usize) -> Option<&str> {
    let body = s.strip_prefix('#').unwrap_or(s);
    (body.len() == len && body.bytes().all(|b| b.is_ascii_hexdigit())).then_some(body)
}

/// `cssToColor(css)`: `#rrggbb` or `#rgb` (the `#` optional), else black.
pub fn css_to_color(css: &str) -> Value {
    let s = js_trim(css);
    if let Some(six) = hex_digits(s, 6) {
        let n = i64::from_str_radix(six, 16).unwrap_or(0);
        return json!({ "r": (n >> 16) & 255, "g": (n >> 8) & 255, "b": n & 255 });
    }
    if let Some(three) = hex_digits(s, 3) {
        let d: Vec<i64> = three
            .chars()
            .map(|c| i64::from_str_radix(&format!("{c}{c}"), 16).unwrap_or(0))
            .collect();
        return json!({ "r": d[0], "g": d[1], "b": d[2] });
    }
    json!({ "r": 0, "g": 0, "b": 0 })
}

// ── rotation seam (CW content <-> PDF convention) ─────────────────────────────

/// `toPdfRotation(rotCW)`.
pub fn to_pdf_rotation(rot_cw: f64) -> f64 {
    normalize_deg(-rot_cw)
}

/// `fromPdfRotation(rotPdf)`.
pub fn from_pdf_rotation(rot_pdf: f64) -> f64 {
    normalize_deg(-rot_pdf)
}

/// `rotFromDTO(rotation)`: the advisory `rot` of a vertex geom, absent for
/// no (or a zero) rotation.
pub fn rot_from_dto(rotation: Option<f64>) -> Option<f64> {
    rotation.filter(|r| truthy_num(*r)).map(from_pdf_rotation)
}

/// `boxGeomFields(rect, rot, crop)`: `/Rect` plus the rotation pair, total
/// (an unrotated box states both as `null`).
pub fn box_geom_fields(rect: Rect, rot: f64, crop: PdfRect) -> Wire {
    let mut out = Map::new();
    if !truthy_num(rot) {
        out.insert("rect".into(), pdf_json(content_to_pdf_rect(rect, crop)));
        out.insert("rotation".into(), Value::Null);
        out.insert("unrotatedRect".into(), Value::Null);
        return out;
    }
    out.insert("rect".into(), pdf_json(content_to_pdf_rect(rotated_aabb(rect, rot), crop)));
    out.insert("unrotatedRect".into(), pdf_json(content_to_pdf_rect(rect, crop)));
    out.insert("rotation".into(), num_json(to_pdf_rotation(rot)));
    out
}

/// `boxGeomFromDTO(dto, rotation, unrotatedRect, crop, ellipse)`.
pub fn box_geom_from_dto(
    rect: PdfRect,
    rotation: Option<f64>,
    unrotated_rect: Option<PdfRect>,
    crop: PdfRect,
    ellipse: bool,
) -> Geom {
    let rot = match rotation {
        Some(r) if truthy_num(r) => from_pdf_rotation(r),
        _ => 0.0,
    };
    let bx = match unrotated_rect {
        Some(u) if truthy_num(rot) => u,
        _ => rect,
    };
    Geom::Rect { rect: pdf_to_content_rect(bx, crop), ellipse, rot: truthy_num(rot).then_some(rot) }
}

/// `insetPdfRectByRD(r, rd)`.
pub fn inset_pdf_rect_by_rd(r: PdfRect, rd: Option<&Value>) -> PdfRect {
    match rd.filter(|v| v.is_object()) {
        Some(rd) => PdfRect {
            left: r.left + f64_of(rd, "left"),
            bottom: r.bottom + f64_of(rd, "bottom"),
            right: r.right - f64_of(rd, "right"),
            top: r.top - f64_of(rd, "top"),
        },
        None => r,
    }
}

/// `borderFromDTO(d)`: a cloudy effect wins over the border style.
pub fn border_from_dto(d: &Value) -> Border {
    let intensity = get(d, "cloudyIntensity").and_then(Value::as_f64).unwrap_or(0.0);
    if intensity > 0.0 {
        return Border::Cloudy { intensity };
    }
    if get(d, "borderStyle").and_then(Value::as_str) == Some("dashed") {
        let dash = get(d, "dashArray")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
            .unwrap_or_else(|| vec![3.0, 3.0]);
        return Border::Dashed { dash };
    }
    Border::Solid
}

/// `writableTarget(t)`: `goto` and `uri` pass through; the read-only arms
/// yield `None`.
pub fn writable_target(t: Option<&PdfLinkTarget>) -> Option<PdfLinkTarget> {
    t.filter(|t| matches!(t, PdfLinkTarget::Goto { .. } | PdfLinkTarget::Uri { .. })).cloned()
}

/// `writableTarget` over a model `link` member.
pub fn writable_link(link: &Nullable<PdfLinkTarget>) -> Option<PdfLinkTarget> {
    writable_target(link.as_option())
}

pub const TEXT_MARKUP: [&str; 4] = ["highlight", "underline", "squiggly", "strikeout"];
const STROKE_KINDS: [&str; 6] = ["square", "circle", "line", "polygon", "polyline", "ink"];

fn opt_css(v: Option<&Value>) -> Option<String> {
    v.filter(|c| c.is_object()).map(color_to_css)
}

fn blend_of(dto: &Value) -> epdf_annotation_core::BlendMode {
    get(dto, "blendMode").and_then(|b| serde_json::from_value(b.clone()).ok()).unwrap_or_default()
}

/// `styleFromDTO(dto)`: engine DTO to content-space `Style`.
pub fn style_from_dto(dto: &Value) -> Style {
    let subtype = get(dto, "subtype").and_then(Value::as_str).unwrap_or_default();
    let color = || color_to_css(get(dto, "color").unwrap_or(&Value::Null));
    let base = |color: String, interior: Option<String>, stroke: f64, opacity: f64, border| Style {
        color,
        interior_color: interior,
        stroke_width: stroke,
        opacity,
        blend_mode: blend_of(dto),
        border,
    };
    if subtype == "widget" {
        return base(
            opt_css(get(dto, "color")).unwrap_or_else(|| "#1a1a1a".into()),
            opt_css(get(dto, "interiorColor")),
            f64_of(dto, "strokeWidth"),
            1.0,
            if get(dto, "borderStyle").and_then(Value::as_str) == Some("dashed") {
                Border::Dashed { dash: vec![3.0, 3.0] }
            } else {
                Border::Solid
            },
        );
    }
    if STROKE_KINDS.contains(&subtype) {
        return base(
            color(),
            opt_css(get(dto, "interiorColor")),
            f64_of(dto, "strokeWidth"),
            f64_of(dto, "opacity"),
            border_from_dto(dto),
        );
    }
    if TEXT_MARKUP.contains(&subtype) {
        return base(color(), None, 0.0, f64_of(dto, "opacity"), Border::Solid);
    }
    if subtype == "caret" {
        return base(color(), None, 1.0, f64_of(dto, "opacity"), Border::Solid);
    }
    if subtype == "free-text" {
        return base(
            color(),
            opt_css(get(dto, "interiorColor")),
            f64_of(dto, "strokeWidth"),
            f64_of(dto, "opacity"),
            border_from_dto(dto),
        );
    }
    if subtype == "redact" {
        return base(
            color(),
            opt_css(get(dto, "interiorColor")),
            1.5,
            f64_of(dto, "opacity"),
            Border::Solid,
        );
    }
    if subtype == "text" || subtype == "file-attachment" {
        return base(color(), None, 1.0, f64_of(dto, "opacity"), Border::Solid);
    }
    base("#444444".into(), None, 1.0, 1.0, Border::Solid)
}

/// `widgetAppearanceFromProps(props)`: the flat props vocabulary to the
/// engine's widget appearance for `doc.forms` authoring. Absent keys stay
/// absent.
pub fn widget_appearance_from_props(props: &AnnotationPropsPatch) -> Value {
    let mut out = Map::new();
    if let Some(color) = &props.color {
        out.insert("color".into(), css_to_color(color));
    }
    match &props.interior_color {
        Nullable::Undefined => {}
        Nullable::Null => {
            out.insert("interiorColor".into(), Value::Null);
        }
        Nullable::Value(c) => {
            let v = if c.is_empty() { Value::Null } else { css_to_color(c) };
            out.insert("interiorColor".into(), v);
        }
    }
    if let Some(w) = props.stroke_width {
        out.insert("strokeWidth".into(), num_json(w));
    }
    if let Some(border) = &props.border {
        let style = if matches!(border, Border::Dashed { .. }) { "dashed" } else { "solid" };
        out.insert("borderStyle".into(), json!(style));
    }
    if let Some(f) = &props.font_family {
        out.insert("fontFamily".into(), json!(f));
    }
    if let Some(s) = props.font_size {
        out.insert("fontSize".into(), num_json(s));
    }
    if let Some(c) = &props.font_color {
        out.insert("fontColor".into(), css_to_color(c));
    }
    if let Some(a) = props.text_align {
        out.insert("textAlign".into(), serde_json::to_value(a).unwrap_or(Value::Null));
    }
    Value::Object(out)
}

// ── JSON helpers shared by the kind modules ───────────────────────────────────

pub fn pdf_json(r: PdfRect) -> Value {
    json!({ "left": num_json(r.left), "bottom": num_json(r.bottom), "right": num_json(r.right), "top": num_json(r.top) })
}

pub fn num_json(x: f64) -> Value {
    crate::value::number(x)
}

/// `pdfToContentRect` over a wire rect.
pub fn content_rect_of(v: Option<&Value>, crop: PdfRect) -> Rect {
    pdf_to_content_rect(pdf_rect_of(v), crop)
}
