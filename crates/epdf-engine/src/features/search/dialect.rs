//! Translating the search dialect from JavaScript `u`-mode syntax into the
//! `regex` crate's.
//!
//! The two are close and not the same, and every difference below reaches a
//! real query rather than a corner of the grammar:
//!
//! | JavaScript `u` | `regex` crate | Why they differ |
//! |---|---|---|
//! | `\d` `\w` are ASCII | Unicode by default | `\w+` would match Arabic |
//! | `\s` is the ECMAScript set | `\p{White_Space}` | U+FEFF in one, U+0085 in the other |
//! | `\b` is an ASCII boundary | Unicode boundary | `\bcafe\b` inside `café` |
//! | `.` excludes four terminators | excludes `\n` | `a.b` across a `\r` |
//! | `\p{Script=Greek}` | `\p{sc:Greek}` | spelling only |
//! | `\uFFFF` | `\x{FFFF}` | spelling only |
//!
//! The translation is a scan rather than a parse, which is sound here because
//! it only rewrites escape sequences and `.`, and it tracks the two states
//! that change what those mean: inside a character class, and after a
//! backslash. Anything it does not recognize passes through, so a construct
//! the two dialects already agree on costs nothing.
//!
//! What it does not fix is listed in `KNOWN_DIVERGENCES` in
//! `crates/epdf-engine/tests/search_dialect.rs`, because a translator that
//! silently papered over a semantic gap would be worse than one that leaves it
//! visible.

/// The ECMAScript `\s` set: WhiteSpace plus LineTerminator, spelled as a
/// `regex` character class body.
///
/// Neither `\s` nor `\p{White_Space}` is this set. ECMAScript includes U+FEFF,
/// which `White_Space` does not, and excludes U+0085, which it does.
const JS_WHITESPACE: &str = r"\t\n\x{0B}\f\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// ECMAScript `\w`: deliberately ASCII, even under `u`.
const JS_WORD: &str = r"0-9A-Za-z_";

/// ECMAScript `.` without the `s` flag: anything but a line terminator.
const JS_DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// Rewrites a dialect-valid JavaScript pattern into `regex` crate syntax.
///
/// Returns `None` when the pattern uses something the scan cannot faithfully
/// rewrite, which is better than emitting a pattern that compiles and means
/// something else.
pub fn to_regex_syntax(pattern: &str) -> Option<String> {
    let units: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut in_class = false;
    let mut i = 0;

    while i < units.len() {
        let ch = units[i];
        if ch == '\\' {
            let &next = units.get(i + 1)?;
            i += 2;
            match next {
                'd' => out.push_str("[0-9]"),
                'D' => out.push_str("[^0-9]"),
                'w' => out.push_str(&bracket(in_class, JS_WORD, false)),
                'W' => out.push_str(&bracket(in_class, JS_WORD, true)),
                's' => out.push_str(&bracket(in_class, JS_WHITESPACE, false)),
                'S' => out.push_str(&bracket(in_class, JS_WHITESPACE, true)),
                // Inside a class `\b` is a backspace; outside it is a word
                // boundary, and JavaScript's is the ASCII one.
                'b' if in_class => out.push_str(r"\x{8}"),
                'b' => out.push_str(r"(?-u:\b)"),
                'B' if in_class => return None,
                'B' => out.push_str(r"(?-u:\B)"),
                'u' => {
                    let (escape, consumed) = unicode_escape(&units[i..])?;
                    out.push_str(&escape);
                    i += consumed;
                }
                'p' | 'P' => {
                    let (property, consumed) = unicode_property(next, &units[i..])?;
                    out.push_str(&property);
                    i += consumed;
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            continue;
        }

        // `\d` and friends expand to a bracketed class, which cannot nest
        // inside another one in `regex` syntax without the set-operation
        // spelling. `bracket` handles that by emitting the body bare when
        // already inside a class; a nested `[` here would be a different
        // construct in each dialect, so refuse it.
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            out.push(ch);
            i += 1;
            continue;
        }
        match ch {
            '[' => {
                in_class = true;
                out.push('[');
            }
            '.' => out.push_str(JS_DOT),
            other => out.push(other),
        }
        i += 1;
    }

    if in_class { None } else { Some(out) }
}

/// A class body, bracketed when it stands alone and bare when it is already
/// inside `[...]`.
///
/// A negated shorthand inside a class (`[\W]`) cannot be expressed by
/// inlining, so it keeps its brackets and relies on `regex`'s nested-class
/// support.
fn bracket(in_class: bool, body: &str, negated: bool) -> String {
    match (in_class, negated) {
        (false, false) => format!("[{body}]"),
        (false, true) => format!("[^{body}]"),
        (true, false) => body.to_string(),
        (true, true) => format!("[^{body}]"),
    }
}

/// `\uFFFF` and `\u{10FFFF}` become `\x{...}`, which is the spelling `regex`
/// takes. `rest` starts just past the `u`.
fn unicode_escape(rest: &[char]) -> Option<(String, usize)> {
    if rest.first() == Some(&'{') {
        let close = rest.iter().position(|&c| c == '}')?;
        let digits: String = rest[1..close].iter().collect();
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return Some((format!("\\x{{{digits}}}"), close + 1));
    }
    if rest.len() < 4 || !rest[..4].iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digits: String = rest[..4].iter().collect();
    Some((format!("\\x{{{digits}}}"), 4))
}

/// `\p{Script=Greek}` becomes `\p{sc:Greek}`; a bare `\p{L}` passes through.
/// `rest` starts just past the `p` or `P`.
fn unicode_property(kind: char, rest: &[char]) -> Option<(String, usize)> {
    if rest.first() != Some(&'{') {
        return None;
    }
    let close = rest.iter().position(|&c| c == '}')?;
    let body: String = rest[1..close].iter().collect();
    let translated = match body.split_once('=') {
        Some(("Script", value)) => format!("sc:{value}"),
        Some(("Script_Extensions", value)) => format!("scx:{value}"),
        Some(("General_Category", value)) => value.to_string(),
        // Any other `Name=Value` property: `regex` spells binary properties
        // and blocks differently enough that guessing would be worse than
        // refusing.
        Some(_) => return None,
        None => body,
    };
    Some((format!("\\{kind}{{{translated}}}"), close + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthands_become_their_ecmascript_sets() {
        assert_eq!(to_regex_syntax(r"\d").as_deref(), Some("[0-9]"));
        assert_eq!(to_regex_syntax(r"\w").as_deref(), Some("[0-9A-Za-z_]"));
        assert_eq!(to_regex_syntax(r"\W").as_deref(), Some("[^0-9A-Za-z_]"));
    }

    #[test]
    fn a_shorthand_inside_a_class_loses_its_brackets() {
        assert_eq!(to_regex_syntax(r"[\w-]").as_deref(), Some("[0-9A-Za-z_-]"));
    }

    #[test]
    fn backslash_b_means_two_different_things() {
        assert_eq!(to_regex_syntax(r"\bx").as_deref(), Some(r"(?-u:\b)x"));
        assert_eq!(to_regex_syntax(r"[\b]").as_deref(), Some(r"[\x{8}]"));
    }

    #[test]
    fn dot_stops_at_every_javascript_line_terminator() {
        assert_eq!(to_regex_syntax("a.b").as_deref(), Some(r"a[^\n\r\x{2028}\x{2029}]b"));
        // Inside a class a dot is a literal dot in both dialects.
        assert_eq!(to_regex_syntax("[.]").as_deref(), Some("[.]"));
    }

    #[test]
    fn property_names_are_respelled_only_where_they_differ() {
        assert_eq!(to_regex_syntax(r"\p{L}").as_deref(), Some(r"\p{L}"));
        assert_eq!(to_regex_syntax(r"\p{Script=Arabic}").as_deref(), Some(r"\p{sc:Arabic}"));
        assert_eq!(
            to_regex_syntax(r"\p{Script_Extensions=Greek}").as_deref(),
            Some(r"\p{scx:Greek}")
        );
        assert_eq!(to_regex_syntax(r"\P{L}").as_deref(), Some(r"\P{L}"));
    }

    #[test]
    fn code_point_escapes_are_respelled() {
        assert_eq!(to_regex_syntax(r"\u00e9").as_deref(), Some(r"\x{00e9}"));
        assert_eq!(to_regex_syntax(r"\u{1F600}").as_deref(), Some(r"\x{1F600}"));
    }

    #[test]
    fn an_unrewritable_pattern_is_refused_rather_than_mistranslated() {
        // A property spelling with no faithful equivalent.
        assert_eq!(to_regex_syntax(r"\p{Block=Cyrillic}"), None);
        // An unterminated class.
        assert_eq!(to_regex_syntax("[abc"), None);
        // A trailing backslash.
        assert_eq!(to_regex_syntax(r"a\"), None);
    }

    #[test]
    fn everything_else_passes_through_untouched() {
        assert_eq!(to_regex_syntax(r"(?:ca)+t|^x$").as_deref(), Some(r"(?:ca)+t|^x$"));
    }
}

/// The three line terminators JavaScript recognizes that the `regex` crate
/// does not: carriage return, LINE SEPARATOR and PARAGRAPH SEPARATOR.
///
/// `\n` is deliberately absent: it is the one both dialects agree on, and the
/// one the others are normalized to.
pub const EXTRA_LINE_TERMINATORS: [char; 3] = ['\r', '\u{2028}', '\u{2029}'];

/// Whether the pattern contains `^` or `$` as an anchor.
///
/// Escaped (`\^`) and in-class (`[$]`) occurrences are literals and do not
/// count, which is the whole reason this is a scan rather than a `contains`.
///
/// [`match_regex`](super::match_regex) uses this to decide whether a query
/// needs the normalized haystack at all. Most do not, and a query that does
/// not is matched against the real text with no substitution and therefore no
/// approximation.
pub fn has_line_anchor(pattern: &str) -> bool {
    let mut in_class = false;
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '^' | '$' if !in_class => return true,
            _ => {}
        }
    }
    false
}

/// A copy of `text` with every JavaScript line terminator rewritten to `\n`,
/// plus a map from each byte offset in the copy back to the original.
///
/// `None` when the text contains none of them, which is the common case and
/// the one worth not copying for.
///
/// The map is per byte rather than per character because match offsets arrive
/// as byte offsets. A substituted character is one character in each string,
/// so a match boundary always lands on a character start in both.
pub fn normalize_line_terminators(text: &str) -> Option<(String, Vec<usize>)> {
    if !text.contains(EXTRA_LINE_TERMINATORS) {
        return None;
    }
    let mut shadow = String::with_capacity(text.len());
    let mut back = Vec::with_capacity(text.len() + 1);
    for (offset, ch) in text.char_indices() {
        let replacement = if EXTRA_LINE_TERMINATORS.contains(&ch) { '\n' } else { ch };
        for _ in 0..replacement.len_utf8() {
            back.push(offset);
        }
        shadow.push(replacement);
    }
    back.push(text.len());
    Some((shadow, back))
}

#[cfg(test)]
mod terminator_tests {
    use super::*;

    #[test]
    fn an_anchor_is_only_an_anchor_outside_a_class_and_unescaped() {
        assert!(has_line_anchor("^abc"));
        assert!(has_line_anchor("abc$"));
        assert!(has_line_anchor(r"a|^b"));
        assert!(!has_line_anchor(r"\^abc"));
        assert!(!has_line_anchor(r"abc\$"));
        assert!(!has_line_anchor("[a^b]"));
        assert!(!has_line_anchor("[$]"));
        assert!(!has_line_anchor(r"[\]^]"));
    }

    #[test]
    fn text_without_a_stray_terminator_is_not_copied() {
        assert!(normalize_line_terminators("plain\ntext").is_none());
        assert!(normalize_line_terminators("").is_none());
    }

    #[test]
    fn every_javascript_terminator_becomes_a_newline() {
        let (shadow, _) = normalize_line_terminators("a\rb\u{2028}c\u{2029}d\ne").unwrap();
        assert_eq!(shadow, "a\nb\nc\nd\ne");
    }

    #[test]
    fn the_map_carries_multi_byte_offsets_back() {
        // U+2028 is three bytes in the original and one in the shadow, so
        // everything after it shifts by two.
        let original = "a\u{2028}b";
        let (shadow, back) = normalize_line_terminators(original).unwrap();
        assert_eq!(shadow, "a\nb");
        assert_eq!(back.len(), shadow.len() + 1);
        assert_eq!(back[0], 0);
        assert_eq!(back[1], 1);
        assert_eq!(back[2], 4);
        assert_eq!(back[3], original.len());
    }
}
