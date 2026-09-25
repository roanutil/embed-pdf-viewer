//! `repository/kinds/quads.ts`: text markup, caret and redact. Their
//! `/QuadPoints` are set at create and never patched.

use epdf_annotation_core::{
    Annot, Geom, Intent, Label, PdfRect, TextAlign, TextStyle, content_to_pdf_point,
    content_to_pdf_rect, pdf_to_content_point, pdf_to_content_rect,
};
use epdf_geometry::{Point, Quad, TextQuad, normalize_quad};
use serde_json::{Value, json};

use crate::repository::projection::{IngestSlice, Wire};
use crate::repository::seam::{box_geom_fields, color_to_css, from_pdf_rotation, pdf_json};
use crate::value::{
    f64_of, get, opt_f64, opt_pdf_rect, pdf_point_of, pdf_rect_of, point_json, str_of, truthy,
    truthy_num,
};

fn quads_from_dto(quad_points: Option<&Value>, crop: PdfRect) -> Vec<TextQuad> {
    let content = |q: &Value, k: &str| -> Point { pdf_to_content_point(pdf_point_of(get(q, k)), crop) };
    quad_points
        .and_then(Value::as_array)
        .map(|qs| {
            qs.iter()
                .map(|q| {
                    normalize_quad(Quad {
                        p1: content(q, "p1"),
                        p2: content(q, "p2"),
                        p3: content(q, "p3"),
                        p4: content(q, "p4"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A content point as a wire `PdfPoint`.
fn pdf_pt(p: Point, crop: PdfRect) -> (f64, f64) {
    let q = content_to_pdf_point(p, crop);
    (q.x, q.y)
}

/// `quadPointsFor(a, crop)`: the zigzag `/QuadPoints`, or `None` off a
/// quads geom.
pub fn quad_points_for(a: &Annot, crop: PdfRect) -> Option<Vec<[(f64, f64); 4]>> {
    let Geom::Quads { quads } = &a.geom else { return None };
    Some(
        quads
            .iter()
            .map(|q| {
                [
                    pdf_pt(q.upper_start, crop),
                    pdf_pt(q.upper_end, crop),
                    pdf_pt(q.lower_start, crop),
                    pdf_pt(q.lower_end, crop),
                ]
            })
            .collect(),
    )
}

pub fn quad_points_json(quads: &[[(f64, f64); 4]]) -> Value {
    Value::Array(
        quads
            .iter()
            .map(|q| {
                json!({
                    "p1": point_json(q[0].0, q[0].1),
                    "p2": point_json(q[1].0, q[1].1),
                    "p3": point_json(q[2].0, q[2].1),
                    "p4": point_json(q[3].0, q[3].1),
                })
            })
            .collect(),
    )
}

/// `pdfBoundsOfQuads(quads)`.
pub fn pdf_bounds_of_quads(quads: &[[(f64, f64); 4]]) -> PdfRect {
    let (mut left, mut bottom) = (f64::INFINITY, f64::INFINITY);
    let (mut right, mut top) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in quads {
        for &(x, y) in q {
            if x < left {
                left = x;
            }
            if x > right {
                right = x;
            }
            if y < bottom {
                bottom = y;
            }
            if y > top {
                top = y;
            }
        }
    }
    PdfRect { left, bottom, right, top }
}

fn intent_of(d: &Value) -> Option<Intent> {
    get(d, "intent").and_then(|i| serde_json::from_value(i.clone()).ok())
}

pub fn markup_ingest(d: &Value, crop: PdfRect, strikeout: bool) -> IngestSlice {
    let mut slice =
        IngestSlice::geom(Geom::Quads { quads: quads_from_dto(get(d, "quadPoints"), crop) });
    if strikeout && truthy(get(d, "intent")) {
        slice.intent = intent_of(d);
    }
    slice
}

pub fn markup_draft_extras(a: &Annot, crop: PdfRect, strikeout: bool) -> Option<Wire> {
    let quads = quad_points_for(a, crop)?;
    let mut out = Wire::new();
    out.insert("quadPoints".into(), quad_points_json(&quads));
    if strikeout && a.intent == Some(Intent::StrikeoutTextEdit) {
        out.insert("intent".into(), json!("strikeout-text-edit"));
    }
    Some(out)
}

pub fn caret_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let rot = opt_f64(d, "rotation").filter(|r| truthy_num(*r)).map_or(0.0, from_pdf_rotation);
    let bx = match opt_pdf_rect(get(d, "unrotatedRect")) {
        Some(u) if truthy_num(rot) => u,
        _ => pdf_rect_of(get(d, "rect")),
    };
    let mut slice = IngestSlice::geom(Geom::Caret {
        rect: pdf_to_content_rect(bx, crop),
        rot: truthy_num(rot).then_some(rot),
    });
    if truthy(get(d, "intent")) {
        slice.intent = intent_of(d);
    }
    slice
}

pub fn caret_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let Geom::Caret { rect, rot } = &a.geom else { return None };
    Some(box_geom_fields(*rect, rot.unwrap_or(0.0), crop))
}

pub fn caret_draft_extras(a: &Annot) -> Wire {
    let mut out = Wire::new();
    out.insert(
        "rectDifferences".into(),
        json!({ "left": 0.5, "top": 0.5, "right": 0.5, "bottom": 0.5 }),
    );
    if a.intent == Some(Intent::Replace) {
        out.insert("intent".into(), json!("replace"));
    }
    if let Some(contents) = a.data.as_ref().and_then(|d| get(d, "contents")).filter(|c| !c.is_null())
    {
        out.insert("contents".into(), contents.clone());
    }
    out
}

pub fn redact_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let has_quads = get(d, "quadPoints").and_then(Value::as_array).is_some_and(|q| !q.is_empty());
    let geom = if has_quads {
        Geom::Quads { quads: quads_from_dto(get(d, "quadPoints"), crop) }
    } else {
        Geom::Rect { rect: pdf_to_content_rect(pdf_rect_of(get(d, "rect")), crop), ellipse: false, rot: None }
    };
    let mut slice = IngestSlice::geom(geom);
    slice.text = Some(TextStyle {
        font_family: str_of(d, "fontFamily").unwrap_or_default().to_string(),
        font_size: f64_of(d, "fontSize"),
        font_color: color_to_css(get(d, "fontColor").unwrap_or(&Value::Null)),
        text_align: get(d, "textAlign")
            .and_then(|a| serde_json::from_value(a.clone()).ok())
            .unwrap_or(TextAlign::Left),
    });
    if let Some(text) = str_of(d, "overlayText").filter(|t| !t.is_empty()) {
        slice.label = Some(Label {
            text: text.to_string(),
            repeat: get(d, "repeat").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    slice
}

pub fn redact_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let Geom::Rect { rect, .. } = &a.geom else { return None };
    let mut out = Wire::new();
    out.insert("rect".into(), pdf_json(content_to_pdf_rect(*rect, crop)));
    Some(out)
}

pub fn redact_draft_extras(a: &Annot, crop: PdfRect) -> Wire {
    let mut out = Wire::new();
    if let Some(quads) = quad_points_for(a, crop) {
        out.insert("quadPoints".into(), quad_points_json(&quads));
        out.insert("rect".into(), pdf_json(pdf_bounds_of_quads(&quads)));
    }
    out
}
