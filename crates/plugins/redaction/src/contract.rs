//! `contract.ts`: the public redaction vocabulary. Marking is annotation
//! authoring (a mark is a `redact` annotation); applying is the destructive
//! document mutation. The plugin owns no mark state: the pending view is a
//! live projection of the annotation plane.
//!
//! Annotation refs and records stay the engine's JSON (`Value`), as the
//! annotation plugin hands them over; `RedactionApplyResult` is the engine's
//! JSON too, which the plugin never reads inside.

use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use epdf_plugin_search::PageTarget;

/// `RedactionApplyResult` from `engine-core`: `{scope, results, removedAnnotationCount, meta}`.
pub type RedactionApplyResult = Rc<Value>;

/// The label's look, as the settings hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct RedactionOverlayText {
    /// The color of a label.
    pub color: String,
    /// The label's font: a standard font or a registered one.
    pub font_family: String,
    /// The label's size in points; `0` fits it to the area.
    pub font_size: f64,
}

/// What applying paints over every mark.
#[derive(Clone, Debug, PartialEq)]
pub struct RedactionOverlay {
    /// The color of a redacted area.
    pub fill: String,
    pub text: RedactionOverlayText,
}

/// `RedactionSettings`: PDF colors and fonts written into each mark, the
/// redact tool's defaults from then on.
#[derive(Clone, Debug, PartialEq)]
pub struct RedactionSettings {
    pub overlay: RedactionOverlay,
}

impl RedactionSettings {
    /// The settings as the kernel's store holds them. A member of the wrong
    /// type reads as the default, which the store never holds after a typed
    /// write.
    pub fn of(value: &Value) -> RedactionSettings {
        let defaults = redaction_settings_defaults();
        let overlay = value.get("overlay");
        let text = overlay.and_then(|o| o.get("text"));
        let string = |v: Option<&Value>, fallback: &str| {
            v.and_then(Value::as_str).unwrap_or(fallback).to_string()
        };
        RedactionSettings {
            overlay: RedactionOverlay {
                fill: string(overlay.and_then(|o| o.get("fill")), &defaults.overlay.fill),
                text: RedactionOverlayText {
                    color: string(text.and_then(|t| t.get("color")), &defaults.overlay.text.color),
                    font_family: string(
                        text.and_then(|t| t.get("fontFamily")),
                        &defaults.overlay.text.font_family,
                    ),
                    font_size: text
                        .and_then(|t| t.get("fontSize"))
                        .and_then(epdf_annotation_core::json::number)
                        .unwrap_or(defaults.overlay.text.font_size),
                },
            },
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "overlay": {
                "fill": self.overlay.fill,
                "text": {
                    "color": self.overlay.text.color,
                    "fontFamily": self.overlay.text.font_family,
                    "fontSize": self.overlay.text.font_size,
                },
            },
        })
    }
}

/// `REDACTION_DEFAULTS`: what the settings are when the app registers none.
pub fn redaction_settings_defaults() -> RedactionSettings {
    RedactionSettings {
        overlay: RedactionOverlay {
            fill: "#000000".into(),
            text: RedactionOverlayText {
                color: "#ffffff".into(),
                font_family: "helvetica".into(),
                font_size: 0.0,
            },
        },
    }
}

/// `REDACTION_DEFAULTS` as the JSON the settings store merges over.
pub fn redaction_defaults() -> Value {
    redaction_settings_defaults().to_json()
}

/// The label part of a [`RedactionConfig`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedactionOverlayTextConfig {
    pub color: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
}

/// The overlay part of a [`RedactionConfig`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedactionOverlayConfig {
    pub fill: Option<String>,
    pub text: Option<RedactionOverlayTextConfig>,
}

/// `RedactionConfig` (`DeepPartial<RedactionSettings>`): what
/// `redactionPlugin(config)` takes, any of the settings merged over the
/// defaults. Also the change `update_settings` takes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedactionConfig {
    pub overlay: Option<RedactionOverlayConfig>,
}

impl RedactionConfig {
    /// The partial as the settings store merges it: a member left `None` is
    /// absent.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(overlay) = &self.overlay {
            let mut o = Map::new();
            if let Some(fill) = &overlay.fill {
                o.insert("fill".into(), json!(fill));
            }
            if let Some(text) = &overlay.text {
                let mut t = Map::new();
                if let Some(color) = &text.color {
                    t.insert("color".into(), json!(color));
                }
                if let Some(family) = &text.font_family {
                    t.insert("fontFamily".into(), json!(family));
                }
                if let Some(size) = text.font_size {
                    t.insert("fontSize".into(), json!(size));
                }
                o.insert("text".into(), Value::Object(t));
            }
            out.insert("overlay".into(), Value::Object(o));
        }
        Value::Object(out)
    }
}

/// `{ settings, changed }`: the settings in effect after a change, and the
/// top-level members it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct RedactionSettingsChanged {
    pub settings: RedactionSettings,
    pub changed: Vec<String>,
}

/// `area` is a rect-only mark (marquee); `text` has per-line quads
/// (selection or search).
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
    /// The annotation's ref, as the engine's record carries it.
    #[serde(rename = "ref")]
    pub ref_: Value,
    pub page: PageRef,
    /// Display index (0-based) from the page registry, for "page N" labels;
    /// `-1` for a page the registry lacks.
    pub page_index: i64,
    pub kind: RedactionMarkKind,
    /// Page-space bounds of the region.
    pub bounds: Rect,
    /// `/OverlayText`, when set.
    pub overlay_text: Option<String>,
    /// Whether the label fills the area over and over, or shows once.
    pub repeat: bool,
}

/// `RedactionMarkFilter`: one page's marks, by its ref or its index.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RedactionMarkFilter {
    pub page: Option<PageTarget>,
}

/// A label edit. `overlay_text`: `None` leaves it, `Some(None)` clears it,
/// `Some(Some(text))` sets it. `repeat` tiles it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedactionLabelPatch {
    pub overlay_text: Option<Option<String>>,
    pub repeat: Option<bool>,
}

/// Which other annotations an apply would destroy (a client-side estimate;
/// the result is authoritative). `refs` are the records' own refs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionCollateral {
    pub count: usize,
    pub refs: Vec<Value>,
}

/// `OperationOptions`: a caller's cancel.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `{ ref, reason }`: a ref the batch passed over.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatchSkip {
    #[serde(rename = "ref")]
    pub ref_: Value,
    pub reason: String,
}

/// `{ ref, error }`: a ref the batch could not handle, with the
/// `PluginErrorInfo` (`{code, message, capability, permission}`) as JSON.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatchFailure {
    #[serde(rename = "ref")]
    pub ref_: Value,
    pub error: Value,
}

/// `BatchResult<AnnotationRef, AnnotationRef>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BatchResult {
    pub applied: Vec<Value>,
    pub skipped: Vec<BatchSkip>,
    pub failed: Vec<BatchFailure>,
}

/// `{ marks }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MarksResult {
    pub marks: Vec<RedactionMark>,
}

/// `{ mark }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MarkResult {
    pub mark: RedactionMark,
}

/// `RedactionAppliedEvent`: a confirmed apply, from this session or
/// another. `origin` is the event's `EventOrigin`, as JSON.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionAppliedEvent {
    pub result: RedactionApplyResult,
    pub origin: Value,
}

/// `RedactionPendingChangedEvent`: marks were created, changed or removed
/// on these pages.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RedactionPendingChangedEvent {
    pub pages: Vec<Value>,
}
