//! `contract.ts`: the public redaction vocabulary. Marking is annotation
//! authoring (a mark is a `redact` annotation); applying is the destructive
//! document mutation. `RedactionApplyResult` and the `redaction.applied`
//! event stay the engine's JSON: the plugin never reads inside them.

use std::rc::Rc;

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{AnnotationRef, PageRef, Rect};
use epdf_kernel::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `RedactionApplyResult` from `engine-core`, or the `redaction.applied`
/// event that carries one.
pub type RedactionApplyResult = Value;

/// `ChangeOrigin`, as JSON: `{locality, trigger, sessionId, actorId}`.
pub type ChangeOrigin = Value;

/// The overlay label's look.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionOverlayText {
    /// CSS hex colour of the overlay label. Default white.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub font_family: Option<String>,
    /// Points; 0 = auto-fit.
    #[serde(default)]
    pub font_size: Option<f64>,
}

/// Defaults for marks created through this plugin's verbs: the applied
/// fill and the overlay label's look.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RedactionOverlay {
    /// CSS hex colour painted over the region on apply. Default black.
    #[serde(default)]
    pub fill: Option<String>,
    #[serde(default)]
    pub text: Option<RedactionOverlayText>,
}

/// The one ambient value the TypeScript reads from the global scope.
#[derive(Clone, Default)]
pub struct RedactionHost {
    /// `console.error(message, error)`: receives a throwing event listener's
    /// error. `None` drops it.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `redactionPlugin(config)`. `host` is the Rust port's way in for what the
/// TypeScript reads from the global scope.
#[derive(Clone, Default, Deserialize)]
pub struct RedactionConfig {
    #[serde(default)]
    pub overlay: Option<RedactionOverlay>,
    #[serde(skip)]
    pub host: RedactionHost,
}

/// `area` is a rect-only mark (marquee); `text` has per-line quads
/// (selection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RedactionMarkKind {
    Area,
    Text,
}

/// One pending redaction mark: a live projection of a `redact` annotation
/// on the annotation plane (delete the annotation and the mark is gone).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionMark {
    #[serde(rename = "ref")]
    pub ref_: AnnotationRef,
    pub page: PageRef,
    /// Display index (0-based) from the page registry, `-1` when the page
    /// has none.
    pub page_index: i64,
    pub kind: RedactionMarkKind,
    /// Page-space bounds of the region.
    pub bounds: Rect,
    /// `/OverlayText`, when set. The DTO's own member, so `None` is a DTO
    /// without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay_text: Option<Value>,
}

/// `RedactionMarkFilter`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RedactionMarkFilter {
    #[serde(default)]
    pub page: Option<PageRef>,
}

/// A label edit: `overlay_text: Null` clears it, `Undefined` leaves it;
/// `repeat` tiles it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionLabelPatch {
    #[serde(default)]
    pub overlay_text: Nullable<String>,
    #[serde(default)]
    pub repeat: Option<bool>,
}

/// Which other annotations an apply would destroy (a client-side estimate;
/// the result is authoritative). `refs` are the DTOs' own refs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionCollateral {
    pub count: usize,
    pub refs: Vec<Value>,
}

/// `RedactionAppliedEvent`: after any confirmed apply, own or remote.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionAppliedEvent {
    pub result: RedactionApplyResult,
    pub origin: ChangeOrigin,
}

/// `RedactionPendingChangedEvent`: marks were created, changed or removed
/// on these pages.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionPendingChangedEvent {
    pub pages: Vec<PageRef>,
}
