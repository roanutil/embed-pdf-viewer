//! G5's slice protocol: every verdict reached before a page is read.
//!
//! Replays `vectors/search/slice-protocol.json`, which the TypeScript's own
//! `validateSearchQuery`, `searchQueryKey` and `decodeSearchCursor` produced.
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
//! The refusal MESSAGES are not compared. Each implementation words its own,
//! and the property worth holding is that the same requests are refused for
//! the same distinguishable reasons, which the categories below assert.

use epdf_engine::features::search::cursor::{decode_search_cursor, search_query_key};
use epdf_engine::features::search::matcher::SearchQuery;
use epdf_engine::features::search::reader::{SearchMode, SearchRequest};
use epdf_engine::features::search::validate_search_query;
use epdf_engine::identity::PageObjectNumber;

const VECTOR: &str = "tooling/engine-conformance/vectors/search/slice-protocol.json";

/// Why a cursor was refused, told apart without comparing prose.
///
/// Derived from each side's own message, because the two implementations
/// word them differently and the vector carries the TypeScript's. Four
/// categories rather than one, since a caller restarts the search for three
/// of them and fixes its own arguments for the fourth.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Malformed,
    DifferentQuery,
    Stale,
    StartPageConflict,
}

fn classify(message: &str) -> Refusal {
    if message.contains("malformed") {
        Refusal::Malformed
    } else if message.contains("different query") {
        Refusal::DifferentQuery
    } else if message.contains("startPage") {
        Refusal::StartPageConflict
    } else if message.contains("changed since") {
        Refusal::Stale
    } else {
        panic!("a refusal this test cannot classify: {message}");
    }
}

fn query_of(case: &serde_json::Value) -> SearchQuery {
    let q = &case["request"]["query"];
    SearchQuery {
        text: q["text"].as_str().expect("text").to_owned(),
        regex: q["regex"].as_bool().unwrap_or(false),
        match_case: q["matchCase"].as_bool().unwrap_or(false),
        match_diacritics: q["matchDiacritics"].as_bool().unwrap_or(false),
        whole_word: q["wholeWord"].as_bool().unwrap_or(false),
        ignore_whitespace: q["ignoreWhitespace"].as_bool().unwrap_or(false),
    }
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
        let query = query_of(case);

        // The flag combination, which is the only thing a literal query can
        // get wrong and the first thing a regex one is checked for.
        let verdict = validate_search_query(&query);
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
        // cannot cross between implementations.
        //
        // The mode goes through the ENUM rather than through the vector's
        // string, so what is compared is the spelling this build puts in a
        // cursor. Passing the vector's own string through would compare it
        // with itself and pass whatever the enum said.
        let mode: SearchMode = match case["request"]["mode"].as_str() {
            None => SearchMode::default(),
            Some(raw) => serde_json::from_value(serde_json::Value::String(raw.to_owned()))
                .unwrap_or_else(|e| panic!("{name}: this build has no '{raw}' mode: {e}")),
        };
        assert_eq!(
            search_query_key(&query, mode.as_key()),
            case["key"].as_str().expect("key"),
            "{name}: the cursor key differs",
        );

        let Some(expected) = case.get("cursor") else { continue };
        checked_cursors += 1;
        let cursor = case["request"]["cursor"].as_str().expect("cursor");
        let start_page = case["request"]["startPage"]
            .as_u64()
            .map(|n| PageObjectNumber::new(n as u32).expect("a page object number"));
        let seq = case["seq"].as_u64().unwrap_or(0);
        let key = case["key"].as_str().expect("key");

        match decode_search_cursor(cursor, start_page, seq, key) {
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
                    classify(&error.message),
                    classify(expected["message"].as_str().expect("message")),
                    "{name}: refused for a different reason",
                );
            }
        }
    }
    assert!(checked_cursors >= 7, "the cursor half of the vector stopped being exercised");
}

#[test]
fn a_request_decodes_from_the_shape_the_wire_sends() {
    // The nesting is the contract, not a detail. `budget` arrives as an
    // object because a caller passes its policy through whole, and reading
    // `maxPages` off the top level instead would leave every budget at its
    // default: a slice four times longer than asked for, and no error.
    let request: SearchRequest = serde_json::from_value(serde_json::json!({
        "query": { "text": "the", "matchCase": true },
        "mode": "rects",
        "startPage": 4,
        "skip": 9,
        "budget": { "maxPages": 3, "maxMatches": 7 },
    }))
    .expect("the request decodes");

    assert!(request.query.match_case, "a flag was dropped");
    assert!(!request.query.regex, "an absent flag did not default to false");
    assert_eq!(request.mode, SearchMode::Rects);
    assert_eq!(request.start_page.map(PageObjectNumber::get), Some(4));
    assert_eq!(request.skip, Some(9));
    let budget = request.budget.expect("the budget survived");
    assert_eq!(budget.max_pages, Some(3));
    assert_eq!(budget.max_matches, Some(7));
}

#[test]
fn the_smallest_request_the_wire_can_send_decodes() {
    // Everything but the query is optional, and a request that sets none of
    // it is the common one: a search box with no cursor, no viewport and no
    // policy. A required field here would make that request an error.
    let request: SearchRequest =
        serde_json::from_value(serde_json::json!({ "query": { "text": "the" } }))
            .expect("the request decodes");
    assert_eq!(request.mode, SearchMode::Full);
    assert!(request.start_page.is_none());
    assert!(request.cursor.is_none());
    assert!(request.skip.is_none());
    assert!(request.budget.is_none());
}
