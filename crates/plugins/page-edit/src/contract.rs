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

/// A canonical JSON number: plain, or `{"$number": ...}` for the non-finite.
fn js_number(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    match value.get("$number")?.as_str()? {
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => Some(f64::NAN),
    }
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
