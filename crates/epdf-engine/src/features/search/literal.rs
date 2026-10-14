//! Literal search, over folded text.
//!
//! G5. A transcription of `core/src/search/literal.ts`.
//!
//! Both the haystack and the needle go through the same fold, so the
//! comparison is between two normalized forms. The word-character definition
//! lives here and is shared with the regex whole-word post-filter, so the
//! toggle means one thing in both modes: "caf" never whole-word matches inside
//! "café", regex or not.

use super::fold::{FoldOptions, FoldedText, OriginalRange, fold_text, to_original_range};

/// A literal query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LiteralQuery {
    pub text: String,
    pub match_case: bool,
    pub match_diacritics: bool,
    pub whole_word: bool,
    pub ignore_whitespace: bool,
}

impl LiteralQuery {
    /// The fold both sides of this query need.
    ///
    /// The haystack must be folded with exactly these options. A corpus cache
    /// keys its pre-folded text on them, and the default fold is the
    /// persistable one; a query with other flags folds the page at query time.
    pub fn fold_options(&self) -> FoldOptions {
        FoldOptions {
            keep_case: self.match_case,
            keep_marks: self.match_diacritics,
            drop_whitespace: self.ignore_whitespace,
        }
    }
}

/// One definition of "word character" for the whole search subsystem.
///
/// Unicode letters, numbers and underscore, not the ASCII set JavaScript's
/// `\b` uses. The difference is the point: with `\b`, whole-word search would
/// mean something different for accented text than for ASCII, and the toggle
/// would behave differently in literal and regex mode.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether the character ending at a UTF-16 index is a word character.
pub fn word_before(text: &str, utf16_index: usize) -> bool {
    if utf16_index == 0 {
        return false;
    }
    char_ending_at(text, utf16_index).is_some_and(is_word_char)
}

/// Whether the character starting at a UTF-16 index is a word character.
pub fn word_at(text: &str, utf16_index: usize) -> bool {
    char_starting_at(text, utf16_index).is_some_and(is_word_char)
}

fn char_starting_at(text: &str, utf16_index: usize) -> Option<char> {
    let mut units = 0usize;
    for ch in text.chars() {
        if units == utf16_index {
            return Some(ch);
        }
        units += ch.len_utf16();
        if units > utf16_index {
            // Inside a surrogate pair. The character is the pair, which is
            // what a word test should see rather than half of one.
            return Some(ch);
        }
    }
    None
}

fn char_ending_at(text: &str, utf16_index: usize) -> Option<char> {
    let mut units = 0usize;
    let mut previous = None;
    for ch in text.chars() {
        if units >= utf16_index {
            break;
        }
        units += ch.len_utf16();
        previous = Some(ch);
    }
    previous
}

/// Whether a character is a combining mark, by the same rule the fold uses.
///
/// The same function, not a second opinion. A literal search that disagreed
/// with the fold about what a mark is would refuse matches the fold had
/// already stripped the marks from.
fn is_mark(c: char) -> bool {
    super::fold::is_combining_mark(c)
}

/// Whether the folded hit at `at` sits on word boundaries.
///
/// Checked on the folded plane, except under `ignore_whitespace`: dropping
/// whitespace glues neighbouring words together ("i n v o i c e 42" folds to
/// "invoice42"), so there the boundaries are read off the ORIGINAL text
/// around the mapped range.
fn is_whole_word_hit(
    haystack: &FoldedText,
    at: usize,
    needle_length: usize,
    query: &LiteralQuery,
) -> bool {
    if !query.ignore_whitespace {
        return !word_before(&haystack.folded, at)
            && !word_at(&haystack.folded, at + needle_length);
    }
    let Some(range) = to_original_range(haystack, at, needle_length) else {
        return false;
    };
    let (start, end) = (range.start as usize, (range.start + range.length) as usize);
    !word_before(&haystack.original, start) && !word_at(&haystack.original, end)
}

/// Every non-overlapping literal match, in the original text's UTF-16 space.
///
/// `haystack` must have been folded with [`LiteralQuery::fold_options`] for
/// this query. Folding it differently is not a wrong answer, it is an answer
/// to a different question, and nothing here can detect the mismatch.
pub fn match_literal(haystack: &FoldedText, query: &LiteralQuery) -> Vec<OriginalRange> {
    let needle = fold_text(&query.text, query.fold_options()).folded;
    // A needle that is empty or only whitespace would match every collapsed
    // space on the page, which is not what anybody typed it for.
    if needle.trim().is_empty() {
        return Vec::new();
    }

    // Both strings are searched in UTF-16 units, because that is the space
    // the map is indexed in and the space the wire reports.
    let hay: Vec<u16> = haystack.folded.encode_utf16().collect();
    let pin: Vec<u16> = needle.encode_utf16().collect();
    if pin.len() > hay.len() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut from = 0usize;
    while from + pin.len() <= hay.len() {
        let Some(offset) = hay[from..].windows(pin.len()).position(|w| w == pin.as_slice()) else {
            break;
        };
        let at = from + offset;

        if query.whole_word && !is_whole_word_hit(haystack, at, pin.len(), query) {
            from = at + 1;
            continue;
        }

        // A diacritic-sensitive fold is decomposed, so a bare needle would
        // otherwise match just the base letters of an accented character:
        // "cafe" ending immediately before the combining acute of "café". A
        // match may not end mid-character.
        if query.match_diacritics
            && at + pin.len() < hay.len()
            && char_starting_at(&haystack.folded, at + pin.len()).is_some_and(is_mark)
        {
            from = at + 1;
            continue;
        }

        if let Some(range) = to_original_range(haystack, at, pin.len()) {
            out.push(range);
        }
        from = at + pin.len();
    }
    out
}
