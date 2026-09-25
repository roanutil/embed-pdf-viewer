//! `types.ts`: structural page edits, addressed by durable page object
//! number (never display index), mirroring the engine's page service with
//! one addition the engine cannot have: the relative `rotate_by` gesture.

use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::{Deserialize, Serialize};

use crate::capability::PageEditCapability;

/// `PageObjectNumber`: a page's durable address.
pub type PageObjectNumber = u32;

/// `PdfSize`, in PDF points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

/// Where added pages land. The PON anchors stay correct across a
/// concurrent reorder; `Index` is an absolute position. `None` appends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PagePlacement {
    After(PageObjectNumber),
    Before(PageObjectNumber),
    Index(f64),
}

/// `addBlank(opts)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddBlankOptions {
    /// Defaults to the page the new pages sit beside.
    pub size: Option<PdfSize>,
    /// Defaults to 1 (the engine's default: the plugin forwards it absent).
    pub count: Option<f64>,
    pub placement: Option<PagePlacement>,
}

/// `insert(bytes, opts)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertOptions {
    pub placement: Option<PagePlacement>,
}

pub static PAGE_EDIT: LazyLock<Token<PageEditCapability>> =
    LazyLock::new(|| Token::new("page-edit"));
