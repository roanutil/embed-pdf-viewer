//! `host-contract.ts` and `priorities.ts`: the host lens's own types (the
//! measurement seam's report and captured draft, the ghost preview bucket,
//! the hover input) and the interaction-hub priorities of this plugin's
//! handlers.

use epdf_annotation_core::{AnnotationRef, PageRef, Point};
use epdf_kernel::Error;
use serde::Serialize;
use serde_json::{Value, json};

/// `ANNOTATION_GHOST_PRIORITY`: never captures; hides the ghost on every down.
pub const ANNOTATION_GHOST_PRIORITY: f64 = 1000.0;
/// `ANNOTATION_EDIT_PRIORITY`: select, move and resize over an annotation.
pub const ANNOTATION_EDIT_PRIORITY: f64 = 100.0;
/// `ANNOTATION_PLACE_PRIORITY`: click-to-place on empty page space.
pub const ANNOTATION_PLACE_PRIORITY: f64 = 95.0;
/// `ANNOTATION_DRAW_PRIORITY`: drag-create under a draw tool.
pub const ANNOTATION_DRAW_PRIORITY: f64 = 55.0;
/// `ANNOTATION_MARQUEE_PRIORITY`: empty-space selection, the last fallback.
pub const ANNOTATION_MARQUEE_PRIORITY: f64 = 50.0;

/// `previewBucket(devicePixelWidth, cap = 4096)`: powers of two from 128 px
/// up to `cap`.
pub fn preview_bucket(device_pixel_width: f64, cap: Option<f64>) -> f64 {
    let cap = cap.unwrap_or(4096.0);
    let px = epdf_geometry::js::max(128.0, device_pixel_width.ceil());
    epdf_geometry::js::min(cap, 2f64.powf(px.log2().ceil()))
}

/// Why `remeasurePage` left a dimension alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    NoAuthority,
    Locked,
    ForeignMeasure,
    Unavailable,
}

/// `RecalibrationReport`. `scale` is the `PdfMeasure` as JSON; errors are
/// `SerializedEngineError`s.
#[derive(Clone, Debug, PartialEq)]
pub struct RecalibrationReport {
    pub page: PageRef,
    pub scale: Value,
    pub error: Option<Value>,
    pub updated: Vec<AnnotationRef>,
    pub skipped: Vec<(AnnotationRef, SkipReason)>,
    pub failed: Vec<(AnnotationRef, Value)>,
}

impl RecalibrationReport {
    pub fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        out.insert("page".into(), json!(self.page));
        out.insert("scale".into(), self.scale.clone());
        out.insert("updated".into(), json!(self.updated));
        out.insert(
            "skipped".into(),
            json!(
                self.skipped
                    .iter()
                    .map(|(r, reason)| json!({ "ref": r, "reason": reason }))
                    .collect::<Vec<_>>()
            ),
        );
        out.insert(
            "failed".into(),
            json!(
                self.failed.iter().map(|(r, e)| json!({ "ref": r, "error": e })).collect::<Vec<_>>()
            ),
        );
        if let Some(error) = &self.error {
            out.insert("error".into(), error.clone());
        }
        Value::Object(out)
    }
}

/// `serializeError(error)` (engine-core): an `EngineError` as its
/// `toJSON()`, anything else as an `EngineError` coded `Aborted` (for an
/// `AbortError`) or `Unknown`.
pub fn serialize_error(error: &Error) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("name".into(), json!("EngineError"));
    if error.name == "EngineError" {
        out.insert("code".into(), error.code().map_or(Value::Null, |c| json!(c)));
        out.insert("message".into(), json!(error.message));
        if let Some(details) = error.member("details").filter(|d| !d.is_null()) {
            out.insert("details".into(), details.clone());
        }
    } else {
        let code = if error.name == "AbortError" { "Aborted" } else { "Unknown" };
        out.insert("code".into(), json!(code));
        out.insert("message".into(), json!(error.message));
    }
    Value::Object(out)
}

/// `CapturedAnnotationDraft`: a capture tool's line, in PDF user space.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CapturedAnnotationDraft {
    pub tool: String,
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
}

/// `hoverAt`'s argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverAt {
    pub page: PageRef,
    pub point: Point,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub zoom: Option<f64>,
}
