//! `contract.ts`: the document's page structure, edited: rotate, reorder,
//! delete, insert, duplicate and extract, and which pages are selected. A
//! page argument is a `PageRef` or an index, the page at that index when the
//! verb is called; refs are what to keep, because an index shifts the moment
//! a page before it moves or goes. A position names its page by ref. Page
//! changes are announced by the kernel's `pagesChanged`; this plugin adds no
//! events of its own.

use std::sync::LazyLock;

use epdf_geometry::js::number_to_string;
use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::controller::PageEditCapability;

/// The package token.
pub static PAGE_EDIT: LazyLock<Token<PageEditCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "page-edit",
        "add pageEditPlugin() from '@embedpdf/plugin-page-edit' to your plugins list",
    )
});

/// `PdfSize`, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

/// A page argument, `PageRef | number`: a ref, or a zero-based index into
/// the document's page list at the moment the verb is called.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageArg {
    Ref(PageRef),
    /// A ref whose `objectNumber` no PDF object can have (zero, negative,
    /// fractional, non-finite, past `u32`). It names no page, so a verb
    /// refuses it `not-found` with the number as the TypeScript prints it. A
    /// typed Rust caller never builds one; JavaScript callers can.
    ForeignRef(f64),
    Index(f64),
}

impl PageArg {
    /// A canonical JSON page argument: a number is an index, anything else
    /// is read as a ref.
    pub fn from_json(value: &Value) -> PageArg {
        if let Some(index) = js_number(value) {
            return PageArg::Index(index);
        }
        match PageRef::from_json(value) {
            Some(page) => PageArg::Ref(page),
            None => PageArg::ForeignRef(
                value.get("objectNumber").and_then(js_number).unwrap_or(f64::NAN),
            ),
        }
    }

    /// What `pageOf` says when the document has no such page.
    pub(crate) fn not_found_message(self) -> String {
        match self {
            PageArg::Ref(page) => {
                format!("page {} is not in this document", page.page_object_number)
            }
            PageArg::ForeignRef(number) => {
                format!("page {} is not in this document", number_to_string(number))
            }
            PageArg::Index(index) => {
                format!("there is no page {} in this document", number_to_string(index))
            }
        }
    }
}

impl From<PageRef> for PageArg {
    fn from(page: PageRef) -> Self {
        PageArg::Ref(page)
    }
}

impl From<usize> for PageArg {
    fn from(index: usize) -> Self {
        PageArg::Index(index as f64)
    }
}

/// A canonical JSON number: plain, or `{"$number": text}`, which the
/// TypeScript runner reads as `Number(text)` (`scenario.ts:589`).
pub(crate) fn js_number(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    Some(string_to_number(value.get("$number")?.as_str()?))
}

/// JavaScript's WhiteSpace and LineTerminator code points, which
/// `StringToNumber` trims (U+0085 is not one).
fn is_js_white_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// `Number(text)`: surrounding white space ignored, empty is 0, a signed
/// decimal literal or `Infinity`, or an unsigned `0x`/`0o`/`0b` integer;
/// anything else is NaN. The same rules as the kernel's private
/// `string_to_number` (`crates/epdf-kernel/src/mirror.rs:630`).
pub(crate) fn string_to_number(text: &str) -> f64 {
    let t = text.trim_matches(is_js_white_space);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = t.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |n, c| {
                n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
            });
        }
    }
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if unsigned == "Infinity" {
        return if t.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let decimal =
        unsigned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
            && unsigned.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.');
    if !decimal {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `PagePosition`: where pages go, the engine's. Right after or before a
/// page, or the start or the end. It names a neighbour, so it stays right
/// when pages move before the edit applies. The anchor is checked when the
/// verb is called, like the verb's own pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PagePosition {
    Start,
    End,
    After(PageArg),
    Before(PageArg),
}

/// A position with its anchor resolved to the page's ref: what an op carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ResolvedPosition {
    Start,
    End,
    After(PageRef),
    Before(PageRef),
}

impl ResolvedPosition {
    pub(crate) fn to_json(self) -> Value {
        match self {
            ResolvedPosition::Start => json!("start"),
            ResolvedPosition::End => json!("end"),
            ResolvedPosition::After(page) => json!({ "after": page.to_json() }),
            ResolvedPosition::Before(page) => json!({ "before": page.to_json() }),
        }
    }
}

/// `OperationOptions`: every verb that writes takes a signal. When it fires
/// the verb rejects `operation-cancelled`: before its change is made nothing
/// changes, after it the change still applies.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `PageInsertBlankOptions`.
#[derive(Clone, Default)]
pub struct InsertBlankOptions {
    /// Where the new pages go. Default `End`.
    pub position: Option<PagePosition>,
    /// The page size, in points. Default: the size of the page the new pages
    /// go next to.
    pub size: Option<PdfSize>,
    /// How many blank pages. Default 1. Any JavaScript number, sent as given.
    pub count: Option<f64>,
    pub signal: Option<AbortSignal>,
}

/// `PageInsertFromBytesOptions`.
#[derive(Clone, Default)]
pub struct InsertFromBytesOptions {
    /// Only the source PDF's pages at these indexes, in this order. Default:
    /// every page.
    pub page_indexes: Option<Vec<f64>>,
    /// Where the new pages go. Default `End`.
    pub position: Option<PagePosition>,
    pub signal: Option<AbortSignal>,
}

/// `PagePositionOptions`: `insert_from_document()` and `duplicate()`.
#[derive(Clone, Default)]
pub struct PagePositionOptions {
    pub position: Option<PagePosition>,
    pub signal: Option<AbortSignal>,
}

/// `PageEditInsertResult`: the new pages, in the order they were inserted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageEditInsertResult {
    pub pages: Vec<Value>,
}

impl PageEditInsertResult {
    pub fn to_json(&self) -> Value {
        json!({ "pages": self.pages })
    }

    /// The new pages as refs, leaving out an entry that is not one.
    pub fn page_refs(&self) -> Vec<PageRef> {
        self.pages.iter().filter_map(PageRef::from_json).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::string_to_number;

    #[test]
    fn a_number_marker_reads_as_javascript_number_does() {
        for (text, expected) in [
            ("1.5", 1.5),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("+Infinity", f64::INFINITY),
            (" 12 ", 12.0),
            ("", 0.0),
            ("0x1f", 31.0),
            ("1e3", 1000.0),
            (".5", 0.5),
        ] {
            assert_eq!(string_to_number(text), expected, "{text:?}");
        }
        let zero = string_to_number("-0");
        assert!(zero == 0.0 && zero.is_sign_negative());
        for text in ["NaN", "inf", "infinity", "1_0", "0x", "-0x1", "abc", "\u{85}1"] {
            assert!(string_to_number(text).is_nan(), "{text:?}");
        }
    }
}
