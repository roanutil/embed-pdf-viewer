//! The excerpt shown around a match.
//!
//! G5. A transcription of `core/src/search/snippet.ts`.
//!
//! Up to a fixed amount of page text on each side of the match, trimmed to a
//! whitespace boundary when one exists inside the window so the excerpt starts
//! and ends on whole words, never splitting a surrogate pair, with whitespace
//! flattened one unit to one space.
//!
//! The flattening is length-preserving on purpose. A collapsing flatten would
//! shift every offset after it, and the match offsets this returns are indexes
//! into the flattened text; keeping the lengths equal is what lets a consumer
//! highlight without recomputing anything.

use super::fold::OriginalRange;

/// Default context on each side, in UTF-16 code units.
pub const SEARCH_SNIPPET_CONTEXT: u32 = 48;

/// An excerpt, with the match located inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSnippet {
    pub text: String,
    /// Where the match starts within `text`, in UTF-16 code units.
    pub match_start: u32,
    pub match_length: u32,
}

/// Build the excerpt around one match.
///
/// Offsets are UTF-16 code units into `text`, which is the page's extracted
/// text. The window is chosen in that space and only converted to bytes to
/// slice, so a snippet boundary always lands on a whole character.
pub fn build_snippet(text: &str, range: OriginalRange, context: u32) -> SearchSnippet {
    let units: Vec<u16> = text.encode_utf16().collect();
    let total = units.len() as u32;
    let match_end = range.start.saturating_add(range.length).min(total);
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

    let window: Vec<u16> = units[start as usize..end as usize].to_vec();
    let flattened: String = String::from_utf16_lossy(&window)
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();

    SearchSnippet {
        text: flattened,
        match_start: start_of_match - start,
        match_length: range.length,
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
