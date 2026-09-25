//! `types.ts`: the slice, its actions, the pending item and the label patch.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::js::Nullable;
use epdf_kernel::{Error, Token};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::RedactionCapability;

/// `RedactionApplyResult` from `engine-core`, or the `redaction.applied`
/// event that carries one. The plugin never reads inside it, so it stays the
/// engine's JSON.
pub type RedactionApplyResult = Value;

/// One pending redaction mark: a live projection of a `redact` annotation on
/// the annotation plane. The plugin owns no mark state of its own; delete
/// the annotation and the pending item is gone.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionPendingItem {
    /// Stable list key derived from the ref (the annotation model id).
    pub id: String,
    /// The DTO's `AnnotationRef`, verbatim.
    #[serde(rename = "ref")]
    pub ref_: Value,
    pub page_object_number: u32,
    /// Display index (0-based) from the page registry, `-1` when the page
    /// has none.
    pub page_index: f64,
    pub kind: PendingKind,
    /// `/OverlayText`: a string, `null`, or absent when the DTO has no such
    /// member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay_text: Option<Value>,
}

/// `area` is a rect-only mark (marquee); `text` has per-line quads
/// (selection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingKind {
    Area,
    Text,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionState {
    /// A destructive apply is in flight.
    pub applying: bool,
    /// The last apply result seen (own or remote), for status UI.
    pub last_result: Option<RedactionApplyResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RedactionAction {
    #[serde(rename = "APPLY_STARTED")]
    ApplyStarted,
    #[serde(rename = "APPLY_FINISHED")]
    ApplyFinished {
        #[serde(default)]
        result: Option<RedactionApplyResult>,
    },
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

/// An `onApplied` listener. An `Err` is the listener throwing: it is
/// reported to [`RedactionHost::console_error`] and the next listener runs.
pub type AppliedCallback = Rc<dyn Fn(&RedactionApplyResult) -> Result<(), Error>>;

/// The ambient value the TypeScript reads from the global scope.
#[derive(Clone, Default)]
pub struct RedactionHost {
    /// `console.error(message, error)`: receives a throwing `onApplied`
    /// listener's error. `None` drops it.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

pub static REDACTION: LazyLock<Token<RedactionCapability>> =
    LazyLock::new(|| Token::new("redaction"));
