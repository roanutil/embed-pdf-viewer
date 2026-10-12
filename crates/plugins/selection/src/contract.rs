//! `contract.ts` and `token.ts`: the public range and read vocabulary, the
//! events, the configuration and the token.
//!
//! Selection ranges live in character space (half-open [`TextRange`], the
//! space search hits address). Segments, endpoints and anchors are in page
//! space (y-down, PDF units, crop-relative).

use std::sync::LazyLock;

use epdf_geometry::{Rect, TextQuad};
use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use epdf_plugin_interaction::page_ref_json;
use serde::{Deserialize, Serialize};

use crate::controller::SelectionCapability;
pub use crate::geometry::SelectionSegment;

// ── configuration ─────────────────────────────────────────────────────────────

/// `SelectionConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionConfig {
    /// Viewport px the pointer must travel before a press becomes a drag-select (default 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drag_threshold: Option<f64>,
}

// ── the range vocabulary ──────────────────────────────────────────────────────

/// A position in a page's character space. Indices are `i64`: `selectAll`
/// parks its open end at `Number.MAX_SAFE_INTEGER` and `select` accepts a
/// negative start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPosition {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    pub index: i64,
}

/// A half-open range in character space, in document order.
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

// ── the read model ────────────────────────────────────────────────────────────

/// A selection boundary on its boundary glyph's own oriented cell.
/// `advance` is the reading direction of its segment (1 is the frame's +x).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionEndpoint {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    pub glyph_quad: TextQuad,
    pub advance: i32,
    pub rect: Rect,
}

/// Where selection-scoped floating UI attaches.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionAnchor {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionDirection {
    Forward,
    Backward,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPage {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    pub segments: Vec<SelectionSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionSnapshot {
    pub pages: Vec<SnapshotPage>,
    pub start: Option<SelectionEndpoint>,
    pub end: Option<SelectionEndpoint>,
    pub direction: SelectionDirection,
    /// A valid `select` input; `None` with nothing selected.
    pub range: Option<TextRange>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `ChangeOrigin['trigger']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeTrigger {
    User,
    Api,
    Script,
    System,
    Unknown,
}

/// `ChangeOrigin`. Selection only ever reports local changes, with no
/// session or actor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeOrigin {
    pub locality: String,
    pub trigger: ChangeTrigger,
    pub session_id: Option<String>,
    pub actor_id: Option<String>,
}

impl ChangeOrigin {
    /// `localOrigin(trigger)`.
    pub fn local(trigger: ChangeTrigger) -> Self {
        ChangeOrigin { locality: "local".to_string(), trigger, session_id: None, actor_id: None }
    }
}

fn ser_pages<S: serde::Serializer>(pages: &[PageRef], s: S) -> Result<S::Ok, S::Error> {
    pages.iter().map(|p| p.to_json()).collect::<Vec<_>>().serialize(s)
}

/// Any change to the selection: a gesture step, a programmatic select, a
/// derived recompute, or a clear.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionChangedEvent {
    pub range: Option<TextRange>,
    /// The pages that carry a segment right now.
    #[serde(serialize_with = "ser_pages")]
    pub pages: Vec<PageRef>,
    pub origin: ChangeOrigin,
}

/// A selection gesture ended with a selection in place.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionCommittedEvent {
    pub range: TextRange,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionClearedEvent {
    pub origin: ChangeOrigin,
}

// ── the token ─────────────────────────────────────────────────────────────────

/// The selection token. The TypeScript types it twice (the public lens from
/// the package root, the host lens from `/contract/host` and `/internal`)
/// over one runtime object; here it is one token and one type.
pub static SELECTION: LazyLock<Token<SelectionCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "selection",
        "add selectionPlugin() from '@embedpdf/plugin-selection' to your plugins list",
    )
});
