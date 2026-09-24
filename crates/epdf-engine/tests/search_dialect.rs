//! G5's entry gate: the Rust search engine reproduces `regex.ts` case by case.
//!
//! The port order gives G5 an entry gate rather than an exit gate, and this is
//! it. Nothing in `core/src/search/` gets ported until a Rust engine agrees
//! with JavaScript `RegExp` on the dialect the shipping matcher actually
//! executes: what it accepts, what it refuses, and where the matches land.
//!
//! The vectors under `tooling/engine-conformance/vectors/search/` are what
//! `validateSearchRegex`, `matchRegex` and `foldText` produce. Regenerate them,
//! deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run search
//!
//! Divergences this gate has accepted are listed in [`KNOWN_DIVERGENCES`],
//! each with a reason. A case that moves in or out of that list fails the
//! test, so "we already knew about that one" stays a decision somebody made
//! rather than a silence.

use std::collections::BTreeMap;

use epdf_engine::features::search::{SearchMatchRange, match_regex, validate_search_regex};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct DialectFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<DialectCase>,
}

#[derive(Debug, Deserialize)]
struct DialectCase {
    name: String,
    covers: String,
    pattern: String,
    #[serde(default, rename = "matchCase")]
    match_case: bool,
    #[serde(default, rename = "wholeWord")]
    whole_word: bool,
    texts: Vec<String>,
    /// Whether `validateSearchRegex` accepted the pattern.
    valid: bool,
    /// The `SearchRegexIssue` it reported. Absent when it accepted.
    #[serde(default)]
    issue: Option<String>,
    /// One entry per text, in order. Absent when the pattern was refused.
    #[serde(default)]
    results: Option<Vec<Vec<Range>>>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
struct Range {
    start: usize,
    length: usize,
}

impl From<SearchMatchRange> for Range {
    fn from(value: SearchMatchRange) -> Self {
        Self { start: value.start, length: value.length }
    }
}

/// Cases where the Rust engine and JavaScript `RegExp` genuinely differ, with
/// the reason each one is recorded rather than fixed. The gate fails both when
/// a listed case starts agreeing and when an unlisted one stops, so this is a
/// decision somebody made rather than a silence.
///
/// Both entries are the price of the normalized haystack in
/// `features/search/mod.rs`. An anchored pattern is matched against a copy of
/// the text with `\r`, U+2028 and U+2029 rewritten to `\n`, because the
/// `regex` crate's `(?m)` breaks a line on `\n` alone. The copy cannot tell
/// the terminators apart, so an anchored pattern that ALSO distinguishes them
/// sees the normalized character.
///
/// This is a deliberate trade, not an oversight. Before it, `^` and `$` did
/// not anchor on a carriage return at all, and PDFium emits `\r` in extracted
/// text for real documents: every anchored query over such a file was quietly
/// wrong. Now the wrongness is confined to patterns that anchor and also
/// mention a terminator, which is a combination nobody types into a document
/// search box by accident. `unanchored-literal-carriage-return` and
/// `unanchored-negated-newline-class` are in the corpus to hold that
/// confinement: without an anchor there is no copy and no approximation.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[
    (
        "anchored-pattern-matching-a-literal-carriage-return",
        "the normalized haystack has \\n where the original had \\r, so the literal finds nothing",
    ),
    (
        "anchored-pattern-with-a-negated-newline-class",
        "[^\\n] excludes the normalized \\r, so the run stops where JavaScript continues",
    ),
];

fn load(name: &str) -> DialectFile {
    let path =
        test_support::workspace_root().join("tooling/engine-conformance/vectors/search").join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: DialectFile =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(file.schema_version, 1, "{} was written by a different generator", path.display());
    assert!(!file.cases.is_empty(), "{} has no cases", path.display());
    file
}

/// Every dialect case behaves in Rust the way it behaves in JavaScript.
#[test]
fn the_rust_engine_reproduces_the_javascript_dialect() {
    let file = load("regex-dialect.json");
    assert_eq!(file.kind, "search.regex");
    let known: BTreeMap<&str, &str> = KNOWN_DIVERGENCES.iter().copied().collect();
    let mut failures: Vec<String> = Vec::new();
    let mut unused_exemptions: Vec<&str> = known.keys().copied().collect();

    for case in &file.cases {
        let mut problems: Vec<String> = Vec::new();
        let verdict = validate_search_regex(&case.pattern);

        match (case.valid, verdict) {
            (true, Err(issue)) => problems
                .push(format!("JavaScript accepts it, Rust refuses it as `{}`", issue.as_wire())),
            (false, Ok(())) => problems.push("JavaScript refuses it, Rust accepts it".to_string()),
            (false, Err(issue)) => {
                let expected = case.issue.as_deref().expect("a refused case carries its issue");
                if issue.as_wire() != expected {
                    // The issue is part of the contract: a UI greys out a
                    // different control for `lookaround` than for `syntax`.
                    problems.push(format!(
                        "both refuse it, but JavaScript says `{expected}` and Rust says `{}`",
                        issue.as_wire()
                    ));
                }
            }
            (true, Ok(())) => {
                let expected = case.results.as_ref().expect("an accepted case carries its results");
                assert_eq!(
                    expected.len(),
                    case.texts.len(),
                    "{}: results/texts mismatch",
                    case.name
                );
                for (text, want) in case.texts.iter().zip(expected) {
                    match match_regex(text, &case.pattern, case.match_case, case.whole_word) {
                        Err(issue) => problems.push(format!(
                            "validates but will not run: `{}` on {text:?}",
                            issue.as_wire()
                        )),
                        Ok(found) => {
                            let got: Vec<Range> = found.into_iter().map(Range::from).collect();
                            if &got != want {
                                problems.push(format!(
                                    "on {text:?}: JavaScript {want:?}, Rust {got:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }

        match (problems.is_empty(), known.get(case.name.as_str())) {
            (true, Some(reason)) => failures.push(format!(
                "{}: listed as a known divergence ({reason}) but now agrees. Remove the entry.",
                case.name
            )),
            (false, Some(_)) => unused_exemptions.retain(|name| *name != case.name),
            (false, None) => failures.push(format!(
                "{} ({}):\n      {}",
                case.name,
                case.covers,
                problems.join("\n      ")
            )),
            (true, None) => {}
        }
    }

    for orphan in unused_exemptions {
        if known.contains_key(orphan) && !file.cases.iter().any(|case| case.name == orphan) {
            failures.push(format!("{orphan}: exempted, but there is no such case any more."));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} dialect case(s) diverge:\n  {}",
        failures.len(),
        file.cases.len(),
        failures.join("\n  "),
    );
}

/// The dialect corpus still covers the categories G5's entry gate names.
///
/// Without this, deleting the astral or the case-folding cases would leave the
/// gate green while proving only that ASCII works.
#[test]
fn the_dialect_corpus_covers_what_the_entry_gate_asks_for() {
    let file = load("regex-dialect.json");
    let refused = file.cases.iter().filter(|case| !case.valid).count();
    assert!(refused >= 8, "only {refused} refused patterns");

    for issue in ["empty", "too-long", "backreference", "lookaround", "syntax"] {
        assert!(
            file.cases.iter().any(|case| case.issue.as_deref() == Some(issue)),
            "no case reports `{issue}`"
        );
    }
    for required in [
        "kelvin-sign-folds-to-k",
        "long-s-folds-to-s",
        "astral-literal",
        "dot-spans-a-surrogate-pair",
        "star-matches-nothing-and-does-not-loop",
        "whole-word-is-unicode-aware",
        "caret-under-m",
        "nested-quantifier",
    ] {
        assert!(file.cases.iter().any(|case| case.name == required), "no `{required}` case");
    }
}

/// An adversarial pattern stays linear, which is the property the dialect's
/// no-backreference, no-lookaround rule exists to preserve.
///
/// A deliberately loose bound. This is not a benchmark; it is the difference
/// between microseconds and heat death, and a backtracking engine would not
/// finish the second of these inside a test run.
#[test]
fn catastrophic_backtracking_shapes_stay_linear() {
    for pattern in ["(?:a+)+b", "(?:a|aa)+b", "(?:a*)*b"] {
        for length in [24usize, 32, 40] {
            let subject = "a".repeat(length);
            let started = std::time::Instant::now();
            let found = match_regex(&subject, pattern, true, false).expect("runs");
            assert!(found.is_empty(), "{pattern} unexpectedly matched");
            assert!(
                started.elapsed() < std::time::Duration::from_secs(1),
                "{pattern} took {:?} on {length} characters",
                started.elapsed()
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct FoldFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<FoldCase>,
}

#[derive(Debug, Deserialize)]
struct FoldCase {
    name: String,
    text: String,
    #[serde(default, rename = "keepCase")]
    keep_case: bool,
    #[serde(default, rename = "keepMarks")]
    keep_marks: bool,
    folded: String,
    /// Folded code unit to the UTF-16 index of the code point it came from.
    map: Vec<usize>,
    /// `toOriginalRange` over every folded range.
    ranges: Vec<FoldRange>,
}

#[derive(Debug, Deserialize)]
struct FoldRange {
    start: usize,
    length: usize,
    original: OriginalRangeVector,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct OriginalRangeVector {
    start: u32,
    length: u32,
}

fn load_folds() -> FoldFile {
    let path =
        test_support::workspace_root().join("tooling/engine-conformance/vectors/search/fold.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: FoldFile =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(file.schema_version, 1, "{} was written by a different generator", path.display());
    assert_eq!(file.kind, "search.fold");
    assert!(!file.cases.is_empty(), "{} has no cases", path.display());
    file
}

/// The fold vectors satisfy the invariants `toOriginalRange` depends on.
///
/// Kept alongside the parity test below rather than replaced by it. Parity
/// says the two implementations agree; this says what they agree ON, so a
/// corpus regenerated from a broken TypeScript fold fails here rather than
/// being adopted by Rust as the new truth. The properties:
///
/// - one map entry per folded UTF-16 code unit, so a folded offset always has
///   an original to translate to;
/// - the map never goes backwards, because a match range translates by taking
///   its first and last entries;
/// - every entry addresses a real UTF-16 index of the original, so
///   `codePointLengthAt` reads a character rather than half of one.
#[test]
fn the_fold_vectors_keep_the_properties_the_anchor_stage_rests_on() {
    let file = load_folds();
    let mut failures: Vec<String> = Vec::new();

    for case in &file.cases {
        let folded_units = case.folded.chars().map(char::len_utf16).sum::<usize>();
        if folded_units != case.map.len() {
            failures.push(format!(
                "{}: {folded_units} folded code units but {} map entries",
                case.name,
                case.map.len()
            ));
        }
        let original_units = case.text.chars().map(char::len_utf16).sum::<usize>();
        // UTF-16 index of every character start in the original, so a map
        // entry pointing into the middle of a surrogate pair is caught.
        let mut starts = std::collections::BTreeSet::new();
        let mut at = 0usize;
        for ch in case.text.chars() {
            starts.insert(at);
            at += ch.len_utf16();
        }

        let mut previous = 0usize;
        for (i, &entry) in case.map.iter().enumerate() {
            if entry >= original_units {
                failures.push(format!(
                    "{}: map[{i}] is {entry}, past the original's {original_units} units",
                    case.name
                ));
                break;
            }
            if !starts.contains(&entry) {
                failures.push(format!(
                    "{}: map[{i}] is {entry}, which is not a character start",
                    case.name
                ));
                break;
            }
            if entry < previous {
                failures.push(format!(
                    "{}: map[{i}] goes backwards, {previous} then {entry}",
                    case.name
                ));
                break;
            }
            previous = entry;
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} fold case(s) break an invariant:\n  {}",
        failures.len(),
        file.cases.len(),
        failures.join("\n  "),
    );
}

// --- G5: the Rust fold against the TypeScript one.

#[test]
fn the_fold_agrees_with_typescript() {
    use epdf_engine::features::search::fold::{FoldOptions, fold_text};

    // NFKD, mark stripping and the case round trip, on the corpus the entry
    // gate names: the Kelvin sign, the long s, the sharp s, final sigma,
    // ligatures, combining marks and astral text.
    let file = load_folds();
    let mut failures: Vec<String> = Vec::new();

    for case in &file.cases {
        let options = FoldOptions { keep_case: case.keep_case, keep_marks: case.keep_marks };
        let actual = fold_text(&case.text, options);
        if actual.folded != case.folded {
            failures.push(format!(
                "{}: folded {:?}, TypeScript said {:?}",
                case.name, actual.folded, case.folded,
            ));
            continue;
        }
        let actual_map: Vec<usize> = actual.map.iter().map(|u| *u as usize).collect();
        if actual_map != case.map {
            failures.push(format!(
                "{}: map {:?}, TypeScript said {:?}",
                case.name, actual_map, case.map,
            ));
        }
    }

    assert!(failures.is_empty(), "fold divergence:\n  {}", failures.join("\n  "));
}

#[test]
fn folded_ranges_translate_back_the_same_way() {
    use epdf_engine::features::search::fold::{FoldOptions, fold_text, to_original_range};

    // The translation the whole anchor stage rests on: a match found in
    // folded space becomes a range in the original. Exhaustive over every
    // folded range in every case, because the boundary that matters is a
    // range ending on the first unit of a collapsed whitespace run, and a
    // sampled sweep steps over it.
    let file = load_folds();
    let mut failures: Vec<String> = Vec::new();
    let mut probes = 0usize;

    for case in &file.cases {
        let options = FoldOptions { keep_case: case.keep_case, keep_marks: case.keep_marks };
        let folded = fold_text(&case.text, options);
        for probe in &case.ranges {
            let Some(actual) = to_original_range(&folded, probe.start, probe.length) else {
                failures.push(format!(
                    "{}: range [{}, {}) translated to nothing",
                    case.name, probe.start, probe.length,
                ));
                continue;
            };
            let expected = &probe.original;
            if actual.start != expected.start || actual.length != expected.length {
                failures.push(format!(
                    "{}: folded [{}, +{}) became [{}, +{}), TypeScript said [{}, +{})",
                    case.name,
                    probe.start,
                    probe.length,
                    actual.start,
                    actual.length,
                    expected.start,
                    expected.length,
                ));
            }
            probes += 1;
        }
    }

    assert!(failures.is_empty(), "range translation divergence:\n  {}", failures.join("\n  "));
    assert!(probes >= 1_000, "only {probes} ranges checked");
}

// --- G5: literal search.

#[derive(Debug, Deserialize)]
struct LiteralFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<LiteralCase>,
}

#[derive(Debug, Deserialize)]
struct LiteralCase {
    name: String,
    covers: String,
    haystack: String,
    needle: String,
    #[serde(default, rename = "matchCase")]
    match_case: bool,
    #[serde(default, rename = "matchDiacritics")]
    match_diacritics: bool,
    #[serde(default, rename = "wholeWord")]
    whole_word: bool,
    matches: Vec<Range>,
}

#[test]
fn literal_search_agrees_with_typescript() {
    use epdf_engine::features::search::fold::fold_text;
    use epdf_engine::features::search::literal::{LiteralQuery, match_literal};

    let path = test_support::workspace_root()
        .join("tooling/engine-conformance/vectors/search/literal.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: LiteralFile = serde_json::from_str(&text).expect("literal.json");
    assert_eq!(file.schema_version, 1, "vector envelope moved");
    assert_eq!(file.kind, "search.literal");
    assert!(file.cases.len() >= 22, "the corpus shrank");

    let mut failures: Vec<String> = Vec::new();
    for case in &file.cases {
        let query = LiteralQuery {
            text: case.needle.clone(),
            match_case: case.match_case,
            match_diacritics: case.match_diacritics,
            whole_word: case.whole_word,
        };
        let haystack = fold_text(&case.haystack, query.fold_options());
        let actual: Vec<Range> = match_literal(&haystack, &query)
            .into_iter()
            .map(|r| Range { start: r.start as usize, length: r.length as usize })
            .collect();
        if actual != case.matches {
            failures.push(format!(
                "{}: matched {:?}, TypeScript said {:?} ({})",
                case.name, actual, case.matches, case.covers,
            ));
        }
    }
    assert!(failures.is_empty(), "literal search divergence:\n  {}", failures.join("\n  "));
}

// --- G5: the snippet around a match.

#[derive(Debug, Deserialize)]
struct SnippetFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<SnippetCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnippetCase {
    name: String,
    covers: String,
    text: String,
    match_start: u32,
    match_length: u32,
    context: u32,
    snippet: SnippetVector,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SnippetVector {
    text: String,
    match_start: u32,
    match_length: u32,
}

#[test]
fn snippets_agree_with_typescript() {
    use epdf_engine::features::search::fold::OriginalRange;
    use epdf_engine::features::search::snippet::build_snippet;

    let path = test_support::workspace_root()
        .join("tooling/engine-conformance/vectors/search/snippet.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: SnippetFile = serde_json::from_str(&text).expect("snippet.json");
    assert_eq!(file.schema_version, 1, "vector envelope moved");
    assert_eq!(file.kind, "search.snippet");
    assert!(file.cases.len() >= 8, "the corpus shrank");

    let mut failures: Vec<String> = Vec::new();
    for case in &file.cases {
        let actual = build_snippet(
            &case.text,
            OriginalRange { start: case.match_start, length: case.match_length },
            case.context,
        );
        let actual = SnippetVector {
            text: actual.text,
            match_start: actual.match_start,
            match_length: actual.match_length,
        };
        if actual != case.snippet {
            failures.push(format!(
                "{}: produced {:?}, TypeScript said {:?} ({})",
                case.name, actual, case.snippet, case.covers,
            ));
        }
    }
    assert!(failures.is_empty(), "snippet divergence:\n  {}", failures.join("\n  "));
}

#[test]
fn a_snippet_locates_its_match_inside_itself() {
    // The property a highlighter depends on, checked as a property rather
    // than through the vectors: the reported offset must address the match
    // within the text returned, or the highlight lands somewhere else.
    use epdf_engine::features::search::fold::OriginalRange;
    use epdf_engine::features::search::snippet::build_snippet;

    let path = test_support::workspace_root()
        .join("tooling/engine-conformance/vectors/search/snippet.json");
    let file: SnippetFile =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");

    for case in &file.cases {
        let snippet = build_snippet(
            &case.text,
            OriginalRange { start: case.match_start, length: case.match_length },
            case.context,
        );
        let units: Vec<u16> = snippet.text.encode_utf16().collect();
        let end = snippet.match_start + snippet.match_length;
        assert!(
            end as usize <= units.len(),
            "{}: match [{}, +{}) runs past a {}-unit snippet",
            case.name,
            snippet.match_start,
            snippet.match_length,
            units.len(),
        );

        // And the characters there are the matched ones, with whitespace
        // flattened the same way the snippet flattened them.
        let page: Vec<u16> = case.text.encode_utf16().collect();
        let matched: String = String::from_utf16_lossy(
            &page[case.match_start as usize..(case.match_start + case.match_length) as usize],
        )
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
        let in_snippet =
            String::from_utf16_lossy(&units[snippet.match_start as usize..end as usize]);
        assert_eq!(in_snippet, matched, "{}: the offset does not address the match", case.name);
    }
}
