//! `joining.ts`: which letters of a cursive script join (Arabic, Syriac,
//! N'Ko, Mongolian…), as the engine's shaper reads Unicode's joining types. A
//! letter takes its joined form on each side where it and its neighbour both
//! join. Marks between them don't stop it.

use serde::Serialize;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

use super::graphemes::code_points_of;

/// Unicode's Joining_Type: `D` joins on both sides, `R` only the character
/// before it (in typing order), `L` only the one after, `C` causes joining on
/// both, `T` is see-through (a mark), `U` joins nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum JoiningType {
    U,
    L,
    R,
    D,
    C,
    T,
}

include!(concat!(env!("OUT_DIR"), "/joining.rs"));

/// `/^[\p{Mn}\p{Me}\p{Cf}]$/u`.
fn is_transparent(code_point: u32) -> bool {
    char::from_u32(code_point).is_some_and(|c| {
        matches!(
            c.general_category(),
            GeneralCategory::NonspacingMark
                | GeneralCategory::EnclosingMark
                | GeneralCategory::Format
        )
    })
}

/// A character's joining type: the table's, else see-through for a mark or a
/// format character.
pub fn joining_type_of(code_point: u32) -> JoiningType {
    let found = JOINING_TYPES.binary_search_by(|&(first, last, _)| {
        if code_point < first {
            std::cmp::Ordering::Greater
        } else if code_point > last {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    match found {
        Ok(index) => JOINING_TYPES[index].2,
        Err(_) if is_transparent(code_point) => JoiningType::T,
        Err(_) => JoiningType::U,
    }
}

/// A grapheme's joining type: its first character's that isn't see-through.
pub fn grapheme_joining_type(grapheme: &[u16]) -> JoiningType {
    code_points_of(grapheme)
        .into_iter()
        .map(joining_type_of)
        .find(|&kind| kind != JoiningType::T)
        .unwrap_or(JoiningType::T)
}

fn joins_before(kind: JoiningType) -> bool {
    matches!(kind, JoiningType::D | JoiningType::R | JoiningType::C)
}

fn joins_after(kind: JoiningType) -> bool {
    matches!(kind, JoiningType::D | JoiningType::L | JoiningType::C)
}

/// Where a grapheme joins its neighbours: the one before it and the one
/// after, in typing order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Joins {
    pub previous: bool,
    pub next: bool,
}

/// Where each of `graphemes` joins its neighbours, shaped together: a pair
/// joins when the first joins what follows it and the second what precedes
/// it, see-through graphemes between them skipped. `together(a, b)` says
/// whether two graphemes are shaped as one (the same font and direction).
pub fn joins_of(graphemes: &[&[u16]], together: impl Fn(usize, usize) -> bool) -> Vec<Joins> {
    let types: Vec<JoiningType> = graphemes.iter().map(|g| grapheme_joining_type(g)).collect();
    let mut joins = vec![Joins::default(); graphemes.len()];
    let mut before: Option<usize> = None;
    for (index, &kind) in types.iter().enumerate() {
        if kind == JoiningType::T {
            continue;
        }
        if let Some(previous) = before
            && joins_after(types[previous])
            && joins_before(kind)
            && together(previous, index)
        {
            joins[previous].next = true;
            joins[index].previous = true;
        }
        before = Some(index);
    }
    joins
}

/// U+200D: next to a letter drawn alone, it gives the letter the form joined
/// on that side.
const ZERO_WIDTH_JOINER: u16 = 0x200d;

/// `grapheme` with a joiner on each side `joins` names: drawn alone, it takes
/// its joined form.
pub fn joined_text(grapheme: &[u16], joins: Joins) -> Vec<u16> {
    let mut text = Vec::with_capacity(grapheme.len() + 2);
    if joins.previous {
        text.push(ZERO_WIDTH_JOINER);
    }
    text.extend_from_slice(grapheme);
    if joins.next {
        text.push(ZERO_WIDTH_JOINER);
    }
    text
}
