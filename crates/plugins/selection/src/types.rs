//! `types.ts`: the range vocabulary (public), the gesture state
//! (host/internal), and the token.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use epdf_geometry::{Rect, TextQuad};
use epdf_kernel::Token;
use serde::{Deserialize, Serialize};

use crate::capability::SelectionCapability;
pub use crate::geometry::SelectionSegment;

pub type PageObjectNumber = u32;

// ── the public range vocabulary ──────────────────────────────────────────────

/// A position in a page's character space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPosition {
    pub pon: PageObjectNumber,
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
    Span { pon: PageObjectNumber, start: i64, count: i64 },
}

// ── gesture-state vocabulary ─────────────────────────────────────────────────

/// A page plus a flat character index within it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphPointer {
    pub pon: PageObjectNumber,
    pub glyph: i64,
}

/// Anchor is where the drag began, focus the current end. Inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionRange {
    pub anchor: GlyphPointer,
    pub focus: GlyphPointer,
}

/// A selection boundary on its boundary glyph's own oriented cell.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionEndpoint {
    pub pon: PageObjectNumber,
    pub glyph_quad: TextQuad,
    pub advance: i32,
    pub rect: Rect,
}

/// Where selection-scoped floating UI attaches.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionMenuAnchor {
    pub pon: PageObjectNumber,
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
    pub pon: PageObjectNumber,
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

/// The slice. `segments` and `loaded` are `Record<number, _>`: integer keys,
/// which JavaScript enumerates in ascending order, as a `BTreeMap` does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionState {
    pub selection: Option<SelectionRange>,
    pub segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>>,
    pub loaded: BTreeMap<PageObjectNumber, bool>,
    pub highlight_hidden: bool,
    pub selecting: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SelectionAction {
    #[serde(rename = "PAGE_LOADED")]
    PageLoaded { pon: PageObjectNumber },
    #[serde(rename = "SET")]
    Set {
        selection: SelectionRange,
        #[serde(deserialize_with = "integer_keys")]
        segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>>,
    },
    #[serde(rename = "CLEAR")]
    Clear,
    #[serde(rename = "SET_HIGHLIGHT_HIDDEN")]
    SetHighlightHidden { hidden: bool },
    #[serde(rename = "SET_SELECTING")]
    SetSelecting { selecting: bool },
}

/// A `Record<number, _>` read back inside a tagged enum, where serde buffers
/// the map and no longer parses integer keys from their string form.
fn integer_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PageObjectNumber, Vec<SelectionSegment>>, D::Error> {
    let raw = BTreeMap::<String, Vec<SelectionSegment>>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| k.parse().map(|k| (k, v)).map_err(serde::de::Error::custom))
        .collect()
}

/// The selection token. The TypeScript types it twice (the public lens from
/// the package root, the host lens from `/contract/host` and `/internal`)
/// over one runtime object; here it is one token and one type.
pub static SELECTION: LazyLock<Token<SelectionCapability>> =
    LazyLock::new(|| Token::new("selection"));
