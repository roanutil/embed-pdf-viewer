//! `repository/kinds/shape.ts`: square and circle, and the cloudy-border
//! physics (`/BE` intensity plus the derived `/RD` inset).

use epdf_annotation_core::{Annot, Border, Geom, PdfRect, PropKey, cloudy_border_extent};
use serde_json::{Value, json};

use crate::repository::projection::{IngestSlice, Wire};
use crate::repository::props::border_slice;
use crate::repository::seam::{box_geom_from_dto, num_json};
use crate::value::{get, opt_f64, opt_pdf_rect, pdf_rect_of};

/// `cloudyExtras(a)`: total, so turning cloudy off states the clears.
pub fn cloudy_extras(a: &Annot) -> Wire {
    let mut out = Wire::new();
    let Geom::Rect { ellipse, .. } = &a.geom else { return out };
    if let Border::Cloudy { intensity } = a.style.border {
        let inset = cloudy_border_extent(intensity, a.style.stroke_width, *ellipse);
        out.insert("cloudyIntensity".into(), num_json(intensity));
        out.insert(
            "rectDifferences".into(),
            json!({ "left": num_json(inset), "top": num_json(inset), "right": num_json(inset), "bottom": num_json(inset) }),
        );
    } else {
        out.insert("cloudyIntensity".into(), Value::Null);
        out.insert("rectDifferences".into(), Value::Null);
    }
    out
}

pub fn ingest(dto: &Value, crop: PdfRect, ellipse: bool) -> IngestSlice {
    IngestSlice::geom(box_geom_from_dto(
        pdf_rect_of(get(dto, "rect")),
        opt_f64(dto, "rotation"),
        opt_pdf_rect(get(dto, "unrotatedRect")),
        crop,
        ellipse,
    ))
}

pub fn prop(key: PropKey, a: &Annot) -> Option<Wire> {
    match key {
        PropKey::StrokeWidth => {
            let mut out = Wire::new();
            out.insert("strokeWidth".into(), num_json(a.style.stroke_width));
            out.extend(cloudy_extras(a));
            Some(out)
        }
        PropKey::Border => {
            let mut out = border_slice(&a.style);
            out.extend(cloudy_extras(a));
            Some(out)
        }
        _ => None,
    }
}
