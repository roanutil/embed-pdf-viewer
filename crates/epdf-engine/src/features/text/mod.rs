//! G5. Text extraction.
//!
//! Replaces the ported half of `core/src/text/` (762).

pub mod charmap;
pub mod layout;
pub mod reader;

/// Characters on a page, in its character space: `count` characters from
/// `start` (`TextRange`). The one range shape: selections, search matches
/// and segments all take or return it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextRange {
    pub start: u32,
    pub count: u32,
}
