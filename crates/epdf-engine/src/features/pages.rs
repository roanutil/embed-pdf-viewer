//! Listing a document's pages.
//!
//! G6. Replaces `services/src/features/pages/PagesReader.ts`. Operation:
//! `pages.list`.
//!
//! Nothing here loads a page. The `...ByIndex` family reads straight from the
//! page tree, which is what keeps listing a 500-page document cheap, and it is
//! the one place an index is the right address: the caller is enumerating, and
//! the object number is what it is producing rather than what it has.
//!
//! # Not here yet
//!
//! Page open and close actions. `PageLayout.actions` comes from
//! `ActionModelReader`, which is 522 lines of its own and belongs with
//! `actions.read`. The field is on the struct so adding it later does not
//! reshape the wire, and it is always absent until then; the conformance
//! runner asserts that the corpus has no page carrying one, so this stays a
//! known gap rather than a silent difference.

use serde::{Deserialize, Serialize};

use pdfium::{Document, PageBoxType};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfRect, PdfRotation, PdfSize, normalize_pdf_rect};
use crate::identity::PageObjectNumber;

/// A page's boxes. Media and crop are always present; the rest are optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBoxes {
    pub media: PdfRect,
    pub crop: PdfRect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bleed: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art: Option<PdfRect>,
}

/// One page, as `pages.list` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayout {
    /// Zero-based position today. Display metadata: an index is not identity.
    pub index: u32,
    /// The durable key.
    pub page_object_number: PageObjectNumber,
    /// `/PageLabel`, or absent. An empty label reads as absent.
    pub label: Option<String>,
    /// The page's size with rotation normalized away.
    pub size: PdfSize,
    pub rotation: PdfRotation,
    /// `/UserUnit`, defaulting to 1.
    pub user_unit: f32,
    pub boxes: PageBoxes,
}

/// The whole listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageListSnapshot {
    pub page_count: u32,
    pub pages: Vec<PageLayout>,
}

/// List every page.
pub fn list_pages(
    document: &Document,
    cancel: &CancelToken,
) -> Result<PageListSnapshot, EngineError> {
    cancel.check()?;
    let count = document.page_count();
    let mut pages = Vec::with_capacity(count as usize);

    for index in 0..count {
        cancel.check()?;
        let number =
            PageObjectNumber::new(document.page_object_number_by_index(index)).map_err(|_| {
                EngineError::with_message(
                    EngineErrorCode::MalformedPdf,
                    format!(
                        "page at index {index} is a direct (non-indirect) PDF object; \
                         the engine requires every page to have a stable indirect object number"
                    ),
                )
            })?;

        let (width, height) = document.page_size_by_index(index).unwrap_or((0.0, 0.0));
        pages.push(PageLayout {
            index,
            page_object_number: number,
            label: document.page_label_by_index(index).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("reading the label of page {index} failed: {e}"),
                )
            })?,
            size: PdfSize { width: f64::from(width), height: f64::from(height) },
            rotation: rotation_of(document, index),
            user_unit: document.page_user_unit_by_index(index),
            boxes: boxes_of(document, index),
        });
    }

    Ok(PageListSnapshot { page_count: pages.len() as u32, pages })
}

fn rotation_of(document: &Document, index: u32) -> PdfRotation {
    match document.page_rotation_quarter_turns(index) {
        1 => PdfRotation::Clockwise90,
        2 => PdfRotation::Clockwise180,
        3 => PdfRotation::Clockwise270,
        _ => PdfRotation::None,
    }
}

fn boxes_of(document: &Document, index: u32) -> PageBoxes {
    let read = |which: PageBoxType| -> Option<PdfRect> {
        document.page_box_by_index(index, which).map(|r| {
            // Normalized, so the lower-left and upper-right invariant holds
            // however the PDF ordered the corners. A producer is free to write
            // them backwards and plenty do.
            normalize_pdf_rect(PdfRect {
                left: f64::from(r.left),
                bottom: f64::from(r.bottom),
                right: f64::from(r.right),
                top: f64::from(r.top),
            })
        })
    };

    // Media resolves through page-tree inheritance and PDFium's default, so
    // the fallback is for a document broken enough that nothing resolved.
    let media = read(PageBoxType::Media).unwrap_or(PdfRect {
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
        top: 0.0,
    });
    let crop = read(PageBoxType::Crop).unwrap_or(media);
    PageBoxes {
        media,
        crop,
        bleed: read(PageBoxType::Bleed),
        trim: read(PageBoxType::Trim),
        art: read(PageBoxType::Art),
    }
}
