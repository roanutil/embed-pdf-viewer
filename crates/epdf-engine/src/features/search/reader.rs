//! The whole search pipeline for one budgeted slice.
//!
//! G5. Replaces `services/src/features/search/SearchReader.ts`. Operation:
//! `search.query`.
//!
//! Three stages, in order:
//!
//! - extract, taking each page's text through the reader;
//! - match, through the pure matcher, which is the same code on every engine
//!   and is where cross-platform parity comes from rather than from testing;
//! - anchor, reading geometry only for pages that actually matched, and
//!   turning each hit into visual-line segments through the shared layout.
//!
//! Scope gating is the caller's job. A session's
//! [`crate::session::authority::SessionAuthority`] answers whether
//! `doc.text.search` is granted; this assumes an authorized request.

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::geometry::read_page_geometry;
use crate::features::text::charmap::char_range_for_text_offsets;
use crate::features::text::layout::{
    PdfTextSegment, build_page_text_layout, text_segments_for_range,
};
use crate::features::text::reader::read_page_text;
use crate::identity::PageObjectNumber;
use crate::session::document::DocumentSession;

use super::cursor::{
    SearchCursorState, decode_search_cursor, encode_search_cursor, search_query_key,
};
use super::fold::{FoldOptions, fold_text};
use super::matcher::{SearchQuery, match_page_text};
use super::snippet::{SEARCH_SNIPPET_CONTEXT, SearchSnippet, build_snippet};

/// Per-slice budget defaults and ceilings.
///
/// The ceiling is the worker's self-defence: one query is one synchronous
/// stretch of native work, and no request may hold the worker longer than a
/// few hundred pages whatever budget it asks for. A caller wanting more
/// issues more slices, which the cursor makes cheap.
const DEFAULT_MAX_PAGES: u32 = 64;
const CEILING_MAX_PAGES: u32 = 256;
const DEFAULT_MAX_MATCHES: u32 = 256;
const CEILING_MAX_MATCHES: u32 = 1024;

/// How much of each hit leaves the engine.
///
/// A permission distinction rather than a verbosity one. `Rects` is the mode
/// for a document whose owner denies text extraction: the reader can FIND,
/// and nothing readable crosses the boundary. `Full` adds a snippet, which
/// IS extracted text and needs the scope to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchMode {
    /// Segments and a snippet.
    #[default]
    Full,
    /// Segments only.
    Rects,
}

impl SearchMode {
    /// The spelling that goes into the cursor key.
    ///
    /// The same string the TypeScript puts there, because a cursor minted by
    /// one implementation and replayed against the other has to compare
    /// equal. Spelling this `positions` would make every such cursor a
    /// "different query" refusal that no caller could diagnose.
    ///
    /// Public so the conformance runner can compare it against the
    /// TypeScript's. A test that spelled the mode itself would pass whatever
    /// this returned, which is the one thing worth checking.
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Rects => "rects",
        }
    }
}

/// Per-slice spending caps.
///
/// Nested rather than flattened onto the request, matching the wire: a
/// caller passes the caps through untouched from wherever its policy lives,
/// and a policy object is easier to pass whole than to spread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSliceBudget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_matches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<u32>,
}

/// What a caller asks for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: SearchQuery,
    #[serde(default)]
    pub mode: SearchMode,
    /// Begin at this page and wrap, so a viewer searches from what the reader
    /// is looking at rather than from page one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_page: Option<PageObjectNumber>,
    /// Resume a previous slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Trusted absolute resume position: pages of the scan order already
    /// consumed.
    ///
    /// For a caller that pins content versions EXTERNALLY, which the cloud
    /// wire does by carrying the search content epoch in the URL. No
    /// sequence check applies, which is what makes it trusted and why
    /// everyone else should use `cursor` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<SearchSliceBudget>,
}

/// One hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub page_object_number: PageObjectNumber,
    /// CHARACTER space, which is what geometry and selection use.
    pub char_start: u32,
    pub char_count: u32,
    pub segments: Vec<PdfTextSegment>,
    /// Present in [`SearchMode::Full`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<SearchSnippet>,
}

/// One budgeted slice of a scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSlice {
    pub matches: Vec<SearchMatch>,
    /// `None` when the scan is finished.
    pub next_cursor: Option<String>,
    pub scanned_pages: u32,
    pub total_pages: u32,
}

fn clamp(requested: Option<u32>, fallback: u32, ceiling: u32) -> u32 {
    requested.map_or(fallback, |value| value.clamp(1, ceiling))
}

/// Run one slice.
pub fn search(
    session: &mut DocumentSession,
    request: &SearchRequest,
    cancel: &CancelToken,
) -> Result<SearchSlice, EngineError> {
    cancel.check()?;

    // Up front, and once, rather than on the first page that gets scanned.
    // A document with no pages would otherwise accept a pattern the dialect
    // refuses and answer with an empty slice, which reads as "no matches"
    // and is the one answer a broken query must never give.
    if let Err(issue) = super::validate_search_query(&request.query) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("invalid search query ({}): {}", issue.as_wire(), issue.message()),
        ));
    }

    let records: Vec<PageObjectNumber> =
        session.all_records()?.iter().map(|r| r.page_object_number).collect();
    let total_pages = records.len() as u32;

    // An empty or whitespace-only needle would match every collapsed space on
    // every page, so the scan is not started at all rather than budgeted.
    if !request.query.regex
        && fold_text(&request.query.text, FoldOptions::default()).folded.trim().is_empty()
    {
        return Ok(SearchSlice {
            matches: Vec::new(),
            next_cursor: None,
            scanned_pages: 0,
            total_pages,
        });
    }

    let seq = session.mutation_sequence();
    let key = search_query_key(&request.query, request.mode.as_key());

    let (mut start, mut scanned) = (0u32, 0u32);
    if let Some(cursor) = &request.cursor {
        let state = decode_search_cursor(cursor, request.start_page, seq, &key)?;
        start = state.start;
        scanned = state.scanned;
    } else {
        // Both, not either. `startPage` chooses WHERE the scan order begins
        // and `skip` how far into that order the caller already is, so a
        // resumed viewport-first scan needs the two together.
        if let Some(page) = request.start_page {
            // Resolved through the registry, so an unknown page is NotFound
            // here rather than a scan that silently begins at zero.
            session.record_by_object_number(page)?;
            start = page.get();
        }
        if let Some(skip) = request.skip {
            scanned = skip.min(total_pages);
        }
    }

    // Viewport first: rotate the display order to begin at `start`, wrapping.
    let order: Vec<PageObjectNumber> = if start == 0 {
        records
    } else {
        let at = records.iter().position(|r| r.get() == start).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no page with object number {start}"),
            )
        })?;
        records[at..].iter().chain(&records[..at]).copied().collect()
    };

    let budget = request.budget.unwrap_or_default();
    let max_pages = clamp(budget.max_pages, DEFAULT_MAX_PAGES, CEILING_MAX_PAGES);
    let max_matches = clamp(budget.max_matches, DEFAULT_MAX_MATCHES, CEILING_MAX_MATCHES);

    let mut matches: Vec<SearchMatch> = Vec::new();
    let mut pages_this_slice = 0u32;

    // Budget checks sit at page granularity: a page's matches are never split
    // across slices, so a cursor only ever points between pages and a caller
    // never has to stitch one page's hits together.
    while (scanned as usize) < order.len()
        && pages_this_slice < max_pages
        && (matches.len() as u32) < max_matches
    {
        cancel.check()?;
        let page_number = order[scanned as usize];

        let page = session.acquire_page(page_number)?;
        let snapshot = read_page_text(page.as_ref(), cancel)?;
        let ranges = match_page_text(&snapshot.text, &request.query).map_err(|issue| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("invalid search query: {issue:?}"),
            )
        })?;

        if !ranges.is_empty() {
            let geometry = read_page_geometry(page.as_ref(), cancel)?;
            // One canonical layout per page, shared by every match on it.
            let layout = build_page_text_layout(&geometry);
            for range in ranges {
                // Match ranges are TEXT space; the hit and the layout speak
                // CHARACTER space. Converted exactly once, here, through the
                // biased helper that keeps zero-width characters adjacent to
                // the match outside it on both sides. Snippets stay in text
                // space, because their offsets are internal to the snippet.
                let chars =
                    char_range_for_text_offsets(&snapshot, range.start, range.start + range.length);
                matches.push(SearchMatch {
                    page_object_number: page_number,
                    char_start: chars.start,
                    char_count: chars.end - chars.start,
                    segments: text_segments_for_range(
                        &layout,
                        chars.start,
                        chars.end - chars.start,
                    ),
                    snippet: (request.mode == SearchMode::Full)
                        .then(|| build_snippet(&snapshot.text, range, SEARCH_SNIPPET_CONTEXT)),
                });
            }
        }

        scanned += 1;
        pages_this_slice += 1;
    }

    Ok(SearchSlice {
        next_cursor: ((scanned as usize) < order.len())
            .then(|| encode_search_cursor(&SearchCursorState { v: 1, seq, key, start, scanned })),
        matches,
        scanned_pages: scanned,
        total_pages,
    })
}
