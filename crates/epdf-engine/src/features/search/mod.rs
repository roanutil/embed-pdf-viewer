//! G5. Search.
//!
//! Replaces the ported half of `core/src/search/` (655). Search output is one
//! of the bounded resources in `crate::dispatch`.
//!
//! What is here is G5's *entry* gate rather than its port: the dialect
//! validator, the dialect translation, and the engine choice they rest on. The
//! port order gives G5 an entry gate because the shipping matcher is
//! JavaScript `RegExp` and the server is RE2, and a Rust engine that
//! reproduces neither would be found out halfway through 655 lines rather than
//! before them.
//!
//! # Two engines, because no one crate does both jobs
//!
//! [`regress`] parses ECMAScript syntax, which is what the dialect validator
//! needs: `validateSearchRegex` reports `syntax` for exactly what the
//! `RegExp` constructor refuses under `u`, and that verdict is part of the
//! contract a UI shows. Parsing is linear, so using it here is safe.
//!
//! It does not execute the patterns. Measured on this machine, `regress`
//! 0.12.0 is exponential on the classic catastrophic shapes, and both of its
//! backends are:
//!
//! | pattern | subject | backtracking | PikeVM |
//! |---|---|---|---|
//! | `(?:a+)+b` | 20 `a` | 40 ms | |
//! | `(?:a+)+b` | 24 `a` | 295 ms | 1.13 s |
//! | `(?:a*)*b` | 24 `a` | 425 ms | |
//!
//! Roughly 8x per four characters. A 40-character subject does not finish, and
//! page text is longer than that. `regress::backends::DefaultExecutor` is the
//! classical backtracker, and the `backend-pikevm` feature makes a PikeVM
//! available rather than default; neither is linear here. That disqualifies it
//! for patterns a search box accepts from anyone, which is the whole reason
//! the dialect forbids backreferences and lookaround in the first place.
//!
//! So execution is the [`regex`] crate, whose linear-time guarantee is the
//! same property RE2 gives the server, over a pattern rewritten by
//! [`dialect::to_regex_syntax`].
//!
//! The cost, stated because the plan asks for it: a translation layer is a
//! second dialect to maintain, and it is the component most likely to be
//! subtly wrong. `tooling/engine-conformance/vectors/search/` holds it to the
//! shipping behaviour case by case, and
//! `crates/epdf-engine/tests/search_dialect.rs` lists what it still cannot
//! reproduce.

pub mod dialect;

use std::sync::OnceLock;

/// Longest accepted pattern, matching `SEARCH_REGEX_MAX_LENGTH` in
/// `core/src/search/regex.ts`. Measured in UTF-16 code units, because that is
/// what `pattern.length` counts in JavaScript.
pub const SEARCH_REGEX_MAX_LENGTH: usize = 512;

/// Why a pattern is not in the dialect. Wire spellings match
/// `SearchRegexIssue` in `regex.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRegexIssue {
    Empty,
    TooLong,
    Backreference,
    Lookaround,
    Syntax,
}

impl SearchRegexIssue {
    /// The `SearchRegexIssue` string the TypeScript side uses.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too-long",
            Self::Backreference => "backreference",
            Self::Lookaround => "lookaround",
            Self::Syntax => "syntax",
        }
    }
}

/// Validates a pattern against the portable search dialect: JavaScript `u`
/// mode minus what RE2 refuses.
///
/// A direct port of `validateSearchRegex`, including the order of its checks,
/// because the reported issue is part of the contract and not only the
/// accept/reject verdict. The one-pass scan tracks escape and character-class
/// state, since `\1` and `(?=` only mean backreference and lookahead outside a
/// class.
pub fn validate_search_regex(pattern: &str) -> Result<(), SearchRegexIssue> {
    if pattern.is_empty() {
        return Err(SearchRegexIssue::Empty);
    }
    // JavaScript compares `pattern.length`, which counts UTF-16 code units.
    if pattern.chars().map(char::len_utf16).sum::<usize>() > SEARCH_REGEX_MAX_LENGTH {
        return Err(SearchRegexIssue::TooLong);
    }

    let units: Vec<char> = pattern.chars().collect();
    let at = |i: usize| units.get(i).copied();
    let mut in_class = false;
    let mut i = 0;
    while i < units.len() {
        let ch = units[i];
        if ch == '\\' {
            let next = at(i + 1).unwrap_or('\0');
            if !in_class && next.is_ascii_digit() && next != '0' {
                return Err(SearchRegexIssue::Backreference);
            }
            if !in_class && next == 'k' && at(i + 2) == Some('<') {
                return Err(SearchRegexIssue::Backreference);
            }
            i += 2; // consume the escaped char
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if ch == '[' {
            in_class = true;
            i += 1;
            continue;
        }
        if ch == '(' && at(i + 1) == Some('?') {
            let tag = at(i + 2).unwrap_or('\0');
            if tag == '=' || tag == '!' {
                return Err(SearchRegexIssue::Lookaround);
            }
            // `(?<` is lookbehind when followed by = or !, a named group
            // otherwise.
            if tag == '<' && matches!(at(i + 3), Some('=') | Some('!')) {
                return Err(SearchRegexIssue::Lookaround);
            }
        }
        i += 1;
    }

    // `u` is the strict common ground with RE2's syntax; `m` because ^ and $
    // meaning line boundaries is what page-text search wants. The flags have
    // to match `regex.ts`, or a pattern legal on one side is a syntax error on
    // the other. This is `regress` rather than the executing engine because
    // the contract is "what the RegExp constructor refuses", and only an
    // ECMAScript parser answers that.
    regress::Regex::with_flags(pattern, "mu").map(|_| ()).map_err(|_| SearchRegexIssue::Syntax)?;
    // A pattern the translator cannot rewrite is one this engine cannot run,
    // and reporting that at validation time is better than at query time.
    dialect::to_regex_syntax(pattern)
        .and_then(|translated| regex::Regex::new(&translated).ok())
        .map(|_| ())
        .ok_or(SearchRegexIssue::Syntax)
}

/// Compiles a dialect pattern for execution, after translating it.
///
/// `match_case` is the absence of `i`, exactly as in `matchRegex`. Multi-line
/// is always on, because `^` and `$` meaning line boundaries is what page-text
/// search wants and what `regex.ts` hard-codes.
pub fn compile(pattern: &str, match_case: bool) -> Result<regex::Regex, SearchRegexIssue> {
    let translated = dialect::to_regex_syntax(pattern).ok_or(SearchRegexIssue::Syntax)?;
    regex::RegexBuilder::new(&translated)
        .multi_line(true)
        .case_insensitive(!match_case)
        .build()
        .map_err(|_| SearchRegexIssue::Syntax)
}

/// A half-open match range in UTF-16 code units of the original text, which is
/// what `SearchMatchRange` carries and what `PageTextSnapshot` addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatchRange {
    pub start: usize,
    pub length: usize,
}

/// One definition of "word character" for the whole search subsystem: Unicode
/// letters, numbers and underscore, never `\b`'s ASCII set.
///
/// Compiled once. It is the same test `literal.ts` and the `wholeWord`
/// post-filter share, which is what makes the toggle mean one thing in both
/// literal and regex mode: `caf` never whole-word matches inside `caf\u{e9}`.
fn word_test() -> &'static regex::Regex {
    static WORD: OnceLock<regex::Regex> = OnceLock::new();
    WORD.get_or_init(|| regex::Regex::new(r"^[\p{L}\p{N}_]$").expect("the word class compiles"))
}

fn is_word_char(ch: char) -> bool {
    let mut buffer = [0u8; 4];
    word_test().is_match(ch.encode_utf8(&mut buffer))
}

/// All matches of a dialect-valid pattern over raw page text.
///
/// A port of `matchRegex`, with its three rules intact:
///
/// - regex runs on the raw plane, never folded text, so case handling is the
///   `i` flag and diacritic folding never applies;
/// - `whole_word` is a boundary post-filter using [`is_word_char`], never
///   `\b`, and filtering after the match is also what survives the move to
///   RE2 server-side;
/// - zero-length matches are skipped, because search UI cannot highlight
///   nothing and skipping them is the infinite-loop guard for `a*`.
///
/// Offsets are converted to UTF-16 on the way out. The `regex` crate works in
/// UTF-8 byte offsets, and the difference is invisible until a document
/// contains a supplementary-plane character, which is the wrong time to find
/// out.
pub fn match_regex(
    text: &str,
    pattern: &str,
    match_case: bool,
    whole_word: bool,
) -> Result<Vec<SearchMatchRange>, SearchRegexIssue> {
    validate_search_regex(pattern)?;
    let regex = compile(pattern, match_case)?;

    // UTF-16 index of each UTF-8 byte offset that starts a character, built
    // once rather than recounted per match.
    let mut utf16_at = vec![0usize; text.len() + 1];
    let mut utf16 = 0;
    for (offset, ch) in text.char_indices() {
        utf16_at[offset] = utf16;
        utf16 += ch.len_utf16();
    }
    utf16_at[text.len()] = utf16;

    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor <= text.len() {
        let Some(found) = regex.find_at(text, cursor) else { break };
        let (start, end) = (found.start(), found.end());
        if start == end {
            // Advance one full code point past the empty match, matching the
            // `lastIndex` nudge in matchRegex.
            cursor = start + text[start..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        cursor = end;

        if whole_word {
            let before = text[..start].chars().next_back().is_some_and(is_word_char);
            let after = text[end..].chars().next().is_some_and(is_word_char);
            if before || after {
                continue;
            }
        }
        out.push(SearchMatchRange {
            start: utf16_at[start],
            length: utf16_at[end] - utf16_at[start],
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialect_refuses_what_re2_cannot_execute() {
        assert_eq!(validate_search_regex(""), Err(SearchRegexIssue::Empty));
        assert_eq!(validate_search_regex(&"a".repeat(513)), Err(SearchRegexIssue::TooLong));
        assert!(validate_search_regex(&"a".repeat(512)).is_ok());
        assert_eq!(validate_search_regex(r"(a)\1"), Err(SearchRegexIssue::Backreference));
        assert_eq!(validate_search_regex(r"(?<x>a)\k<x>"), Err(SearchRegexIssue::Backreference));
        assert_eq!(validate_search_regex("a(?=b)"), Err(SearchRegexIssue::Lookaround));
        assert_eq!(validate_search_regex("(?<=a)b"), Err(SearchRegexIssue::Lookaround));
        assert_eq!(validate_search_regex("(a"), Err(SearchRegexIssue::Syntax));
    }

    #[test]
    fn a_named_group_is_not_a_lookbehind() {
        assert!(validate_search_regex(r"(?<word>\w+)").is_ok());
    }

    #[test]
    fn the_length_limit_counts_utf16_units_like_javascript() {
        // 256 supplementary code points are 512 UTF-16 units, so exactly at
        // the limit, while `chars().count()` would see 256 and let twice as
        // much through.
        let at_limit: String = std::iter::repeat_n('\u{1F600}', 256).collect();
        assert!(validate_search_regex(&at_limit).is_ok());
        let over: String = std::iter::repeat_n('\u{1F600}', 257).collect();
        assert_eq!(validate_search_regex(&over), Err(SearchRegexIssue::TooLong));
    }

    #[test]
    fn offsets_come_back_in_utf16_units() {
        // "a😀b": the b starts at UTF-16 index 3 and UTF-8 offset 5.
        let found = match_regex("a\u{1F600}b", "b", true, false).unwrap();
        assert_eq!(found, vec![SearchMatchRange { start: 3, length: 1 }]);
    }

    #[test]
    fn a_zero_length_match_advances_a_whole_code_point() {
        // Without the code-point-sized nudge this either loops forever or
        // lands inside a surrogate pair.
        let found = match_regex("a\u{1F600}b", "x*", true, false).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn whole_word_uses_the_unicode_test_rather_than_a_word_boundary() {
        // `\b` would allow this one, because é is not an ASCII word char.
        let found = match_regex("caf\u{e9}", "caf", true, true).unwrap();
        assert!(found.is_empty());
    }
}
