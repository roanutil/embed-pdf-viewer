//! Resuming a search where the last slice stopped.
//!
//! G5. A transcription of
//! `services/src/features/search/internal/searchCursor.ts`.
//!
//! A cursor pins three things and refuses to resume if any of them moved:
//!
//! - the query and mode, because a cursor is not transferable between
//!   queries;
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
    /// Query and mode identity.
    pub key: String,
    /// The page object number the scan order was rotated to. Zero is natural
    /// order.
    pub start: u32,
    /// Pages of the scan order already consumed.
    pub scanned: u32,
}

/// Stable identity for a query and mode.
///
/// Hand-ordered rather than derived, because a derived encoding would change
/// when a field is added and silently invalidate every outstanding cursor.
pub fn search_query_key(query: &SearchQuery, mode: &str) -> String {
    serde_json::json!([
        if query.regex { "r" } else { "l" },
        query.text,
        u8::from(query.match_case),
        u8::from(query.match_diacritics),
        u8::from(query.whole_word),
        mode,
    ])
    .to_string()
}

pub fn encode_search_cursor(state: &SearchCursorState) -> String {
    serde_json::to_string(state).expect("a cursor is plain data")
}

/// Decode a cursor, refusing one that does not belong to this scan.
///
/// Every refusal is `InvalidArg` rather than a distinct code, because a caller
/// can do exactly one thing about any of them: start again without a cursor.
pub fn decode_search_cursor(
    cursor: &str,
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
            "this cursor belongs to a different query",
        ));
    }
    if state.seq != expected_seq {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the document changed since this cursor was issued",
        ));
    }
    Ok(state)
}
