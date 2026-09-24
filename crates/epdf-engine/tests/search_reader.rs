//! G5: searching a document, slice by slice.
//!
//! The pieces below this are pinned to the TypeScript by vectors: the fold,
//! the literal and regex matchers, the snippet, the charmap conversion and
//! the layout. What this covers is the pipeline that composes them, where the
//! failures are about budgets, ordering and resumption rather than about text.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::search::matcher::SearchQuery;
use epdf_engine::features::search::reader::{SearchMode, SearchRequest, search};
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;

fn report() -> DocumentSession {
    let mut session = DocumentSession::new();
    session
        .open_with(
            test_support::fixture(test_support::REPORT),
            None,
            SessionAuthority::unrestricted(),
        )
        .expect("open");
    session
}

fn literal(text: &str) -> SearchQuery {
    SearchQuery { text: text.to_string(), ..SearchQuery::default() }
}

/// A needle rare enough that a scan finishes inside the default match budget.
///
/// "the" appears often enough in report.pdf to exhaust the 256-match default
/// after four pages, which is correct behaviour and useless for testing the
/// scan mechanics. "figure" appears 27 times across the document.
const RARE: &str = "figure";

fn request(query: SearchQuery) -> SearchRequest {
    SearchRequest { query, ..SearchRequest::default() }
}

fn pages(session: &mut DocumentSession) -> Vec<PageObjectNumber> {
    session.all_records().expect("records").iter().map(|r| r.page_object_number).collect()
}

#[test]
fn a_query_finds_matches_with_segments_and_a_snippet() {
    let mut session = report();
    let slice =
        search(&mut session, &request(literal("the")), &CancelToken::never()).expect("search");

    assert!(!slice.matches.is_empty(), "report.pdf contains 'the'");
    assert_eq!(slice.total_pages, 14);
    for m in &slice.matches {
        assert!(m.char_count > 0, "a match covers no characters");
        assert!(!m.segments.is_empty(), "a match on real text has no segments to highlight");
        let snippet = m.snippet.as_ref().expect("full mode carries a snippet");
        assert!(!snippet.text.is_empty());
    }
}

#[test]
fn positions_mode_omits_the_snippet() {
    // For a caller that already has the page text and only wants to know
    // where. The segments are the same either way.
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest { mode: SearchMode::Positions, ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("search");

    assert!(!slice.matches.is_empty());
    for m in &slice.matches {
        assert!(m.snippet.is_none(), "positions mode produced a snippet");
        assert!(!m.segments.is_empty());
    }
}

#[test]
fn an_empty_needle_scans_nothing_rather_than_matching_everything() {
    // A whitespace-only needle folds to a single space, which would match
    // every collapsed run on every page. The scan is not started at all.
    let mut session = report();
    for text in ["", "   ", "\t\n"] {
        let slice =
            search(&mut session, &request(literal(text)), &CancelToken::never()).expect("search");
        assert!(slice.matches.is_empty(), "{text:?} matched something");
        assert_eq!(slice.scanned_pages, 0, "{text:?} burned a scan");
        assert!(slice.next_cursor.is_none());
    }
}

#[test]
fn a_page_budget_stops_the_slice_and_yields_a_cursor() {
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest { max_pages: Some(3), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("search");

    assert_eq!(slice.scanned_pages, 3);
    assert_eq!(slice.total_pages, 14);
    assert!(slice.next_cursor.is_some(), "an unfinished scan produced no cursor");
}

#[test]
fn a_cursor_resumes_where_the_last_slice_stopped() {
    let mut session = report();
    let first = search(
        &mut session,
        &SearchRequest { max_pages: Some(5), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("first");
    assert_eq!(first.scanned_pages, 5);

    let second = search(
        &mut session,
        &SearchRequest {
            cursor: first.next_cursor.clone(),
            max_pages: Some(5),
            ..request(literal(RARE))
        },
        &CancelToken::never(),
    )
    .expect("second");
    assert_eq!(second.scanned_pages, 10, "the second slice restarted rather than resumed");

    // And the two slices cover different pages.
    let first_pages: std::collections::BTreeSet<u32> =
        first.matches.iter().map(|m| m.page_object_number.get()).collect();
    let second_pages: std::collections::BTreeSet<u32> =
        second.matches.iter().map(|m| m.page_object_number.get()).collect();
    assert!(first_pages.is_disjoint(&second_pages), "the slices reported matches on the same page",);
}

#[test]
fn a_finished_scan_yields_no_cursor() {
    let mut session = report();
    let slice =
        search(&mut session, &request(literal(RARE)), &CancelToken::never()).expect("search");
    assert_eq!(slice.scanned_pages, slice.total_pages);
    assert!(slice.next_cursor.is_none());
}

#[test]
fn a_cursor_from_a_different_query_is_refused() {
    // A cursor is not transferable. Resuming one query's position inside
    // another would report matches for a needle nobody searched for.
    let mut session = report();
    let first = search(
        &mut session,
        &SearchRequest { max_pages: Some(2), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("first");

    let refused = search(
        &mut session,
        &SearchRequest { cursor: first.next_cursor.clone(), ..request(literal("different")) },
        &CancelToken::never(),
    )
    .expect_err("a cursor from another query");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("different query"), "{}", refused.message);
}

#[test]
fn a_cursor_from_before_a_mutation_is_refused() {
    // Resuming over mutated content could serve stale pages or, worse, text a
    // redaction just removed.
    let mut session = report();
    let first = search(
        &mut session,
        &SearchRequest { max_pages: Some(2), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("first");

    session.note_mutation();

    let refused = search(
        &mut session,
        &SearchRequest { cursor: first.next_cursor.clone(), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect_err("a stale cursor");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("changed"), "{}", refused.message);
}

#[test]
fn a_malformed_cursor_is_refused_rather_than_ignored() {
    let mut session = report();
    for cursor in ["", "{}", "not json", r#"{"v":2,"seq":0,"key":"x","start":0,"scanned":0}"#] {
        let refused = search(
            &mut session,
            &SearchRequest { cursor: Some(cursor.to_string()), ..request(literal("the")) },
            &CancelToken::never(),
        )
        .expect_err("malformed");
        assert_eq!(refused.code, EngineErrorCode::InvalidArg, "{cursor:?}");
    }
}

#[test]
fn a_start_page_rotates_the_scan_order_and_wraps() {
    // Viewport-first: a viewer searches from the page being read, and the
    // scan wraps rather than stopping at the end.
    let mut session = report();
    let all = pages(&mut session);
    let from_middle = search(
        &mut session,
        &SearchRequest { start_page: Some(all[7]), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("search");

    assert_eq!(from_middle.scanned_pages, 14, "the scan did not wrap");
    // The first match comes from page 7 or later, not from page 0.
    let first_page = from_middle.matches.first().expect("matches").page_object_number;
    let first_index = all.iter().position(|p| *p == first_page).expect("known page");
    assert!(first_index >= 7, "the scan began at the start rather than at the requested page");
}

#[test]
fn an_unknown_start_page_is_not_found() {
    let mut session = report();
    let missing = PageObjectNumber::new(99_999).expect("number");
    let refused = search(
        &mut session,
        &SearchRequest { start_page: Some(missing), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect_err("unknown page");
    assert_eq!(refused.code, EngineErrorCode::NotFound);
}

#[test]
fn a_match_budget_stops_the_slice() {
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest { max_matches: Some(1), ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("search");

    // The budget is checked between pages, so a page's matches are never
    // split: one page's worth may exceed the budget and that is deliberate.
    assert!(slice.scanned_pages >= 1);
    assert!(slice.next_cursor.is_some(), "a budgeted slice produced no cursor");
}

#[test]
fn a_regex_query_finds_what_a_literal_one_would_not() {
    let mut session = report();
    let slice = search(
        &mut session,
        &request(SearchQuery {
            text: r"\bthe\b".to_string(),
            regex: true,
            ..SearchQuery::default()
        }),
        &CancelToken::never(),
    )
    .expect("search");
    assert!(!slice.matches.is_empty(), "a word-boundary pattern found nothing");
}

#[test]
fn an_invalid_regex_is_refused() {
    let mut session = report();
    let refused = search(
        &mut session,
        &request(SearchQuery {
            text: "(unclosed".to_string(),
            regex: true,
            ..SearchQuery::default()
        }),
        &CancelToken::never(),
    )
    .expect_err("invalid pattern");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
}

#[test]
fn a_cancelled_search_reports_aborted() {
    let mut session = report();
    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        search(&mut session, &request(literal("the")), &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}

#[test]
fn searching_a_closed_session_is_refused() {
    let mut session = report();
    session.close();
    assert_eq!(
        search(&mut session, &request(literal("the")), &CancelToken::never())
            .expect_err("closed")
            .code,
        EngineErrorCode::DocNotOpen,
    );
}
