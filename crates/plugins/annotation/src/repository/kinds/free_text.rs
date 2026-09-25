//! `repository/kinds/freeText.ts`: the plain box and the callout (leader
//! plus box), and the `/DA` text slice.

use epdf_annotation_core::{
    Annot, Callout, Geom, LineEnding, PdfRect, TextAlign, TextStyle, callout_line_points,
    content_to_pdf_point, content_to_pdf_rect, geom_pdf_bounds, geom_rotation,
    pdf_to_content_point, pdf_to_content_rect, rotated_aabb,
};
use epdf_geometry::js as num;
use serde_json::{Value, json};

use crate::repository::projection::{IngestSlice, Wire, box_emit};
use crate::repository::seam::{
    color_to_css, from_pdf_rotation, inset_pdf_rect_by_rd, num_json, pdf_json, to_pdf_rotation,
};
use crate::value::{
    f64_of, get, opt_f64, opt_pdf_rect, pdf_point_of, pdf_rect_of, point_json, str_of, truthy_num,
};

fn text_align_of(dto: &Value) -> TextAlign {
    get(dto, "textAlign").and_then(|a| serde_json::from_value(a.clone()).ok()).unwrap_or(TextAlign::Left)
}

/// `textFromDTO(dto)`: an absent `fontColor` falls back to the `/DA` colour.
fn text_from_dto(dto: &Value) -> TextStyle {
    let color = get(dto, "fontColor").filter(|c| !c.is_null()).or_else(|| get(dto, "color"));
    TextStyle {
        font_family: str_of(dto, "fontFamily").unwrap_or_default().to_string(),
        font_size: f64_of(dto, "fontSize"),
        font_color: color_to_css(color.unwrap_or(&Value::Null)),
        text_align: text_align_of(dto),
    }
}

/// `calloutFields(a, crop)`: the engine geometry of a callout, or `None`
/// for a plain box.
pub fn callout_fields(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let g = &a.geom;
    let Geom::Text { rect, callout: Some(callout), .. } = g else { return None };
    let rot = geom_rotation(g);
    let overall = geom_pdf_bounds(g, a.style.stroke_width, crop, None);
    let tb = content_to_pdf_rect(if truthy_num(rot) { rotated_aabb(*rect, rot) } else { *rect }, crop);
    let nn = |n: f64| num::max(0.0, n);
    let pts: Vec<Value> = callout_line_points(g)
        .into_iter()
        .map(|p| {
            let q = content_to_pdf_point(p, crop);
            point_json(q.x, q.y)
        })
        .collect();
    let at = |i: usize| pts.get(i).cloned().unwrap_or(Value::Null);
    let callout_line =
        if pts.len() == 3 { json!([at(0), at(1), at(2)]) } else { json!([at(0), at(1)]) };
    let mut out = Wire::new();
    out.insert("rect".into(), pdf_json(overall));
    out.insert(
        "rectDifferences".into(),
        json!({
            "left": num_json(nn(tb.left - overall.left)),
            "bottom": num_json(nn(tb.bottom - overall.bottom)),
            "right": num_json(nn(overall.right - tb.right)),
            "top": num_json(nn(overall.top - tb.top)),
        }),
    );
    out.insert("calloutLine".into(), callout_line);
    out.insert("lineEnding".into(), serde_json::to_value(callout.ending).unwrap_or_default());
    if truthy_num(rot) {
        out.insert("rotation".into(), num_json(to_pdf_rotation(rot)));
        out.insert("unrotatedRect".into(), pdf_json(content_to_pdf_rect(*rect, crop)));
    } else {
        out.insert("rotation".into(), Value::Null);
        out.insert("unrotatedRect".into(), Value::Null);
    }
    Some(out)
}

pub fn ingest(d: &Value, crop: PdfRect) -> IngestSlice {
    let rect = pdf_rect_of(get(d, "rect"));
    let rotation = opt_f64(d, "rotation").filter(|r| truthy_num(*r));
    let unrotated = opt_pdf_rect(get(d, "unrotatedRect"));
    let cl = get(d, "calloutLine").and_then(Value::as_array).filter(|cl| cl.len() >= 2);
    if let (Some("free-text-callout"), Some(cl)) = (str_of(d, "intent"), cl) {
        let rot = rotation.map_or(0.0, from_pdf_rotation);
        let bx = match unrotated {
            Some(u) if truthy_num(rot) => pdf_to_content_rect(u, crop),
            _ => pdf_to_content_rect(inset_pdf_rect_by_rd(rect, get(d, "rectDifferences")), crop),
        };
        let ending = get(d, "lineEnding")
            .and_then(|e| serde_json::from_value::<LineEnding>(e.clone()).ok())
            .unwrap_or(LineEnding::None);
        let mut slice = IngestSlice::geom(Geom::Text {
            rect: bx,
            callout: Some(Callout {
                tip: pdf_to_content_point(pdf_point_of(cl.first()), crop),
                knee: (cl.len() == 3).then(|| pdf_to_content_point(pdf_point_of(cl.get(1)), crop)),
                ending,
            }),
            rot: (truthy_num(rot) && unrotated.is_some()).then_some(rot),
        });
        slice.text = Some(text_from_dto(d));
        return slice;
    }
    let rot = rotation.map_or(0.0, from_pdf_rotation);
    let bx = match unrotated {
        Some(u) if truthy_num(rot) => u,
        _ => rect,
    };
    let mut slice = IngestSlice::geom(Geom::Text {
        rect: pdf_to_content_rect(bx, crop),
        callout: None,
        rot: truthy_num(rot).then_some(rot),
    });
    slice.text = Some(text_from_dto(d));
    slice
}

pub fn geometry(a: &Annot, crop: PdfRect) -> Option<Wire> {
    if !matches!(a.geom, Geom::Text { .. }) {
        return None;
    }
    Some(callout_fields(a, crop).unwrap_or_else(|| box_emit(a, crop)))
}

pub fn draft_extras(a: &Annot, crop: PdfRect) -> Wire {
    let mut out = Wire::new();
    let intent = if callout_fields(a, crop).is_some() { "free-text-callout" } else { "free-text" };
    out.insert("intent".into(), json!(intent));
    let contents = a
        .data
        .as_ref()
        .and_then(|d| get(d, "contents"))
        .filter(|c| !c.is_null())
        .cloned()
        .unwrap_or_else(|| json!(""));
    out.insert("contents".into(), contents);
    out
}
