//! `repository/kinds/misc.ts`: the icon kinds (text note, file attachment),
//! stamps, links, widgets and the unsupported fallback.

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{
    Annot, Geom, PdfLinkTarget, PdfRect, PropKey, TextAlign, TextStyle, content_to_pdf_rect,
    pdf_to_content_rect,
};
use serde_json::Value;

use crate::repository::projection::{IngestSlice, Wire};
use crate::repository::seam::{box_geom_from_dto, color_to_css, pdf_json, writable_link};
use crate::value::{get, opt_f64, opt_pdf_rect, pdf_rect_of, str_of};

fn rect_geom(dto: &Value, crop: PdfRect) -> Geom {
    Geom::Rect {
        rect: pdf_to_content_rect(pdf_rect_of(get(dto, "rect")), crop),
        ellipse: false,
        rot: None,
    }
}

/// `rectGeometry`: `/Rect` for any geom that has a box.
pub fn rect_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let rect = a.geom.rect()?;
    let mut out = Wire::new();
    out.insert("rect".into(), pdf_json(content_to_pdf_rect(*rect, crop)));
    Some(out)
}

pub fn icon_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let mut slice = IngestSlice::geom(rect_geom(d, crop));
    slice.icon = str_of(d, "icon").map(str::to_string);
    slice
}

pub fn stamp_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    IngestSlice::geom(box_geom_from_dto(
        pdf_rect_of(get(d, "rect")),
        opt_f64(d, "rotation"),
        opt_pdf_rect(get(d, "unrotatedRect")),
        crop,
        false,
    ))
}

/// The DTO's `target` as the model's three-state `link`.
pub fn link_target_of(d: &Value) -> Nullable<PdfLinkTarget> {
    match get(d, "target") {
        None => Nullable::Undefined,
        Some(Value::Null) => Nullable::Null,
        Some(t) => serde_json::from_value(t.clone()).map_or(Nullable::Null, Nullable::Value),
    }
}

pub fn link_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let mut slice = IngestSlice::geom(rect_geom(d, crop));
    slice.link = link_target_of(d);
    slice
}

/// The link kind's three-state target: a writable value replaces `/A`, a
/// model `null` clears it, a read-only arm is left untouched.
pub fn link_prop(key: PropKey, a: &Annot) -> Option<Wire> {
    if key != PropKey::Link {
        return None;
    }
    let mut out = Wire::new();
    match writable_link(&a.link) {
        Some(target) => {
            out.insert("target".into(), serde_json::to_value(target).unwrap_or_default());
        }
        None if a.link.as_option().is_none() => {
            out.insert("target".into(), Value::Null);
        }
        None => {}
    }
    Some(out)
}

pub fn link_draft_extras(a: &Annot) -> Wire {
    let mut out = Wire::new();
    out.insert(
        "target".into(),
        writable_link(&a.link).map_or(Value::Null, |t| serde_json::to_value(t).unwrap_or_default()),
    );
    out
}

/// `widgetKindOf(family)`: one PDF `widget` subtype, per-family client kinds.
pub fn widget_kind_of(family: &str) -> String {
    match family {
        "text" => "widget-text",
        "combobox" | "listbox" => "widget-choice",
        "pushbutton" => "widget-button",
        "checkbox" | "radio" => "widget-toggle",
        _ => "widget-box",
    }
    .to_string()
}

const WIDGET_TEXT_KINDS: [&str; 3] = ["widget-text", "widget-choice", "widget-button"];

fn widget_text_from_dto(d: &Value) -> TextStyle {
    TextStyle {
        font_family: str_of(d, "fontFamily").unwrap_or("helvetica").to_string(),
        font_size: get(d, "fontSize").and_then(Value::as_f64).unwrap_or(0.0),
        font_color: get(d, "fontColor")
            .filter(|c| c.is_object())
            .map_or_else(|| "#000000".to_string(), color_to_css),
        text_align: get(d, "textAlign")
            .and_then(|a| serde_json::from_value(a.clone()).ok())
            .unwrap_or(TextAlign::Left),
    }
}

pub fn widget_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let mut slice =
        IngestSlice::geom(box_geom_from_dto(pdf_rect_of(get(d, "rect")), None, None, crop, false));
    let family = str_of(d, "fieldFamily").unwrap_or_default();
    if WIDGET_TEXT_KINDS.contains(&widget_kind_of(family).as_str()) {
        slice.text = Some(widget_text_from_dto(d));
    }
    slice
}

pub fn unsupported_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    IngestSlice::geom(rect_geom(d, crop))
}
