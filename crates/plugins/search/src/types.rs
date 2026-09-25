//! `types.ts`, plus the engine-core search DTOs the plugin reads
//! (`search/types.ts`, `text/layout.ts`'s `PdfTextSegment`).

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::{Quad, Rect, TextQuad};
use epdf_kernel::Token;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use serde::{Deserialize, Serialize};

use crate::capability::SearchCapability;

/// `PageObjectNumber`, as the engine contract spells a page.
pub type PageObjectNumber = u32;

// ── the engine's DTOs ─────────────────────────────────────────────────────────

/// `SearchQuery`: THE query shape, engine to wire to state to search box.
/// The flags stay absent when the caller left them absent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// The literal text, or with `regex` a pattern in the portable dialect.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_case: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_word: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_diacritics: Option<bool>,
}

/// `SearchMode`: how much of a match leaves the engine. `Rects` needs
/// `doc.text.search`; `Full` (a snippet per match) also `doc.text.copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Rects,
    Full,
}

/// `SearchSliceBudget`: per-slice spending caps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSliceBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_matches: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<f64>,
}

/// `SearchRequest`: one bounded slice of work.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: SearchQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SearchMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_page: Option<PageObjectNumber>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<SearchSliceBudget>,
}

/// `SearchSnippet`: context around one match, `Full` mode only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippet {
    pub text: String,
    pub match_start: f64,
    pub match_length: f64,
}

/// `PdfRect`: PDF user space, y-up edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// `PdfTextSegment`: one canonical visual line of a match, in PDF space.
/// `quad`'s slots are frame-geometric (`p1..p4` = upper-start, upper-end,
/// lower-start, lower-end).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfTextSegment {
    pub quad: Quad,
    pub rect: PdfRect,
    pub advance: f64,
}

/// `SearchMatch`: one hit as the engine reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub page_object_number: PageObjectNumber,
    pub char_start: f64,
    pub char_count: f64,
    pub segments: Vec<PdfTextSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<SearchSnippet>,
}

/// `SearchSlice`: the result of one bounded slice. `next_cursor: None` (the
/// engine's `null`) means the search space is exhausted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSlice {
    pub matches: Vec<SearchMatch>,
    pub next_cursor: Option<String>,
    pub scanned_pages: f64,
    pub total_pages: f64,
}

// ── the plugin's types ────────────────────────────────────────────────────────

/// One merged visual line of a match, in content space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// The reading direction along the baseline: `1` or `-1`.
    pub advance: f64,
}

/// One match, viewer-shaped: content-space segments plus the page's display
/// index, with the engine's text-page offsets kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub pon: PageObjectNumber,
    pub page_index: usize,
    pub char_start: f64,
    pub char_count: f64,
    pub segments: Vec<TextSegment>,
    /// The union of the segments; absent when the match has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<SearchSnippet>,
}

/// `searching` means the slice loop is still walking pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchStatus {
    Idle,
    Searching,
    Complete,
    Error,
}

/// `SearchExecOptions`: how the session scan runs, never what it means.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchExecOptions {
    /// Scan origin. Defaults to the Stage's current page.
    pub start_page: Option<PageObjectNumber>,
}

/// `SearchFindAllOptions`.
#[derive(Clone, Default)]
pub struct SearchFindAllOptions {
    /// Abort the scan; the promise rejects with the signal's reason.
    pub signal: Option<AbortSignal>,
    /// Pin the slice mode. `None` is `Full` with the automatic `Rects`
    /// fallback when snippets are denied.
    pub mode: Option<SearchMode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchProgress {
    pub scanned: f64,
    pub total: f64,
}

/// `SearchState`. `hits` and `hits_by_page` are shared with every state
/// that did not change them, as the TypeScript's spreads share them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchState {
    /// The query in flight or completed; `None` when idle.
    pub query: Option<SearchQuery>,
    pub status: SearchStatus,
    pub hits: Rc<Vec<SearchHit>>,
    /// pon to indices into `hits`.
    pub hits_by_page: Rc<BTreeMap<PageObjectNumber, Vec<usize>>>,
    /// Index into `hits`, `-1` for none.
    pub active_index: isize,
    pub progress: SearchProgress,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SearchAction {
    #[serde(rename = "START")]
    Start { query: SearchQuery },
    #[serde(rename = "APPEND")]
    Append { hits: Vec<SearchHit>, scanned: f64, total: f64 },
    #[serde(rename = "COMPLETE")]
    Complete,
    #[serde(rename = "ERROR")]
    Error { message: String },
    #[serde(rename = "SET_ACTIVE")]
    SetActive { index: isize },
    #[serde(rename = "CLEAR")]
    Clear,
}

/// `SearchRevealOptions`: how hit navigation arrives, merged per key as
/// `{...default, ...config.reveal, ...perCall}`.
///
/// Each member is two options because a spread copies a key that is
/// present with the value `undefined`: the outer `None` is an absent key
/// (the layer below shows through), `Some(None)` a key present as
/// `undefined` (or, for `behavior`, `null`, which the Stage reads the same
/// way), which overrides the layer below away. A `null` anchor is present
/// and positions the reveal with no axis set, which is
/// `Some(Some(RevealAnchor::default()))`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchRevealOptions {
    pub anchor: Option<Option<RevealAnchor>>,
    pub behavior: Option<Option<ScrollBehaviorKind>>,
}

/// `SearchPluginConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchPluginConfig {
    /// Arrival defaults for `next`, `prev` and `go_to`.
    pub reveal: Option<SearchRevealOptions>,
}

pub static SEARCH: LazyLock<Token<SearchCapability>> = LazyLock::new(|| Token::new("search"));
