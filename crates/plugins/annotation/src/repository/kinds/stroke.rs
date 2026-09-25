//! `repository/kinds/stroke.ts`: line, polygon, polyline and ink. Their
//! `/Rect` is the visual bounds (stroke, border and endings included), and
//! their rotation an advisory scalar.

use epdf_annotation_core::{
    Annot, Border, Geom, Intent, PdfRect, PropKey, content_to_pdf_point, geom_pdf_bounds,
    geom_rotation, pdf_to_content_point,
};
use serde_json::{Value, json};

use crate::repository::projection::{IngestSlice, Wire};
use crate::repository::props::border_slice;
use crate::repository::seam::{num_json, pdf_json, rot_from_dto, to_pdf_rotation};
use crate::value::{get, opt_f64, pdf_point_of, point_json, truthy_num};

fn advisory_rotation(g: &Geom) -> Wire {
    let rot = geom_rotation(g);
    let mut out = Wire::new();
    out.insert(
        "rotation".into(),
        if truthy_num(rot) { num_json(to_pdf_rotation(rot)) } else { Value::Null },
    );
    out
}

fn poly_cloudy(a: &Annot) -> Wire {
    let mut out = Wire::new();
    if let Geom::Poly { closed: true, .. } = a.geom {
        let v = match a.style.border {
            Border::Cloudy { intensity } => num_json(intensity),
            _ => Value::Null,
        };
        out.insert("cloudyIntensity".into(), v);
    }
    out
}

fn visual_rect(a: &Annot, crop: PdfRect) -> Wire {
    let mut out = Wire::new();
    match &a.geom {
        g @ (Geom::Line { .. } | Geom::Ink { .. }) => {
            out.insert(
                "rect".into(),
                pdf_json(geom_pdf_bounds(g, a.style.stroke_width, crop, None)),
            );
        }
        g @ Geom::Poly { .. } => {
            out.insert(
                "rect".into(),
                pdf_json(geom_pdf_bounds(g, a.style.stroke_width, crop, Some(&a.style.border))),
            );
        }
        _ => {}
    }
    out
}

fn with_rect(mut lowered: Wire, a: &Annot, crop: PdfRect) -> Wire {
    lowered.extend(visual_rect(a, crop));
    lowered
}

pub fn prop(key: PropKey, a: &Annot, crop: PdfRect) -> Option<Wire> {
    let lowered = match key {
        PropKey::StrokeWidth => {
            let mut out = Wire::new();
            out.insert("strokeWidth".into(), num_json(a.style.stroke_width));
            out
        }
        PropKey::Border => {
            let mut out = border_slice(&a.style);
            out.extend(poly_cloudy(a));
            out
        }
        PropKey::LineEndings => {
            let mut out = Wire::new();
            let ends = match &a.geom {
                Geom::Line { ends, .. } | Geom::Poly { ends, .. } => *ends,
                _ => None,
            };
            if let Some(ends) = ends {
                out.insert("lineEndings".into(), serde_json::to_value(ends).unwrap_or_default());
            }
            out
        }
        _ => return None,
    };
    Some(with_rect(lowered, a, crop))
}

fn endings_of(d: &Value) -> Option<epdf_annotation_core::LineEndings> {
    get(d, "lineEndings").and_then(|e| serde_json::from_value(e.clone()).ok())
}

pub fn line_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let lp = get(d, "linePoints").cloned().unwrap_or(Value::Null);
    IngestSlice::geom(Geom::Line {
        a: pdf_to_content_point(pdf_point_of(get(&lp, "start")), crop),
        b: pdf_to_content_point(pdf_point_of(get(&lp, "end")), crop),
        ends: endings_of(d),
        rot: rot_from_dto(opt_f64(d, "rotation")),
    })
}

fn pdf_point_json(p: epdf_annotation_core::PdfPoint) -> Value {
    point_json(p.x, p.y)
}

pub fn line_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let Geom::Line { a: pa, b: pb, .. } = &a.geom else { return None };
    let mut out = Wire::new();
    out.insert(
        "linePoints".into(),
        json!({
            "start": pdf_point_json(content_to_pdf_point(*pa, crop)),
            "end": pdf_point_json(content_to_pdf_point(*pb, crop)),
        }),
    );
    out.extend(visual_rect(a, crop));
    out.extend(advisory_rotation(&a.geom));
    Some(out)
}

pub fn poly_ingest(d: &Value, crop: PdfRect, closed: bool) -> IngestSlice {
    let points = get(d, "vertices")
        .and_then(Value::as_array)
        .map(|vs| vs.iter().map(|p| pdf_to_content_point(pdf_point_of(Some(p)), crop)).collect())
        .unwrap_or_default();
    IngestSlice::geom(Geom::Poly {
        points,
        closed,
        ends: endings_of(d),
        rot: rot_from_dto(opt_f64(d, "rotation")),
    })
}

pub fn poly_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let Geom::Poly { points, .. } = &a.geom else { return None };
    let mut out = Wire::new();
    out.insert(
        "vertices".into(),
        Value::Array(
            points.iter().map(|p| pdf_point_json(content_to_pdf_point(*p, crop))).collect(),
        ),
    );
    out.extend(visual_rect(a, crop));
    out.extend(advisory_rotation(&a.geom));
    Some(out)
}

pub fn ink_ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let strokes = get(d, "inkList")
        .and_then(Value::as_array)
        .map(|ss| {
            ss.iter()
                .map(|s| {
                    s.as_array()
                        .map(|ps| {
                            ps.iter()
                                .map(|p| pdf_to_content_point(pdf_point_of(Some(p)), crop))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let mut slice =
        IngestSlice::geom(Geom::Ink { strokes, rot: rot_from_dto(opt_f64(d, "rotation")) });
    slice.intent = get(d, "intent").and_then(|i| serde_json::from_value::<Intent>(i.clone()).ok());
    slice
}

pub fn ink_geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let Geom::Ink { strokes, .. } = &a.geom else { return None };
    let mut out = Wire::new();
    out.insert(
        "inkList".into(),
        Value::Array(
            strokes
                .iter()
                .map(|s| {
                    Value::Array(
                        s.iter().map(|p| pdf_point_json(content_to_pdf_point(*p, crop))).collect(),
                    )
                })
                .collect(),
        ),
    );
    out.extend(visual_rect(a, crop));
    out.extend(advisory_rotation(&a.geom));
    Some(out)
}

pub fn ink_draft_extras(a: &Annot) -> Wire {
    let mut out = Wire::new();
    if a.intent == Some(Intent::InkHighlight) {
        out.insert("intent".into(), json!("ink-highlight"));
    }
    out
}
