//! G5's slice protocol: every verdict reached before a page is read.
//!
//! Replays `vectors/search/slice-protocol.json`, which the TypeScript's own
//! `validateSearchQuery`, `searchQueryKey` and `decodeSearchCursor` produced.
//! Each case replays its recorded `request`, the flattened `SearchRequest`
//! the TypeScript built and checked, not its `case`, which is written in the
//! spelling before upstream's a10ed2659.
//!
//! Three things are pinned, and all three are refusals a caller can hit
//! without the document being involved at all:
//!
//! - the flag combination, because `regex` plus `matchDiacritics` asks for
//!   matching on a plane that does not exist;
//! - the cursor key, because it is compared as a STRING and a cursor minted
//!   by one implementation is replayed against the other;
//! - the cursor's own checks, because a resume that quietly succeeds against
//!   the wrong query or a mutated document is the worst answer available.
//!
//! A cursor refusal's message is compared word for word: this build words
//! them as `searchCursor.ts` does.

use epdf_engine::features::search::cursor::{decode_search_cursor, search_query_key};
use epdf_engine::features::search::reader::{SearchLimit, SearchRequest};
use epdf_engine::features::search::validate_search_query;
use epdf_engine::identity::PageObjectNumber;

const VECTOR: &str = "tooling/engine-conformance/vectors/search/slice-protocol.json";

/// The case's recorded request, read as the host reads a `search.query`'s
/// `request`.
fn request_of(case: &serde_json::Value) -> SearchRequest {
    serde_json::from_value(case["request"].clone())
        .unwrap_or_else(|e| panic!("{}: the recorded request does not decode: {e}", case["name"]))
}

#[test]
fn every_slice_protocol_case_agrees_with_the_oracle() {
    let raw = std::fs::read_to_string(test_support::workspace_root().join(VECTOR))
        .unwrap_or_else(|e| panic!("read {VECTOR}: {e}"));
    let file: serde_json::Value = serde_json::from_str(&raw).expect("the vector parses");
    assert_eq!(file["schemaVersion"], 1, "the vector's envelope changed shape");
    let cases = file["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "the vector is empty");

    let mut checked_cursors = 0;
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let request = request_of(case);
        let query = &request.query;

        // The flag combination, which is the only thing a literal query can
        // get wrong and the first thing a regex one is checked for.
        let verdict = validate_search_query(query);
        assert_eq!(
            verdict.is_ok(),
            case["valid"].as_bool().expect("valid"),
            "{name}: the two implementations disagree about whether the query is valid",
        );
        if let Err(issue) = verdict {
            assert_eq!(
                issue.as_wire(),
                case["issue"].as_str().expect("issue"),
                "{name}: refused for a different reason",
            );
        }

        // The key, character for character. A synonym here is a cursor that
        // cannot cross between implementations. `snippets` comes off the
        // decoded request, so what is compared is the spelling this build
        // puts in a cursor for the request it was sent.
        assert_eq!(
            search_query_key(query, request.snippets),
            case["key"].as_str().expect("key"),
            "{name}: the cursor key differs",
        );

        let Some(expected) = case.get("cursor") else { continue };
        checked_cursors += 1;
        let cursor = request.cursor.as_deref().expect("cursor");
        let seq = case["seq"].as_u64().unwrap_or(0);
        let key = case["key"].as_str().expect("key");

        match decode_search_cursor(cursor, request.from, seq, key) {
            Ok(_) => assert!(
                expected["ok"].as_bool().expect("ok"),
                "{name}: this build resumed a cursor the oracle refuses",
            ),
            Err(error) => {
                assert!(
                    !expected["ok"].as_bool().expect("ok"),
                    "{name}: this build refused a cursor the oracle resumes",
                );
                assert_eq!(
                    error.message,
                    expected["message"].as_str().expect("message"),
                    "{name}: refused for a different reason",
                );
            }
        }
    }
    assert!(checked_cursors >= 7, "the cursor half of the vector stopped being exercised");
}

#[test]
fn a_request_decodes_from_the_shape_the_wire_sends() {
    // The query's text and flags sit at the top level beside how to run it
    // (`SearchScanRequest`). `limit` arrives as an object because a caller
    // passes its policy through whole, and reading `pages` off the top level
    // instead would leave every limit at its default: a slice four times
    // longer than asked for, and no error.
    let request: SearchRequest = serde_json::from_value(serde_json::json!({
        "text": "the",
        "matchCase": true,
        "snippets": true,
        "from": { "kind": "objectNumber", "objectNumber": 4 },
        "skip": 9,
        "limit": { "pages": 3, "matches": 7 },
    }))
    .expect("the request decodes");

    assert_eq!(request.query.text, "the");
    assert!(request.query.match_case, "a flag was dropped");
    assert!(!request.query.regex, "an absent flag did not default to false");
    assert!(request.snippets);
    assert_eq!(request.from.map(PageObjectNumber::get), Some(4));
    assert_eq!(request.skip, Some(9.0));
    assert_eq!(request.limit, Some(SearchLimit { pages: Some(3.0), matches: Some(7.0) }));
}

#[test]
fn the_smallest_request_the_wire_can_send_decodes() {
    // Everything but the text is optional, and a request that sets none of
    // it is the common one: a search box with no cursor, no viewport and no
    // policy. Snippets are off unless asked for, as upstream's are.
    let request: SearchRequest =
        serde_json::from_value(serde_json::json!({ "text": "the" })).expect("the request decodes");
    assert!(!request.snippets);
    assert!(request.from.is_none());
    assert!(request.cursor.is_none());
    assert!(request.skip.is_none());
    assert!(request.limit.is_none());
}
