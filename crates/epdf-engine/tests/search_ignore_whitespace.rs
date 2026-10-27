//! `ignoreWhitespace`, the literal-only flag `f082bf3e4` added to the
//! TypeScript (`core/test/search/{fold,literal,regex}.test.ts`, and against
//! real page text `main/test/search-ignore-whitespace.test.ts`). The cases
//! are the TypeScript's, transcribed.

use epdf_engine::features::search::fold::{FoldOptions, fold_text};
use epdf_engine::features::search::literal::{LiteralQuery, match_literal};
use epdf_engine::features::search::matcher::SearchQuery;
use epdf_engine::features::search::reader::{SearchMatch, SearchRequest, search};
use epdf_engine::features::search::{SearchQueryIssue, validate_search_query};

fn find(text: &str, query: LiteralQuery) -> Vec<(u32, u32)> {
    let haystack = fold_text(text, query.fold_options());
    match_literal(&haystack, &query).into_iter().map(|r| (r.start, r.count)).collect()
}

fn needle(text: &str) -> LiteralQuery {
    LiteralQuery { text: text.into(), ..Default::default() }
}

fn relaxed(text: &str) -> LiteralQuery {
    LiteralQuery { ignore_whitespace: true, ..needle(text) }
}

#[test]
fn dropping_whitespace_keeps_the_map_on_the_letters() {
    let f = fold_text("i n\tv\noice", FoldOptions { drop_whitespace: true, ..Default::default() });
    assert_eq!(f.folded, "invoice");
    assert_eq!(f.map, vec![0, 2, 4, 6, 7, 8, 9]);
}

#[test]
fn dropping_whitespace_also_drops_whitespace_born_from_decomposition() {
    let f = fold_text("a \u{a8} b", FoldOptions { drop_whitespace: true, ..Default::default() });
    assert_eq!(f.folded, "ab");
}

#[test]
fn by_default_a_needle_must_carry_a_space_wherever_the_page_does() {
    assert_eq!(find("Ref: i n v o i c e 42", needle("invoice")), vec![]);
    assert_eq!(find("Invoice 42", needle("i n v o i c e")), vec![]);
}

#[test]
fn letter_spaced_text_is_found_and_the_hit_spans_the_gaps() {
    assert_eq!(find("Ref: i n v o i c e 42", relaxed("invoice")), vec![(5, 13)]);
}

#[test]
fn whitespace_drops_on_the_needle_side_too() {
    assert_eq!(find("Invoice 42", relaxed("i n v o i c e")), vec![(0, 7)]);
    assert_eq!(find("totalamount", relaxed("total amount")), vec![(0, 11)]);
}

#[test]
fn a_match_crosses_a_line_wrap() {
    assert_eq!(find("in\nvoice", relaxed("invoice")), vec![(0, 8)]);
}

#[test]
fn it_composes_with_match_case() {
    let lower = LiteralQuery { match_case: true, ..relaxed("invoice") };
    assert_eq!(find("I n v o i c e", lower), vec![]);
    let upper = LiteralQuery { match_case: true, ..relaxed("Invoice") };
    assert_eq!(find("I n v o i c e", upper), vec![(0, 13)]);
}

#[test]
fn whole_word_reads_its_boundaries_off_the_original_text() {
    let spaced = LiteralQuery { whole_word: true, ..relaxed("invoice") };
    assert_eq!(find("Ref: i n v o i c e 42", spaced.clone()), vec![(5, 13)]);
    assert_eq!(find("the invoices", spaced), vec![]);
}

#[test]
fn a_whitespace_only_needle_finds_nothing() {
    assert_eq!(find("anything", relaxed(" \n ")), vec![]);
}

#[test]
fn a_regex_with_ignore_whitespace_is_refused() {
    let query = SearchQuery {
        text: "a".into(),
        regex: true,
        ignore_whitespace: true,
        ..Default::default()
    };
    let issue = validate_search_query(&query).unwrap_err();
    assert_eq!(issue, SearchQueryIssue::IgnoreWhitespaceWithRegex);
    assert_eq!(issue.as_wire(), "ignore-whitespace-with-regex");
    let literal = SearchQuery { text: "i n v o i c e".into(), ignore_whitespace: true, ..query };
    assert!(validate_search_query(&SearchQuery { regex: false, ..literal }).is_ok());
}

// ── against real page text (`main/test/search-ignore-whitespace.test.ts`) ──
//
// The fixture carries the letter-spaced heading "i n v o i c e" on two pages,
// a spaced "total amount" and a glued "totalamount", "in" / "voice" split
// across a line break, and "the invoices".

const LETTER_SPACED: &str = "packages/engine/main/test/fixtures/letter_spaced_text.pdf";

/// Every match of a query over the whole fixture, slice after slice, with
/// snippets.
fn search_all(query: SearchQuery) -> Vec<SearchMatch> {
    use epdf_engine::cancel::CancelToken;
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::document::DocumentSession;

    let mut session = DocumentSession::new();
    session
        .open_with(test_support::fixture(LETTER_SPACED), None, SessionAuthority::unrestricted())
        .expect("open");
    let mut matches = Vec::new();
    let mut cursor = None;
    loop {
        let request =
            SearchRequest { query: query.clone(), snippets: true, cursor, ..Default::default() };
        let slice = search(&mut session, &request, &CancelToken::never()).expect("search");
        matches.extend(slice.matches);
        match slice.next_cursor {
            None => return matches,
            next => cursor = next,
        }
    }
}

fn spaced(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), ignore_whitespace: true, ..Default::default() }
}

/// The matched text exactly as the page carries it (from the snippet).
fn hit_text(m: &SearchMatch) -> String {
    m.snippet.as_ref().expect("a snippet").matched.clone()
}

/// Case-folded, whitespace-free: the identity the flag matches on.
fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

fn key(m: &SearchMatch) -> (u32, u32, u32) {
    (m.page.get(), m.start, m.count)
}

#[test]
fn the_default_fold_does_not_see_letter_spaced_text() {
    let hits = search_all(SearchQuery { text: "invoice".into(), ..Default::default() });
    assert!(!hits.is_empty());
    for m in &hits {
        assert!(!hit_text(m).chars().any(char::is_whitespace), "{:?}", hit_text(m));
    }
}

#[test]
fn letter_spaced_hits_are_found_on_both_pages_and_every_default_hit_is_kept() {
    let plain = search_all(SearchQuery { text: "invoice".into(), ..Default::default() });
    let relaxed = search_all(spaced("invoice"));

    let relaxed_keys: std::collections::HashSet<_> = relaxed.iter().map(key).collect();
    for m in &plain {
        assert!(relaxed_keys.contains(&key(m)), "the flag lost a default hit {:?}", key(m));
    }
    assert!(relaxed.len() > plain.len());

    let texts: Vec<String> = relaxed.iter().map(hit_text).collect();
    // The letter-spaced occurrences, one per page: the hit spans the gaps.
    assert_eq!(texts.iter().filter(|t| *t == "i n v o i c e").count(), 2);
    let pages: std::collections::HashSet<_> = relaxed.iter().map(|m| m.page.get()).collect();
    assert_eq!(pages.len(), 2);
    // The word split across a line break.
    assert!(
        texts.iter().any(|t| {
            t.starts_with("in")
                && t.ends_with("voice")
                && t.len() > 7
                && t[2..t.len() - 5].chars().all(char::is_whitespace)
        }),
        "{texts:?}"
    );
    // Every hit is the same word once whitespace and case are dropped.
    for t in &texts {
        let word = squash(t);
        assert!(word == "invoice" || word == "invoices", "{t:?}");
    }
    // Every hit carries drawable geometry, including the ones spanning gaps.
    for m in &relaxed {
        assert!(!m.segments.is_empty());
    }
}

#[test]
fn a_spaced_needle_finds_the_glued_word() {
    let plain: Vec<String> =
        search_all(SearchQuery { text: "total amount".into(), ..Default::default() })
            .iter()
            .map(hit_text)
            .collect();
    assert!(!plain.contains(&"totalamount".to_string()));

    let relaxed: Vec<String> = search_all(spaced("total amount")).iter().map(hit_text).collect();
    assert!(relaxed.contains(&"totalamount".to_string()), "{relaxed:?}");
    assert!(relaxed.contains(&"total amount".to_string()), "{relaxed:?}");
    for t in &relaxed {
        assert_eq!(squash(t), "totalamount");
    }
}

#[test]
fn whole_word_boundaries_come_from_the_original_text() {
    let relaxed = search_all(spaced("invoice"));
    let whole = search_all(SearchQuery { whole_word: true, ..spaced("invoice") });

    // "the invoices" is the only hit glued to a word character in the
    // original text: the snippet continues with the trailing "s".
    let glued = |m: &&SearchMatch| {
        m.snippet.as_ref().and_then(|s| s.after.chars().next()).is_some_and(char::is_alphanumeric)
    };
    assert_eq!(relaxed.iter().filter(glued).count(), 1);
    assert_eq!(whole.iter().filter(glued).count(), 0);
    assert_eq!(whole.len(), relaxed.len() - 1);
    // "i n v o i c e 42": the digits are glued only on the folded plane.
    assert_eq!(whole.iter().map(hit_text).filter(|t| t == "i n v o i c e").count(), 2);
}

#[test]
fn match_case_composes_with_ignore_whitespace() {
    let lower = search_all(SearchQuery { match_case: true, ..spaced("invoice") });
    assert!(!lower.is_empty());
    for m in &lower {
        assert_eq!(hit_text(m), hit_text(m).to_lowercase());
    }

    let capitalised: Vec<String> =
        search_all(SearchQuery { match_case: true, ..spaced("Invoice") })
            .iter()
            .map(hit_text)
            .collect();
    assert!(capitalised.contains(&"Invoice".to_string()), "{capitalised:?}");
    assert!(capitalised.contains(&"I n v o i c e".to_string()), "{capitalised:?}");
    for t in &capitalised {
        assert!(t.starts_with('I'), "{t:?}");
    }
}
