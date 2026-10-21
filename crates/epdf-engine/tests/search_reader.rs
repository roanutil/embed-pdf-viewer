//! G5: searching a document, slice by slice.
//!
//! The pieces below this are pinned to the TypeScript by vectors: the fold,
//! the literal and regex matchers, the snippet, the charmap conversion and
//! the layout. What this covers is the pipeline that composes them, where the
//! failures are about budgets, ordering and resumption rather than about text.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::search::matcher::SearchQuery;
use epdf_engine::features::search::reader::{SearchLimit, SearchRequest, search};
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
    let slice = search(
        &mut session,
        &SearchRequest { snippets: true, ..request(literal("the")) },
        &CancelToken::never(),
    )
    .expect("search");

    assert!(!slice.matches.is_empty(), "report.pdf contains 'the'");
    assert_eq!(slice.page_count, 14);
    for m in &slice.matches {
        assert!(m.count > 0, "a match covers no characters");
        assert!(!m.segments.is_empty(), "a match on real text has no segments to highlight");
        let snippet = m.snippet.as_ref().expect("a request for snippets carries one");
        assert_eq!(snippet.matched.to_lowercase(), "the", "the match part is not the match");
    }
}

#[test]
fn a_search_without_snippets_withholds_the_text_and_keeps_the_geometry() {
    // A permission distinction rather than a verbosity one. A snippet IS
    // extracted text, so leaving it out is the search for a document whose
    // owner denies extraction: the reader can still FIND, and nothing
    // readable crosses the boundary. The geometry is not text and stays.
    // Snippets are off unless asked for.
    let mut session = report();
    let slice =
        search(&mut session, &request(literal("the")), &CancelToken::never()).expect("search");

    assert!(!slice.matches.is_empty());
    for m in &slice.matches {
        assert!(m.snippet.is_none(), "a search without snippets leaked one");
        assert!(!m.segments.is_empty(), "a search without snippets withheld the geometry too");
    }
}

#[test]
fn a_fractional_limit_rounds_down_as_upstreams_clamp_does() {
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(2.9), ..Default::default() }),
            ..request(literal("the"))
        },
        &CancelToken::never(),
    )
    .expect("search");
    assert_eq!(slice.pages_searched, 2);

    let slice = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(0.0), ..Default::default() }),
            ..request(literal("the"))
        },
        &CancelToken::never(),
    )
    .expect("search");
    assert_eq!(slice.pages_searched, 1, "a limit below one still searches a page");
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
        assert_eq!(slice.pages_searched, 0, "{text:?} burned a scan");
        assert!(slice.next_cursor.is_none());
    }
}

#[test]
fn a_page_budget_stops_the_slice_and_yields_a_cursor() {
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(3.0), ..Default::default() }),
            ..request(literal("the"))
        },
        &CancelToken::never(),
    )
    .expect("search");

    assert_eq!(slice.pages_searched, 3);
    assert_eq!(slice.page_count, 14);
    assert!(slice.next_cursor.is_some(), "an unfinished scan produced no cursor");
}

#[test]
fn a_cursor_resumes_where_the_last_slice_stopped() {
    let mut session = report();
    let first = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(5.0), ..Default::default() }),
            ..request(literal(RARE))
        },
        &CancelToken::never(),
    )
    .expect("first");
    assert_eq!(first.pages_searched, 5);

    let second = search(
        &mut session,
        &SearchRequest {
            cursor: first.next_cursor.clone(),
            limit: Some(SearchLimit { pages: Some(5.0), ..Default::default() }),
            ..request(literal(RARE))
        },
        &CancelToken::never(),
    )
    .expect("second");
    assert_eq!(second.pages_searched, 10, "the second slice restarted rather than resumed");

    // And the two slices cover different pages.
    let first_pages: std::collections::BTreeSet<u32> =
        first.matches.iter().map(|m| m.page.get()).collect();
    let second_pages: std::collections::BTreeSet<u32> =
        second.matches.iter().map(|m| m.page.get()).collect();
    assert!(first_pages.is_disjoint(&second_pages), "the slices reported matches on the same page",);
}

#[test]
fn a_finished_scan_yields_no_cursor() {
    let mut session = report();
    let slice =
        search(&mut session, &request(literal(RARE)), &CancelToken::never()).expect("search");
    assert_eq!(slice.pages_searched, slice.page_count);
    assert!(slice.next_cursor.is_none());
}

#[test]
fn a_cursor_from_a_different_query_is_refused() {
    // A cursor is not transferable. Resuming one query's position inside
    // another would report matches for a needle nobody searched for.
    let mut session = report();
    let first = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(2.0), ..Default::default() }),
            ..request(literal("the"))
        },
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
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(2.0), ..Default::default() }),
            ..request(literal("the"))
        },
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
        &SearchRequest { from: Some(all[7]), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("search");

    assert_eq!(from_middle.pages_searched, 14, "the scan did not wrap");
    // The first match comes from page 7 or later, not from page 0.
    let first_page = from_middle.matches.first().expect("matches").page;
    let first_index = all.iter().position(|p| *p == first_page).expect("known page");
    assert!(first_index >= 7, "the scan began at the start rather than at the requested page");
}

#[test]
fn an_unknown_start_page_is_not_found() {
    let mut session = report();
    let missing = PageObjectNumber::new(99_999).expect("number");
    let refused = search(
        &mut session,
        &SearchRequest { from: Some(missing), ..request(literal("the")) },
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
        &SearchRequest {
            limit: Some(SearchLimit { matches: Some(1.0), ..Default::default() }),
            ..request(literal("the"))
        },
        &CancelToken::never(),
    )
    .expect("search");

    // The budget is checked between pages, so a page's matches are never
    // split: one page's worth may exceed the budget and that is deliberate.
    assert!(slice.pages_searched >= 1);
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

#[test]
fn skip_resumes_by_position_without_a_cursor() {
    // The trusted resume. A caller that pins content versions externally,
    // which the cloud wire does by carrying the search epoch in its URL, has
    // no cursor to present and no sequence to check against; what it has is
    // how far it already got. Without this it would re-scan from page one
    // and re-serve every match it has already shown.
    let mut session = report();
    let whole = search(&mut session, &request(literal(RARE)), &CancelToken::never())
        .expect("the whole scan");

    let skipped = search(
        &mut session,
        &SearchRequest { skip: Some(7.0), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("the resumed scan");

    assert_eq!(skipped.pages_searched, whole.pages_searched, "the scan did not reach the end");

    let order = pages(&mut session);
    let skipped_pages: Vec<_> = order[..7].to_vec();
    let expected: Vec<_> =
        whole.matches.iter().filter(|m| !skipped_pages.contains(&m.page)).map(|m| m.page).collect();
    let actual: Vec<_> = skipped.matches.iter().map(|m| m.page).collect();
    assert_eq!(actual, expected, "a skipped scan is not the tail of the whole one");
    assert!(!expected.is_empty(), "the skip consumed the whole document");
}

#[test]
fn a_skip_past_the_end_scans_nothing_rather_than_wrapping() {
    // Clamped, not wrapped. A caller whose external pin is stale asks for a
    // position past the document and gets an empty slice, which reads as
    // "nothing left"; wrapping would silently re-serve page one as though it
    // were new.
    let mut session = report();
    let slice = search(
        &mut session,
        &SearchRequest { skip: Some(9_999.0), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("search");

    assert!(slice.matches.is_empty(), "a skip past the end still found matches");
    assert_eq!(slice.pages_searched, slice.page_count, "the skip was not clamped to the document");
    assert!(slice.next_cursor.is_none(), "a finished scan offered a cursor");
}

#[test]
fn a_start_page_and_a_skip_apply_together() {
    // Not either/or. `from` chooses where the scan ORDER begins and
    // `skip` how far into that order the caller already is, so a viewport-
    // first scan that resumes needs both. Treating them as alternatives
    // would make the second slice of a viewport-first search start over at
    // page one.
    let mut session = report();
    let order = pages(&mut session);
    let fifth = order[4];

    let from_fifth = search(
        &mut session,
        &SearchRequest { from: Some(fifth), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("the viewport-first scan");

    let resumed = search(
        &mut session,
        &SearchRequest { from: Some(fifth), skip: Some(3.0), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("the resumed viewport-first scan");

    // The rotated order is pages 5..14 then 1..4. Skipping three of it drops
    // the matches on the first three of those and keeps the rest, which is
    // only true if the rotation happened as well as the skip.
    let dropped: Vec<_> = order[4..7].to_vec();
    let expected: Vec<_> =
        from_fifth.matches.iter().filter(|m| !dropped.contains(&m.page)).map(|m| m.page).collect();
    let actual: Vec<_> = resumed.matches.iter().map(|m| m.page).collect();
    assert_eq!(actual, expected, "the start page and the skip did not both apply");
    assert!(!expected.is_empty(), "the fixture has nothing left after the skip");
}

#[test]
fn a_query_the_dialect_refuses_is_refused_before_any_page_is_read() {
    // Up front, not on the first page. A document with no pages would
    // otherwise accept a broken pattern and answer with an empty slice,
    // which a caller reads as "no matches" rather than "your query is
    // wrong". The refusal also names the issue, because a search box shows
    // it.
    let mut session = report();
    let error = search(
        &mut session,
        &request(SearchQuery { text: "a(?=b)".into(), regex: true, ..SearchQuery::default() }),
        &CancelToken::never(),
    )
    .expect_err("a lookaround is not in the dialect");
    assert_eq!(error.code, EngineErrorCode::InvalidArg);
    assert!(error.message.contains("lookaround"), "the issue is not named: {}", error.message);
}

#[test]
fn diacritic_sensitive_matching_is_refused_for_a_regex() {
    // The one rejected flag combination, and the one most likely to be
    // waved through: both flags are individually valid and the matcher
    // simply ignores the second. Ignoring it means a caller who asked for
    // "café" not to match "cafe" gets matches that do, with no indication
    // that the flag did nothing.
    let mut session = report();
    let error = search(
        &mut session,
        &request(SearchQuery {
            text: "caf.".into(),
            regex: true,
            match_diacritics: true,
            ..SearchQuery::default()
        }),
        &CancelToken::never(),
    )
    .expect_err("regex plus diacritics is refused");
    assert_eq!(error.code, EngineErrorCode::InvalidArg);
    assert!(
        error.message.contains("diacritics-with-regex"),
        "the issue is not named: {}",
        error.message,
    );
}

#[test]
fn a_cursor_will_not_resume_against_a_conflicting_start_page() {
    // The cursor owns the position. A `from` beside it that disagrees
    // is a caller bug, and letting the cursor quietly win would hand back
    // matches from a part of the document the caller did not ask about.
    let mut session = report();
    let order = pages(&mut session);
    let first = search(
        &mut session,
        &SearchRequest {
            limit: Some(SearchLimit { pages: Some(2.0), ..Default::default() }),
            ..request(literal(RARE))
        },
        &CancelToken::never(),
    )
    .expect("first");
    let cursor = first.next_cursor.expect("the scan is not finished");

    let error = search(
        &mut session,
        &SearchRequest {
            cursor: Some(cursor.clone()),
            from: Some(order[5]),
            ..request(literal(RARE))
        },
        &CancelToken::never(),
    )
    .expect_err("a conflicting start page is refused");
    assert_eq!(error.code, EngineErrorCode::InvalidArg);
    assert!(
        error.message.contains("`from` conflicts"),
        "the conflict is not named: {}",
        error.message
    );

    // And the same cursor without it still resumes, so the refusal is about
    // the conflict rather than about the cursor.
    search(
        &mut session,
        &SearchRequest { cursor: Some(cursor), ..request(literal(RARE)) },
        &CancelToken::never(),
    )
    .expect("the cursor alone resumes");
}
