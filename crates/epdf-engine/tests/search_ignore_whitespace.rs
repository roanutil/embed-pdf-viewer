//! `ignoreWhitespace`, the literal-only flag `f082bf3e4` added to the
//! TypeScript (`core/test/search/{fold,literal,regex}.test.ts`). The cases
//! are the TypeScript's, transcribed.

use epdf_engine::features::search::fold::{FoldOptions, fold_text};
use epdf_engine::features::search::literal::{LiteralQuery, match_literal};
use epdf_engine::features::search::matcher::SearchQuery;
use epdf_engine::features::search::{SearchQueryIssue, validate_search_query};

fn find(text: &str, query: LiteralQuery) -> Vec<(u32, u32)> {
    let haystack = fold_text(text, query.fold_options());
    match_literal(&haystack, &query).into_iter().map(|r| (r.start, r.length)).collect()
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
