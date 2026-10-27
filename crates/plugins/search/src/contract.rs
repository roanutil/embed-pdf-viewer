//! `contract.ts`: the public search vocabulary, the settings, and the
//! engine-core search DTOs the plugin reads (`search/types.ts`, and
//! `text/layout.ts`'s `PdfTextSegment` in page space). The capability is
//! [`crate::SearchCapability`]; the token is [`crate::SEARCH`].

use std::rc::Rc;

use epdf_geometry::{Quad, Rect};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use epdf_kernel::types::PluginErrorInfo;
/// A page argument: a `PageRef`, or a display index (`PageRef | number`).
pub use epdf_plugin_stage::PageTarget;

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

/// `SearchLimit`: how much one `query()` call may do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<f64>,
}

/// `SearchRequest`: one bounded batch of work, the query's own members
/// first (`{...query, snippets, cursor | from}`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    #[serde(flatten)]
    pub query: SearchQuery,
    /// Include the text around each match; also needs `doc.text.copy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippets: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Viewport-first ordering: start at this page and wrap around.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<SearchLimit>,
}

/// `SearchSnippet`: `match` between `before` and `after`, whitespace
/// flattened to spaces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SearchSnippet {
    pub before: String,
    #[serde(rename = "match")]
    pub matched: String,
    pub after: String,
}

/// `TextSegment` (engine-core `PdfTextSegment<PageCoordinates>`): one merged
/// visual line of a match in page space. `quad` is the geometric authority,
/// its corners named in the line's own frame; `rect` its bounds; `advance`
/// the reading direction along the baseline, `1` or `-1`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    pub quad: Quad,
    pub rect: Rect,
    pub advance: f64,
}

/// `SearchMatch`: one hit as the engine reports it, a range of the page's
/// characters with its segments already in page space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub page: PageRef,
    pub start: f64,
    pub count: f64,
    pub segments: Vec<TextSegment>,
    /// `null` reads as absent, as the TypeScript's truthiness test does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<SearchSnippet>,
}

/// `SearchSlice`: the result of one bounded batch. `next_cursor: None` (the
/// engine's `null`) means the search is done. `pages_searched` is
/// cumulative across the cursor chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSlice {
    pub matches: Vec<SearchMatch>,
    pub next_cursor: Option<String>,
    pub pages_searched: f64,
    pub page_count: f64,
}

// ── the plugin's types ────────────────────────────────────────────────────────

/// `SearchHit`: one match. `page` is the durable identity, `page_index` its
/// display index at the time of the match; `start` and `count` are a range
/// of the page's characters, so a hit goes straight to a selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page: PageRef,
    pub page_index: usize,
    pub start: f64,
    pub count: f64,
    pub segments: Vec<TextSegment>,
    /// The union of the segments, the reveal target; absent when the match
    /// has none.
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

/// `SearchProgress`: pages searched so far, of how many.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SearchProgress {
    pub pages_searched: f64,
    pub page_count: f64,
}

// ── settings ──────────────────────────────────────────────────────────────────

/// `SearchReveal`: how a match arrives in view when you move to it,
/// forwarded to the Stage's positioned reveal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchReveal {
    /// Where the match lands; `None` is a stored `null`, a positioned reveal
    /// with neither axis set.
    pub anchor: Option<RevealAnchor>,
    pub behavior: ScrollBehaviorKind,
}

/// `SearchBlendMode`: a CSS `mix-blend-mode` keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// The highlight of every match and of the active one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHighlight {
    pub color: String,
    pub active_color: String,
    pub blend_mode: SearchBlendMode,
}

/// `SearchSettings`: what `search_plugin(config)` registers over
/// [`search_defaults`], and `update_settings` changes for every document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchSettings {
    /// Where a match lands when `next_hit`, `previous_hit` or `go_to_hit`
    /// moves to it.
    pub reveal: SearchReveal,
    pub highlight: SearchHighlight,
}

/// `SEARCH_DEFAULTS`: the find-bar feel, the hit 35% of the way down.
pub fn search_defaults() -> SearchSettings {
    SearchSettings {
        reveal: SearchReveal {
            anchor: Some(RevealAnchor {
                x: None,
                y: Some(epdf_plugin_stage::RevealAnchorValue::Fraction(0.35)),
            }),
            behavior: ScrollBehaviorKind::Smooth,
        },
        highlight: SearchHighlight {
            color: "#ffd500".into(),
            active_color: "#ff9632".into(),
            blend_mode: SearchBlendMode::Multiply,
        },
    }
}

/// A `DeepPartial<SearchSettings['reveal']>`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchRevealConfig {
    /// `Some(None)` is a `null` anchor: a positioned reveal with no axis.
    pub anchor: Option<Option<RevealAnchor>>,
    pub behavior: Option<ScrollBehaviorKind>,
}

/// A `DeepPartial<SearchSettings['highlight']>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchHighlightConfig {
    pub color: Option<String>,
    pub active_color: Option<String>,
    pub blend_mode: Option<SearchBlendMode>,
}

/// `SearchConfig`: any of the settings, merged over the defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchConfig {
    pub reveal: Option<SearchRevealConfig>,
    pub highlight: Option<SearchHighlightConfig>,
}

impl SearchConfig {
    /// The partial as the settings store merges it.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(reveal) = &self.reveal {
            let mut r = Map::new();
            if let Some(anchor) = &reveal.anchor {
                r.insert("anchor".into(), serde_json::to_value(anchor).unwrap_or(Value::Null));
            }
            if let Some(behavior) = reveal.behavior {
                r.insert("behavior".into(), json!(behavior));
            }
            out.insert("reveal".into(), Value::Object(r));
        }
        if let Some(highlight) = &self.highlight {
            let mut h = Map::new();
            if let Some(color) = &highlight.color {
                h.insert("color".into(), json!(color));
            }
            if let Some(color) = &highlight.active_color {
                h.insert("activeColor".into(), json!(color));
            }
            if let Some(mode) = highlight.blend_mode {
                h.insert("blendMode".into(), json!(mode));
            }
            out.insert("highlight".into(), Value::Object(h));
        }
        Value::Object(out)
    }

    /// Read a `DeepPartial<SearchSettings>`: members of the wrong type are
    /// ignored.
    pub fn from_json(value: &Value) -> Self {
        let reveal = value.get("reveal").filter(|r| r.is_object()).map(|r| SearchRevealConfig {
            anchor: match r.get("anchor") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(v) => serde_json::from_value(v.clone()).ok().map(Some),
            },
            behavior: r.get("behavior").and_then(|b| serde_json::from_value(b.clone()).ok()),
        });
        let text = |h: &Value, key: &str| h.get(key).and_then(Value::as_str).map(str::to_string);
        let highlight =
            value.get("highlight").filter(|h| h.is_object()).map(|h| SearchHighlightConfig {
                color: text(h, "color"),
                active_color: text(h, "activeColor"),
                blend_mode: h.get("blendMode").and_then(|b| serde_json::from_value(b.clone()).ok()),
            });
        SearchConfig { reveal, highlight }
    }
}

/// `SettingsChanged<SearchSettings>`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchSettingsChanged {
    pub settings: SearchSettings,
    pub changed: Vec<String>,
}

/// `SearchRevealOptions` (`Partial<SearchReveal>`): the `reveal` setting for
/// one call, merged as `{...settings.reveal, ...options}`.
///
/// Each member is two options because a spread copies a key that is
/// present with the value `undefined`: the outer `None` is an absent key
/// (the setting shows through), `Some(None)` a key present as `undefined`
/// (or, for `behavior`, `null`, which the Stage reads the same way), which
/// overrides the setting away. A `null` anchor is present and positions the
/// reveal with no axis set, which is `Some(Some(RevealAnchor::default()))`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchRevealOptions {
    pub anchor: Option<Option<RevealAnchor>>,
    pub behavior: Option<Option<ScrollBehaviorKind>>,
}

// ── verbs ─────────────────────────────────────────────────────────────────────

/// `OperationOptions`.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `SearchOptions`.
#[derive(Clone, Default)]
pub struct SearchOptions {
    /// Where to start: a page's ref or its index. Defaults to the Stage's
    /// current page (viewport-first) when a Stage is installed.
    pub from: Option<PageTarget>,
    pub signal: Option<AbortSignal>,
}

/// `SearchFindAllOptions`.
#[derive(Clone, Default)]
pub struct SearchFindAllOptions {
    /// Pin whether hits carry snippets. `None` asks for them and falls back
    /// to none when snippets are denied.
    pub snippets: Option<bool>,
    /// Rejects `operation-cancelled` when it aborts.
    pub signal: Option<AbortSignal>,
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

/// `SearchResult`: it finished, a newer search replaced it, or it was
/// cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub status: SearchResultStatus,
    pub hit_count: usize,
}

/// `goToHit`'s target: the hit itself (such as the one a layer's click hands
/// you), or its index, which wraps.
#[derive(Clone, Debug, PartialEq)]
pub enum HitTarget {
    Hit(Rc<SearchHit>),
    Index(isize),
}

impl From<isize> for HitTarget {
    fn from(index: isize) -> Self {
        HitTarget::Index(index)
    }
}

impl From<Rc<SearchHit>> for HitTarget {
    fn from(hit: Rc<SearchHit>) -> Self {
        HitTarget::Hit(hit)
    }
}

impl From<SearchHit> for HitTarget {
    fn from(hit: SearchHit) -> Self {
        HitTarget::Hit(Rc::new(hit))
    }
}

// ── events ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SearchStartedEvent {
    pub query: SearchQuery,
}

/// The progress changed: pages searched, of how many, and the matches found
/// so far, the values `get_progress()` and `get_hit_count()` return from then
/// on.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchProgressChangedEvent {
    pub pages_searched: f64,
    pub page_count: f64,
    pub hit_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCompletedEvent {
    pub hit_count: usize,
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
pub struct SearchCancelledEvent {
    pub reason: SearchCancelReason,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchFailedEvent {
    pub error: PluginErrorInfo,
}

/// `index` is `-1` when no hit is active.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchActiveHitChangedEvent {
    pub index: isize,
    pub hit: Option<Rc<SearchHit>>,
}

/// `SearchClearedEvent`: an empty object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct SearchClearedEvent {}
