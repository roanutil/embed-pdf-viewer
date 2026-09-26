//! `contract.ts`: structural page operations, addressed by durable page refs
//! (never display index, which shifts the moment a sibling moves). Mirrors
//! the engine's page service, plus the relative rotate gesture and the
//! placements the thumbnail gestures speak.

use std::sync::LazyLock;

use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};

use crate::controller::PageEditCapability;

/// `PdfSize`, in PDF points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

/// `PagePlacement`: where pages land. The ref anchors stay correct across a
/// concurrent reorder between click and call; `Index` is an absolute
/// position (any JavaScript number, carried as written) and `End` appends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PagePlacement {
    After(PageRef),
    Before(PageRef),
    Index(f64),
    End,
}

/// `insertBlank(options)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertBlankOptions {
    /// Defaults to the page the new pages sit beside.
    pub size: Option<PdfSize>,
    /// Defaults to 1 (the engine's default: the plugin forwards it absent).
    pub count: Option<f64>,
    /// Defaults to `End`.
    pub placement: Option<PagePlacement>,
}

/// `insertFromBytes(bytes, options)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertFromBytesOptions {
    /// A subset of the PDF's pages, by index; every page when absent.
    pub page_indexes: Option<Vec<f64>>,
    pub placement: Option<PagePlacement>,
}

pub static PAGE_EDIT: LazyLock<Token<PageEditCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "page-edit",
        "add pageEditPlugin() from '@embedpdf/plugin-page-edit' to your plugins list",
    )
});
