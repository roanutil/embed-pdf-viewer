//! Page labels as sections (`PageLabelSection`): the label of a page, and
//! where the sections go when pages move, come or go.
//!
//! Ports `core/src/pages/labels.ts`, `core/src/dto/PageLabelSection.ts` and
//! `services/src/features/pages/internal/pageLabels.ts`. The engine writes
//! `/PageLabels` with these rules and a viewer predicts with them, so both
//! label the pages the same way:
//!
//!   - a section stays with the page it starts at, and a page takes the
//!     numbering of the section it lands in;
//!   - when a section's first page goes, the section starts on its next
//!     page, and a section left with no pages goes;
//!   - the first section always starts at the first page, so pages put
//!     before every page join it;
//!   - a move is a delete followed by an insert.

use serde::{Deserialize, Serialize};

use pdfium::Document;

use crate::error::{EngineError, EngineErrorCode};

/// How a section numbers its pages (`/S`, ISO 32000-1 §12.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageLabelStyle {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
}

/// Pages numbered together (`/PageLabels`): from `first_page` up to the next
/// section's, each labelled `prefix` followed by its number in `style`,
/// counting from `first_number`. The first section starts at page 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLabelSection {
    /// The index of its first page.
    pub first_page: u32,
    /// `None`: no number, the label is the prefix alone.
    pub style: Option<PageLabelStyle>,
    /// The number of its first page (`/St`).
    pub first_number: i32,
    /// What each of its labels starts with (`/P`); `""` for none.
    pub prefix: String,
}

impl PageLabelStyle {
    fn from_native(style: pdfium::PageLabelStyle) -> Self {
        match style {
            pdfium::PageLabelStyle::Decimal => Self::Decimal,
            pdfium::PageLabelStyle::UpperRoman => Self::UpperRoman,
            pdfium::PageLabelStyle::LowerRoman => Self::LowerRoman,
            pdfium::PageLabelStyle::UpperLetters => Self::UpperLetters,
            pdfium::PageLabelStyle::LowerLetters => Self::LowerLetters,
        }
    }

    fn native(self) -> pdfium::PageLabelStyle {
        match self {
            Self::Decimal => pdfium::PageLabelStyle::Decimal,
            Self::UpperRoman => pdfium::PageLabelStyle::UpperRoman,
            Self::LowerRoman => pdfium::PageLabelStyle::LowerRoman,
            Self::UpperLetters => pdfium::PageLabelStyle::UpperLetters,
            Self::LowerLetters => pdfium::PageLabelStyle::LowerLetters,
        }
    }
}

/// `readLabelSections`: the document's sections, as the fork reads
/// `/PageLabels`, in page order. Empty when it has none.
pub fn read_label_sections(document: &Document) -> Result<Vec<PageLabelSection>, EngineError> {
    let sections = document.page_label_sections().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the page label sections failed: {e}"),
        )
    })?;
    Ok(sections
        .into_iter()
        .map(|section| PageLabelSection {
            first_page: section.first_page,
            style: section.style.map(PageLabelStyle::from_native),
            first_number: section.first_number,
            prefix: section.prefix,
        })
        .collect())
}

/// `writeLabelSections`: `sections` as the document's `/PageLabels`; none
/// removes it. The sections are the caller's to get right. `Unknown` when
/// the fork refuses them.
pub fn write_label_sections(
    document: &Document,
    sections: &[PageLabelSection],
) -> Result<(), EngineError> {
    let native: Vec<pdfium::PageLabelSection> = sections
        .iter()
        .map(|section| pdfium::PageLabelSection {
            first_page: section.first_page,
            style: section.style.map(PageLabelStyle::native),
            first_number: section.first_number,
            prefix: section.prefix.clone(),
        })
        .collect();
    if document.set_page_label_sections(&native) {
        return Ok(());
    }
    Err(EngineError::with_message(EngineErrorCode::Unknown, "EPDFDoc_SetPageLabelSections refused")
        .with_details(serde_json::json!({ "sections": sections.len() })))
}

/// `labelAt`: the label of the page at `index`, its section's prefix and its
/// number in the section's style, as PDFium writes them. `None` when no
/// section covers it, or the label is empty.
pub fn label_at(sections: &[PageLabelSection], index: u32) -> Option<String> {
    let section = sections.iter().take_while(|each| each.first_page <= index).last()?;
    let number = i64::from(section.first_number) + i64::from(index) - i64::from(section.first_page);
    let label = format!("{}{}", section.prefix, number_in(section.style, number));
    (!label.is_empty()).then_some(label)
}

/// `sectionsAfterDelete`: the sections once the pages at `indexes` (of
/// `page_count`) are deleted.
pub fn sections_after_delete(
    sections: &[PageLabelSection],
    page_count: u32,
    indexes: &[u32],
) -> Vec<PageLabelSection> {
    if sections.is_empty() || indexes.is_empty() {
        return sections.to_vec();
    }
    // kept_before[i]: how many pages before index i stay.
    let mut kept_before = vec![0u32];
    for index in 0..page_count {
        let last = kept_before[index as usize];
        kept_before.push(last + u32::from(!indexes.contains(&index)));
    }
    let kept = |index: u32| kept_before[index.min(page_count) as usize];
    let mut next = Vec::new();
    for (at, section) in sections.iter().enumerate() {
        let end = sections.get(at + 1).map_or(page_count, |next| next.first_page);
        if kept(end) == kept(section.first_page) {
            continue;
        }
        // Its first page that stays, which is where the pages before it end.
        next.push(PageLabelSection { first_page: kept(section.first_page), ..section.clone() });
    }
    next
}

/// `sectionsAfterInsert`: the sections once `count` pages come in at `at`.
/// A section that starts at or after `at` moves along with its first page,
/// so the new pages take the numbering of the section before them, or the
/// first section's at the start.
pub fn sections_after_insert(
    sections: &[PageLabelSection],
    at: u32,
    count: u32,
) -> Vec<PageLabelSection> {
    sections
        .iter()
        .map(|section| {
            if count == 0 || section.first_page == 0 || section.first_page < at {
                section.clone()
            } else {
                PageLabelSection { first_page: section.first_page + count, ..section.clone() }
            }
        })
        .collect()
}

/// `sectionsAfterMove`: the sections once a move that changes the order
/// takes the pages at `indexes` (of `page_count`) out and puts them back
/// together at `at`, the index of the first of them afterwards.
pub fn sections_after_move(
    sections: &[PageLabelSection],
    page_count: u32,
    indexes: &[u32],
    at: u32,
) -> Vec<PageLabelSection> {
    sections_after_insert(
        &sections_after_delete(sections, page_count, indexes),
        at,
        indexes.len() as u32,
    )
}

const ROMAN: [(i64, &str); 13] = [
    (1000, "m"),
    (900, "cm"),
    (500, "d"),
    (400, "cd"),
    (100, "c"),
    (90, "xc"),
    (50, "l"),
    (40, "xl"),
    (10, "x"),
    (9, "ix"),
    (5, "v"),
    (4, "iv"),
    (1, "i"),
];

/// `number` in `style`, as PDFium writes it; nothing for a number a style
/// can't write.
fn number_in(style: Option<PageLabelStyle>, number: i64) -> String {
    match style {
        None => String::new(),
        Some(PageLabelStyle::Decimal) => number.to_string(),
        Some(PageLabelStyle::UpperRoman) => roman(number).to_uppercase(),
        Some(PageLabelStyle::LowerRoman) => roman(number),
        Some(PageLabelStyle::UpperLetters) => letters(number).to_uppercase(),
        Some(PageLabelStyle::LowerLetters) => letters(number),
    }
}

/// Below a million; nothing for 0 and below.
fn roman(number: i64) -> String {
    // JavaScript's `%` keeps the sign, as Rust's does.
    let mut left = number % 1_000_000;
    let mut out = String::new();
    for (value, digits) in ROMAN {
        while left >= value {
            out.push_str(digits);
            left -= value;
        }
    }
    out
}

/// 1 → a, 26 → z, 27 → aa, 52 → zz, 53 → aaa; nothing for 0 and below.
fn letters(number: i64) -> String {
    if number <= 0 {
        return String::new();
    }
    let from = number - 1;
    let letter = char::from(b'a' + (from % 26) as u8);
    std::iter::repeat_n(letter, ((from / 26 + 1) % 1000) as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(
        first_page: u32,
        style: Option<PageLabelStyle>,
        first_number: i32,
    ) -> PageLabelSection {
        PageLabelSection { first_page, style, first_number, prefix: String::new() }
    }

    #[test]
    fn a_label_is_the_prefix_and_the_number_in_the_style() {
        let sections = vec![
            section(0, Some(PageLabelStyle::LowerRoman), 1),
            PageLabelSection {
                prefix: "A-".into(),
                ..section(2, Some(PageLabelStyle::Decimal), 7)
            },
            section(4, None, 1),
        ];
        let labels: Vec<_> = (0..5).map(|index| label_at(&sections, index)).collect();
        assert_eq!(
            labels,
            [Some("i".into()), Some("ii".into()), Some("A-7".into()), Some("A-8".into()), None]
        );
        assert_eq!(label_at(&[], 0), None);
    }

    #[test]
    fn numbers_are_written_as_pdfium_writes_them() {
        assert_eq!(roman(1994), "mcmxciv");
        assert_eq!(roman(0), "");
        assert_eq!(roman(-3), "");
        assert_eq!(roman(1_000_001), "i");
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa");
        assert_eq!(letters(52), "zz");
        assert_eq!(letters(53), "aaa");
        assert_eq!(letters(0), "");
        assert_eq!(number_in(Some(PageLabelStyle::Decimal), -2), "-2");
        assert_eq!(number_in(Some(PageLabelStyle::UpperLetters), 28), "BB");
    }

    #[test]
    fn a_delete_moves_a_section_to_its_next_page_and_drops_an_empty_one() {
        let sections = vec![section(0, None, 1), section(2, None, 1), section(4, None, 1)];
        // Pages 2 and 3 go: the middle section has none left.
        assert_eq!(
            sections_after_delete(&sections, 6, &[2, 3]),
            vec![section(0, None, 1), section(2, None, 1)]
        );
        // Page 2 goes: the middle section starts on its next page, now 2.
        assert_eq!(
            sections_after_delete(&sections, 6, &[2]),
            vec![section(0, None, 1), section(2, None, 1), section(3, None, 1)]
        );
        // Page 0 goes: the first section still starts at 0.
        assert_eq!(
            sections_after_delete(&sections, 6, &[0]),
            vec![section(0, None, 1), section(1, None, 1), section(3, None, 1)]
        );
    }

    #[test]
    fn an_insert_moves_the_sections_at_or_after_it_but_never_the_first() {
        let sections = vec![section(0, None, 1), section(2, None, 1)];
        assert_eq!(
            sections_after_insert(&sections, 0, 3),
            vec![section(0, None, 1), section(5, None, 1)]
        );
        assert_eq!(
            sections_after_insert(&sections, 2, 1),
            vec![section(0, None, 1), section(3, None, 1)]
        );
        assert_eq!(sections_after_insert(&sections, 3, 1), sections);
    }

    #[test]
    fn a_move_is_a_delete_then_an_insert() {
        let sections = vec![section(0, None, 1), section(2, None, 1)];
        // Page 0 moves to the end of four pages.
        assert_eq!(
            sections_after_move(&sections, 4, &[0], 3),
            vec![section(0, None, 1), section(1, None, 1)]
        );
    }
}
