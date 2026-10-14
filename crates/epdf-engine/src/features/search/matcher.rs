//! The whole match stage in one call.
//!
//! G5. A transcription of `core/src/search/matcher.ts`.
//!
//! Page text in, match ranges out, in the original text's space. An engine
//! with a pre-folded corpus for the default fold skips this and calls
//! [`super::literal::match_literal`] on the cached text directly; everything
//! else comes through here.

use super::fold::{OriginalRange, fold_text};
use super::literal::{LiteralQuery, match_literal};
use super::{SearchRegexIssue, match_regex};

/// A query in either mode.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    /// Every flag defaults to false rather than being required, because the
    /// wire omits a flag it is not setting and a missing `matchCase` means
    /// case-folded rather than malformed.
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub match_case: bool,
    #[serde(default)]
    pub match_diacritics: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub ignore_whitespace: bool,
}

impl SearchQuery {
    fn literal(&self) -> LiteralQuery {
        LiteralQuery {
            text: self.text.clone(),
            match_case: self.match_case,
            match_diacritics: self.match_diacritics,
            whole_word: self.whole_word,
            ignore_whitespace: self.ignore_whitespace,
        }
    }
}

/// Every match on a page.
///
/// Regex mode does not fold. That is the TypeScript's behaviour and it is the
/// right one: a pattern says what it means literally, and folding it first
/// would change what its character classes match.
pub fn match_page_text(
    text: &str,
    query: &SearchQuery,
) -> Result<Vec<OriginalRange>, SearchRegexIssue> {
    if query.regex {
        return Ok(match_regex(text, &query.text, query.match_case, query.whole_word)?
            .into_iter()
            .map(|m| OriginalRange { start: m.start as u32, length: m.length as u32 })
            .collect());
    }
    let literal = query.literal();
    Ok(match_literal(&fold_text(text, literal.fold_options()), &literal))
}
