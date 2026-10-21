//! The excerpt shown around a match.
//!
//! G5. A transcription of `core/src/search/snippet.ts`.
//!
//! Up to a fixed amount of page text on each side of the match, trimmed to a
//! whitespace boundary when one exists inside the window so the excerpt starts
//! and ends on whole words, never splitting a surrogate pair, with whitespace
//! flattened one unit to one space. It comes in three parts, the match between
//! the text before and after it, so a consumer highlights the middle part
//! without counting offsets.

use crate::features::text::TextRange;

/// Default context on each side, in UTF-16 code units.
pub const SEARCH_SNIPPET_CONTEXT: u32 = 48;

/// The text around one match, ready to render: `match` between `before` and
/// `after`, whitespace flattened to spaces (`SearchSnippet`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SearchSnippet {
    pub before: String,
    #[serde(rename = "match")]
    pub matched: String,
    pub after: String,
}

/// Build the excerpt around one match.
///
/// Offsets are UTF-16 code units into `text`, which is the page's extracted
/// text. The window is chosen in that space and only converted to bytes to
/// slice, so a snippet boundary always lands on a whole character.
pub fn build_snippet(text: &str, range: TextRange, context: u32) -> SearchSnippet {
    let units: Vec<u16> = text.encode_utf16().collect();
    let total = units.len() as u32;
    let match_end = range.start.saturating_add(range.count).min(total);
    let start_of_match = range.start.min(total);

    let mut start = start_of_match.saturating_sub(context);
    if start > 0 {
        // Cut at the first whitespace inside the leading context, if there is
        // one. Otherwise accept the mid-word cut, which is one very long
        // token and not worth a worse rule.
        let mut cut = None;
        for i in start..start_of_match {
            if is_whitespace_unit(&units, i) {
                cut = Some(i + 1);
                break;
            }
        }
        if let Some(cut) = cut {
            start = cut;
        }
        if is_low_surrogate(&units, start) {
            start += 1;
        }
    }

    let mut end = match_end.saturating_add(context).min(total);
    if end < total {
        let mut cut = None;
        let mut i = end;
        while i > match_end {
            i -= 1;
            if is_whitespace_unit(&units, i) {
                cut = Some(i);
                break;
            }
        }
        if let Some(cut) = cut {
            end = cut;
        }
        if is_low_surrogate(&units, end) {
            end -= 1;
        }
    }

    let flat = |from: u32, to: u32| -> String {
        String::from_utf16_lossy(&units[from as usize..to as usize])
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect()
    };
    SearchSnippet {
        before: flat(start, start_of_match),
        matched: flat(start_of_match, match_end),
        after: flat(match_end, end),
    }
}

fn is_whitespace_unit(units: &[u16], index: u32) -> bool {
    units
        .get(index as usize)
        .and_then(|u| char::from_u32(u32::from(*u)))
        .is_some_and(char::is_whitespace)
}

fn is_low_surrogate(units: &[u16], index: u32) -> bool {
    units.get(index as usize).is_some_and(|u| (0xDC00..=0xDFFF).contains(u))
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/search/snippet.test.ts`.

    use super::*;

    fn range(start: usize, count: u32) -> TextRange {
        TextRange { start: start as u32, count }
    }

    /// The snippet as one line of text.
    fn whole(s: &SearchSnippet) -> String {
        format!("{}{}{}", s.before, s.matched, s.after)
    }

    /// A UTF-16 index of `needle` in `text`.
    fn index_of(text: &str, needle: &str) -> usize {
        text[..text.find(needle).expect("present")].encode_utf16().count()
    }

    #[test]
    fn the_match_part_is_the_matched_text_between_its_context() {
        let text = "The quick brown fox jumps over the lazy dog near the river bank today";
        let s = build_snippet(text, range(index_of(text, "jumps"), 5), SEARCH_SNIPPET_CONTEXT);
        assert_eq!(s.matched, "jumps");
        assert!(s.before.ends_with("fox "), "{s:?}");
        assert!(s.after.starts_with(" over"), "{s:?}");
    }

    #[test]
    fn trims_to_word_boundaries_inside_the_context_window() {
        let text = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima";
        let s = build_snippet(text, range(index_of(text, "foxtrot"), 7), 12);
        assert!(!whole(&s).starts_with(' '), "{s:?}");
        assert!(!whole(&s).ends_with(' '), "{s:?}");
        assert!(text.contains(&whole(&s)), "{s:?}");
        assert_eq!(s.matched, "foxtrot");
    }

    #[test]
    fn a_match_at_the_very_start_has_no_text_before_it() {
        let s = build_snippet("match at start of page", range(0, 5), SEARCH_SNIPPET_CONTEXT);
        assert_eq!(s.before, "");
        assert_eq!(s.matched, "match");
    }

    #[test]
    fn accepts_a_mid_token_cut_when_one_long_token_fills_the_window() {
        let text = format!("{}needle{}", "x".repeat(200), "y".repeat(200));
        let s = build_snippet(&text, range(200, 6), 20);
        assert_eq!(s.matched, "needle");
        assert!(whole(&s).encode_utf16().count() <= 6 + 40);
    }

    #[test]
    fn flattens_whitespace_to_spaces_in_every_part() {
        let text = "before\n\tthe match\r\nafter words here";
        let s = build_snippet(text, range(index_of(text, "match"), 5), SEARCH_SNIPPET_CONTEXT);
        assert!(!whole(&s).contains(['\n', '\t', '\r']), "{s:?}");
        assert_eq!(s.matched, "match");
        assert!(s.after.starts_with("  after"), "{s:?}");
    }

    #[test]
    fn never_splits_surrogate_pairs_at_the_window_edges() {
        let text = format!("{} needle {}", "\u{1F600}".repeat(30), "\u{1F600}".repeat(30));
        let s = build_snippet(&text, range(index_of(&text, "needle"), 6), 15);
        // A split pair would come back as U+FFFD from the lossy decode.
        assert!(!whole(&s).contains('\u{FFFD}'), "{s:?}");
        assert_eq!(s.matched, "needle");
    }
}
