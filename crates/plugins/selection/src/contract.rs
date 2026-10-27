//! `contract.ts` and `token.ts`: the settings, the public range and read
//! vocabulary, the events and the token.
//!
//! Selection ranges live in character space (half-open [`TextRange`], the
//! space search hits address). Segments, endpoints and anchors are in page
//! space (y-down, PDF units, crop-relative).

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::{Quad, Rect};
use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::controller::SelectionCapability;
pub use crate::geometry::SelectionSegment;

// ── settings ──────────────────────────────────────────────────────────────────

/// The grips at the selection's ends, on touch screens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionHandleSettings {
    /// A CSS color, or `None` for the viewer's accent.
    pub color: Option<String>,
    /// A CSS `box-shadow`.
    pub shadow: String,
}

/// `SelectionSettings`: what `selection_plugin(config)` registers over
/// [`selection_defaults`], and `update_settings` changes for every document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSettings {
    /// Viewport px the pointer travels before a press becomes a drag that selects.
    pub drag_threshold: f64,
    /// The color of selected text, or `None` for the viewer's accent at 35%.
    pub color: Option<String>,
    pub handles: SelectionHandleSettings,
}

/// `SELECTION_DEFAULTS`.
pub fn selection_defaults() -> SelectionSettings {
    SelectionSettings {
        drag_threshold: 4.0,
        color: None,
        handles: SelectionHandleSettings {
            color: None,
            shadow: "0 1px 4px rgb(0 0 0 / 0.35)".to_string(),
        },
    }
}

/// A `DeepPartial<SelectionSettings['handles']>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionHandleConfig {
    /// `Some(None)` sets the color back to the accent.
    pub color: Option<Option<String>>,
    pub shadow: Option<String>,
}

/// `SelectionConfig`: any of the settings, merged over the defaults.
/// `Some(None)` for a color is the `null` that means the accent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionConfig {
    pub drag_threshold: Option<f64>,
    pub color: Option<Option<String>>,
    pub handles: Option<SelectionHandleConfig>,
}

impl SelectionConfig {
    /// The members it names, as the settings store keeps them.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(threshold) = self.drag_threshold {
            out.insert("dragThreshold".into(), json!(threshold));
        }
        if let Some(color) = &self.color {
            out.insert("color".into(), json!(color));
        }
        if let Some(handles) = &self.handles {
            let mut h = Map::new();
            if let Some(color) = &handles.color {
                h.insert("color".into(), json!(color));
            }
            if let Some(shadow) = &handles.shadow {
                h.insert("shadow".into(), json!(shadow));
            }
            out.insert("handles".into(), Value::Object(h));
        }
        Value::Object(out)
    }

    /// Read a `DeepPartial<SelectionSettings>`: members of the wrong type
    /// are ignored.
    pub fn from_json(value: &Value) -> Self {
        let text = |v: &Value| match v {
            Value::Null => Some(None),
            Value::String(s) => Some(Some(s.clone())),
            _ => None,
        };
        let handles =
            value.get("handles").filter(|h| h.is_object()).map(|h| SelectionHandleConfig {
                color: h.get("color").and_then(text),
                shadow: h.get("shadow").and_then(Value::as_str).map(str::to_string),
            });
        SelectionConfig {
            drag_threshold: value.get("dragThreshold").and_then(Value::as_f64),
            color: value.get("color").and_then(text),
            handles,
        }
    }
}

/// `SettingsChanged<SelectionSettings>`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionSettingsChanged {
    pub settings: SelectionSettings,
    pub changed: Vec<String>,
}

// ── the range vocabulary ──────────────────────────────────────────────────────

/// A position in a page's character space. `i64`: `select_all` parks its
/// open end at `Number.MAX_SAFE_INTEGER` and `select` accepts a negative
/// start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPosition {
    pub page: PageRef,
    pub index: i64,
}

/// A half-open range in character space, in document order. `end.index`
/// may be 0 to end a range exactly at a page boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

/// What `select` accepts: a cross-page range, or a single-page span (a
/// search hit's shape).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionRangeInput {
    Range(TextRange),
    Span { page: PageRef, start: i64, count: i64 },
}

impl SelectionRangeInput {
    /// A span from a search hit's `f64` fields:
    /// `SelectionRangeInput::span(hit.page, hit.char_start, hit.char_count)`
    /// needs no casts, as `select({ page, start: hit.charStart, count:
    /// hit.charCount })` needs none in the TypeScript. Fractions truncate,
    /// values past `i64` saturate and NaN is 0 (Rust's `as`); a hit's are
    /// whole numbers.
    pub fn span(page: PageRef, start: f64, count: f64) -> Self {
        SelectionRangeInput::Span { page, start: start as i64, count: count as i64 }
    }
}

/// A page argument: a `PageRef`, or a zero-based index into the document's
/// page list (`PageRef | number`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageTarget {
    Ref(PageRef),
    Index(f64),
}

impl From<PageRef> for PageTarget {
    fn from(page: PageRef) -> Self {
        PageTarget::Ref(page)
    }
}

impl From<usize> for PageTarget {
    fn from(index: usize) -> Self {
        PageTarget::Index(index as f64)
    }
}

// ── the read model ────────────────────────────────────────────────────────────

/// A selection boundary, on its boundary glyph's own oriented cell.
/// `advance` is the reading direction of its segment (1 is the frame's +x).
/// `rect` is the cell's bounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionEndpoint {
    pub page: PageRef,
    pub glyph_quad: Quad,
    pub advance: i32,
    pub rect: Rect,
}

/// Where selection-scoped floating UI attaches: a page and the selection's
/// union box on it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionAnchor {
    pub page: PageRef,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionDirection {
    Forward,
    Backward,
}

/// One page of a snapshot: its segments, the same list the state holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPage {
    pub page: PageRef,
    pub segments: Rc<Vec<SelectionSegment>>,
}

/// `SelectionSnapshot`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionSnapshot {
    pub pages: Vec<SnapshotPage>,
    pub start: Option<SelectionEndpoint>,
    pub end: Option<SelectionEndpoint>,
    pub direction: SelectionDirection,
    /// A valid `select` input; `None` with nothing selected. A boundary
    /// page's index may exceed its character count until its geometry
    /// loads; `select` and `read_text_in_range` clamp.
    pub range: Option<TextRange>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// The range or its segments changed. Selection is session state, so the
/// event carries no document origin.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionChangedEvent {
    pub range: Option<TextRange>,
    /// The pages that carry a segment right now.
    pub pages: Vec<PageRef>,
}

/// A selection gesture ended with a selection in place. Never fires for a
/// selection made from code.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionCommittedEvent {
    pub range: TextRange,
}

/// The selection went from a range to nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SelectionClearedEvent {}

// ── the token ─────────────────────────────────────────────────────────────────

/// The selection token. The TypeScript types it twice (the public lens and
/// the host lens from `/contract/host`) over one runtime object; here it is
/// one token and one type.
pub static SELECTION: LazyLock<Token<SelectionCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "selection",
        "add selectionPlugin() from '@embedpdf/plugin-selection' to your plugins list",
    )
});
