//! `contract.ts`: the public search vocabulary, plus the engine-core search
//! DTOs the plugin reads (`search/types.ts`, `text/layout.ts`'s
//! `PdfTextSegment`). The capability is [`crate::SearchCapability`]; the
//! token is [`crate::SEARCH`].

use epdf_geometry::{Quad, Rect, TextQuad};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::page_ref_json;
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use serde::{Deserialize, Serialize};

/// `PageObjectNumber`, the number inside a [`PageRef`].
pub type PageObjectNumber = u32;

// ── the engine's DTOs ─────────────────────────────────────────────────────────

/// `SearchQuery`: THE query shape, engine to wire to state to search box.
/// The flags stay absent when the caller left them absent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// The literal text, or with `regex` a pattern in the portable dialect.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub regex: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub match_case: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub whole_word: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub match_diacritics: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub ignore_whitespace: Option<bool>,
}

/// `SearchMode`: how much of a match leaves the engine. `Rects` needs
/// `doc.text.search`; `Full` (a snippet per match) also `doc.text.copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
    #[serde(default, skip_serializing_if = "Option::is_none", with = "page_ref_json::option")]
    pub start_page: Option<PageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<SearchSliceBudget>,
}

/// `SearchSnippet`: context around one match, `Full` mode only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippet {
    pub text: String,
    /// Where the match starts within `text`, in UTF-16 code units.
    pub match_start: f64,
    /// In UTF-16 code units.
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
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
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

/// `TextSegment`: one merged visual line of a match in page space, the
/// structural twin of selection's `SelectionSegment`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// The reading direction along the baseline: `1` or `-1`.
    pub advance: f64,
}

/// `SearchHit`: one match. `page` is the durable identity, `page_index` its
/// display index at the time of the match; `char_start` and `char_count`
/// are engine text-page offsets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
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

/// `searching`: the scan is walking pages. `cancelled`: `cancel()` stopped
/// it and the hits found so far stay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "lowercase")]
pub enum SearchStatus {
    Idle,
    Searching,
    Complete,
    Cancelled,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SearchProgress {
    pub scanned: f64,
    pub total: f64,
}

/// `PluginErrorInfo`: the serializable face of a `PluginError`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginErrorInfo {
    pub code: String,
    pub message: String,
    pub capability: String,
}

impl PluginErrorInfo {
    /// `toPluginErrorInfo(error)` of a `PluginError`.
    pub fn of(error: &epdf_kernel::Error) -> Self {
        let text = |key: &str| {
            error.member(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
        };
        PluginErrorInfo {
            code: text("code"),
            message: error.message.clone(),
            capability: text("capability"),
        }
    }
}

/// `SearchRevealOptions`: how a navigated hit arrives, forwarded to the
/// Stage's positioned reveal and merged per key as `{...default,
/// ...config.reveal, ...perCall}`.
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

/// `OperationOptions`.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `SearchOptions`.
#[derive(Clone, Default)]
pub struct SearchOptions {
    /// Scan origin. Defaults to the Stage's current page when a Stage is
    /// installed.
    pub start_page: Option<PageRef>,
    pub signal: Option<AbortSignal>,
}

/// `SearchFindAllOptions`.
#[derive(Clone, Default)]
pub struct SearchFindAllOptions {
    /// Rejects `operation-cancelled` when it aborts.
    pub signal: Option<AbortSignal>,
    /// Pin the slice mode. `None` is `Full` with the automatic `Rects`
    /// fallback when snippets are denied.
    pub mode: Option<SearchMode>,
}

/// What a `search()` resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "lowercase")]
pub enum SearchResultStatus {
    Complete,
    Superseded,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub status: SearchResultStatus,
    pub hit_count: usize,
}

/// `SearchConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchConfig {
    /// Arrival defaults for `next_hit`, `previous_hit` and `go_to_hit`.
    pub reveal: Option<SearchRevealOptions>,
}

// ── events ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SearchStartedEvent {
    pub query: SearchQuery,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchProgressEvent {
    pub scanned: f64,
    pub total: f64,
    pub hit_count: usize,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCompletedEvent {
    pub hit_count: usize,
    pub operation_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "lowercase")]
pub enum SearchCancelReason {
    Superseded,
    Cancelled,
    Cleared,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SearchCancelledEvent {
    pub operation_id: String,
    pub reason: SearchCancelReason,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFailedEvent {
    pub error: PluginErrorInfo,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchActiveHitChangedEvent {
    pub index: usize,
    pub hit: Option<SearchHit>,
}

/// `SearchClearedEvent`: an empty object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct SearchClearedEvent {}
