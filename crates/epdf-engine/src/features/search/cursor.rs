//! Resuming a search where the last slice stopped.
//!
//! G5. A transcription of
//! `services/src/features/search/internal/searchCursor.ts`.
//!
//! A cursor pins three things and refuses to resume if any of them moved:
//!
//! - the query, and whether it asks for snippets, because a cursor is not
//!   transferable between queries;
//! - the document's mutation sequence, because resuming over mutated content
//!   could serve stale pages or, worse, text a redaction just removed;
//! - the scan origin, because a position means nothing in a different page
//!   order.
//!
//! Opaque to callers by contract and JSON underneath, because a local cursor
//! never crosses a trust boundary. The server mints its own authenticated
//! format for the wire.

use serde::{Deserialize, Serialize};

use super::matcher::SearchQuery;
use crate::error::{EngineError, EngineErrorCode};

/// What a slice hands back so the next one can continue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCursorState {
    /// Format version. A cursor from another version is refused rather than
    /// reinterpreted.
    pub v: u32,
    /// The session's mutation sequence when the cursor was minted.
    pub seq: u64,
    /// The query's identity, snippets included ([`search_query_key`]).
    pub key: String,
    /// The `from` page the scan order was rotated to, by object number. Zero
    /// is natural order.
    pub start: u32,
    /// Pages of the scan order already consumed.
    pub scanned: u32,
}

/// Stable identity for a query and whether it asks for snippets
/// (`searchQueryKey`).
///
/// Hand-ordered rather than derived, because a derived encoding would change
/// when a field is added and silently invalidate every outstanding cursor.
/// The spellings are the TypeScript's, because a cursor minted by one
/// implementation is compared as a string by the other.
pub fn search_query_key(query: &SearchQuery, snippets: bool) -> String {
    serde_json::json!([
        if query.regex { "r" } else { "l" },
        query.text,
        u8::from(query.match_case),
        u8::from(query.match_diacritics),
        u8::from(query.whole_word),
        u8::from(query.ignore_whitespace),
        if snippets { "snippets" } else { "matches" },
    ])
    .to_string()
}

pub fn encode_search_cursor(state: &SearchCursorState) -> String {
    serde_json::to_string(state).expect("a cursor is plain data")
}

/// Decode a cursor, refusing one that does not belong to this scan
/// (`decodeSearchCursor`). `from` is the request's own start page, if it
/// names one.
///
/// Every refusal is `InvalidArg` rather than a distinct code, because a caller
/// can do exactly one thing about any of them: start again without a cursor.
pub fn decode_search_cursor(
    cursor: &str,
    from: Option<crate::identity::PageObjectNumber>,
    expected_seq: u64,
    expected_key: &str,
) -> Result<SearchCursorState, EngineError> {
    let malformed =
        || EngineError::with_message(EngineErrorCode::InvalidArg, "malformed search cursor");

    let state: SearchCursorState = serde_json::from_str(cursor).map_err(|_| malformed())?;
    if state.v != 1 {
        return Err(malformed());
    }
    if state.key != expected_key {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "search cursor belongs to a different query \u{2014} restart the search",
        ));
    }
    if state.seq != expected_seq {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "document changed since this search cursor was issued \u{2014} restart the search",
        )
        .with_details(serde_json::json!({ "cursorSeq": state.seq, "currentSeq": expected_seq })));
    }
    // The cursor owns the position. A `from` beside it that disagrees is a
    // caller bug, and silently letting the cursor win would hand back matches
    // from a part of the document the caller did not ask about.
    if let Some(page) = from
        && page.get() != state.start
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "`from` conflicts with the cursor \u{2014} omit it when resuming",
        ));
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> SearchQuery {
        SearchQuery { text: text.into(), ..Default::default() }
    }

    #[test]
    fn a_search_without_snippets_is_keyed_as_matches() {
        // No slice-protocol case mints a cursor without snippets, so the
        // spelling is pinned here: `searchQueryKey(query, false)`.
        assert_eq!(search_query_key(&query("the"), false), r#"["l","the",0,0,0,0,"matches"]"#);
        assert_eq!(search_query_key(&query("the"), true), r#"["l","the",0,0,0,0,"snippets"]"#);
    }

    #[test]
    fn a_cursor_minted_without_snippets_does_not_resume_with_them() {
        let matches = search_query_key(&query("the"), false);
        let cursor = encode_search_cursor(&SearchCursorState {
            v: 1,
            seq: 3,
            key: matches.clone(),
            start: 0,
            scanned: 2,
        });
        assert_eq!(decode_search_cursor(&cursor, None, 3, &matches).unwrap().scanned, 2);
        let error = decode_search_cursor(&cursor, None, 3, &search_query_key(&query("the"), true))
            .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidArg);
        assert!(error.message.contains("different query"), "{}", error.message);
    }

    #[test]
    fn a_stale_cursor_says_which_sequences_differ() {
        let key = search_query_key(&query("the"), true);
        let cursor = encode_search_cursor(&SearchCursorState {
            v: 1,
            seq: 6,
            key: key.clone(),
            start: 0,
            scanned: 5,
        });
        let error = decode_search_cursor(&cursor, None, 7, &key).unwrap_err();
        assert_eq!(error.details, Some(serde_json::json!({ "cursorSeq": 6, "currentSeq": 7 })));
    }
}
