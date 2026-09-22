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
/// the reason each one is recorded rather than fixed. Empty is the goal; a
/// non-empty list is a statement about what G5 still has to resolve.
///
/// Both entries below are the same defect, seen from each anchor. JavaScript
/// `m` mode treats `\n`, `\r`, U+2028 and U+2029 as line boundaries; the
/// `regex` crate treats only `\n`. `RegexBuilder::line_terminator` takes one
/// byte, and the lookbehind that would express the rest is not in the crate
/// and not in the dialect. Normalizing the haystack would move every offset
/// after a U+2028, which is three UTF-8 bytes where `\n` is one, and would
/// change what a pattern containing `\r` matches.
///
/// It is not cosmetic. PDFium emits `\r` in extracted text for some
/// documents, so a `^`-anchored regex query would find different matches on
/// web and on native for those files. G5 owns the fix; this list is here so
/// the gate does not go quiet about it in the meantime.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[
    (
        "caret-after-a-carriage-return",
        "the regex crate breaks m-mode ^ on \\n only, JavaScript on all four line terminators",
    ),
    ("dollar-before-a-carriage-return", "the same gap at $; see the note above KNOWN_DIVERGENCES"),
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
    folded: String,
    /// Folded code unit to the UTF-16 index of the code point it came from.
    map: Vec<usize>,
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
/// The Rust fold itself is G5, not Phase 0: NFKD is a Unicode table, not a
/// hundred lines. What Phase 0 owes is the vector file, and a vector file
/// nothing reads is a file nobody notices breaking. So this checks the
/// properties the whole anchor stage rests on, and doubles as the skeleton the
/// G5 runner grows into:
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
