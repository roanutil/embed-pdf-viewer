//! A scale and a page's measurement viewports in page space
//! (`core/src/pageSpace/measure.ts`).
//!
//! A scale's origin (`/O`) is a place on the page; a viewport's `/BBox` is
//! a box on it. Nothing else in a measure holds a place. A foreign measure
//! (`geospatial`, `unknown`) is its subtype alone and converts to itself.

use serde_json::Value;

use super::{page_point_of, page_rect_of, pdf_point_of};
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PagePoint, PdfPoint, PdfRect, normalize_pdf_rect};

/// `withOrigin`: a rectilinear measure's origin converted, every other
/// measure as it came. An origin that isn't a point goes as it came, for
/// `assertWritableMeasure` to refuse.
fn with_origin(mut measure: Value, convert: impl FnOnce(Value) -> Option<Value>) -> Value {
    if measure.get("subtype").and_then(Value::as_str) != Some("rectilinear") {
        return measure;
    }
    if let Some(origin) = measure.get_mut("origin").filter(|o| !o.is_null())
        && let Some(converted) = convert(origin.clone())
    {
        *origin = converted;
    }
    measure
}

/// A scale in page space (`pageMeasureOf`): its origin a place on the page
/// whose visible box is `visible`.
pub fn page_measure_of(measure: Value, visible: PdfRect) -> Value {
    with_origin(measure, |origin| {
        let point: PdfPoint = serde_json::from_value(origin).ok()?;
        serde_json::to_value(page_point_of(point, visible)).ok()
    })
}

/// A page-space scale in the file's coordinates (`pdfMeasureOf`).
pub fn pdf_measure_of(measure: Value, visible: PdfRect) -> Value {
    with_origin(measure, |origin| {
        let point: PagePoint = serde_json::from_value(origin).ok()?;
        serde_json::to_value(pdf_point_of(point, visible)).ok()
    })
}

/// A page's measurement viewports in page space (`pageViewportsOf`): each
/// `bbox` a box on the page, each measure's origin a place on it.
pub fn page_viewports_of(viewports: Value, visible: PdfRect) -> Result<Value, EngineError> {
    let Value::Array(viewports) = viewports else {
        return Err(malformed("the viewports are not a list"));
    };
    let mut out = Vec::with_capacity(viewports.len());
    for mut viewport in viewports {
        let bbox: PdfRect = serde_json::from_value(viewport["bbox"].take())
            .map_err(|e| malformed(format!("a viewport's bbox is not a rect: {e}")))?;
        viewport["bbox"] = serde_json::to_value(page_rect_of(normalize_pdf_rect(bbox), visible))
            .map_err(|e| malformed(e.to_string()))?;
        let measure = viewport["measure"].take();
        viewport["measure"] =
            if measure.is_null() { Value::Null } else { page_measure_of(measure, visible) };
        out.push(viewport);
    }
    Ok(Value::Array(out))
}

fn malformed(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

#[cfg(test)]
mod tests {
    //! `core/test/pageSpace/features.test.ts`, "a viewport and a scale
    //! origin".
    use super::*;
    use serde_json::json;

    const CROP: PdfRect = PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 };

    #[test]
    fn a_viewport_and_a_scale_origin() {
        let viewports = page_viewports_of(
            json!([{
                "bbox": { "left": 50.0, "bottom": 60.0, "right": 562.0, "top": 732.0 },
                "name": "plan",
                "owned": true,
                "measure": { "subtype": "rectilinear", "x": [], "distance": [], "area": [],
                    "origin": { "x": 50.0, "y": 732.0 } },
            }]),
            CROP,
        )
        .unwrap();
        assert_eq!(
            viewports[0]["bbox"],
            json!({ "x": 0.0, "y": 0.0, "width": 512.0, "height": 672.0 })
        );
        assert_eq!(viewports[0]["measure"]["origin"], json!({ "x": 0.0, "y": 0.0 }));
        assert_eq!(viewports[0]["name"], json!("plan"));
        assert_eq!(
            pdf_measure_of(
                json!({ "subtype": "rectilinear", "x": [], "distance": [], "area": [],
                    "origin": { "x": 0, "y": 0 } }),
                CROP,
            )["origin"],
            json!({ "x": 50.0, "y": 732.0 })
        );
        assert_eq!(
            pdf_measure_of(json!({ "subtype": "geospatial" }), CROP),
            json!({ "subtype": "geospatial" })
        );
    }

    #[test]
    fn a_viewport_without_a_measure_keeps_its_null() {
        let viewports = page_viewports_of(
            json!([{ "bbox": { "left": 50.0, "bottom": 60.0, "right": 150.0, "top": 160.0 },
                "name": null, "owned": true, "measure": null }]),
            CROP,
        )
        .unwrap();
        assert_eq!(viewports[0]["measure"], Value::Null);
        assert_eq!(viewports[0]["name"], Value::Null);
        assert_eq!(
            viewports[0]["bbox"],
            json!({ "x": 0.0, "y": 572.0, "width": 100.0, "height": 100.0 })
        );
    }
}
