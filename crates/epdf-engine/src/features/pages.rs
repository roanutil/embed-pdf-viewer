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
//! Page open and close actions are read here too, through
//! [`crate::features::actions`], because `PageLayout` carries them and a
//! caller listing pages should not have to make a second call to find out
//! whether opening one runs a script.

use serde::{Deserialize, Serialize};

use pdfium::{Document, PageActionEvent, PageBoxType};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::reader::{ActionTree, read_action_model};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
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
    /// The durable key, `ref` on the wire.
    #[serde(rename = "ref", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    /// `/PageLabel`, or absent. An empty label reads as absent.
    pub label: Option<String>,
    /// The page's size with rotation normalized away.
    pub size: PdfSize,
    pub rotation: PdfRotation,
    /// `/UserUnit`, defaulting to 1.
    pub user_unit: f32,
    pub boxes: PageBoxes,
    /// Open and close actions, absent when the page has neither.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<PageActions>,
}

/// What a page asks a viewer to do when it opens or closes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageActions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<ActionTree>,
}

/// The whole listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageListSnapshot {
    pub page_count: u32,
    pub pages: Vec<PageLayout>,
    /// The catalog's `/Names /Pages` and `/Names /Templates` registrations,
    /// `/Pages` first, each in key order. Always present, so a consumer can
    /// tell "no registrations" from an engine that predates the field.
    #[serde(default)]
    pub named_pages: Vec<NamedPageEntry>,
}

/// One named-page registration (`NamedPage.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedPageEntry {
    /// The decoded key, never interpreted.
    pub name: String,
    pub target: NamedPageTarget,
}

/// What a registration resolves to (`NamedPageTarget`). A page target is
/// `{ kind: "page", page: PageRef }`, like every other page address.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NamedPageTarget {
    #[serde(rename_all = "camelCase")]
    Page {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: u32,
    },
    #[serde(rename_all = "camelCase")]
    Template {
        object_number: u32,
    },
    Dangling,
}

/// `readNamedPages` in `PagesReader.ts`.
pub fn read_named_pages(document: &Document) -> Result<Vec<NamedPageEntry>, EngineError> {
    use pdfium::named_pages::{NamedPageTarget as Raw, NamedPageTree};
    let mut out = Vec::new();
    for tree in [NamedPageTree::Pages, NamedPageTree::Templates] {
        let entries = document.named_pages(tree).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading the named-page registry failed: {e}"),
            )
        })?;
        for entry in entries {
            let target = match entry.target {
                Raw::Page(page_object_number) => NamedPageTarget::Page { page_object_number },
                Raw::Template(object_number) => NamedPageTarget::Template { object_number },
                Raw::Dangling => NamedPageTarget::Dangling,
            };
            out.push(NamedPageEntry { name: entry.name, target });
        }
    }
    Ok(out)
}

/// List every page.
pub fn list_pages(
    document: &Document,
    cancel: &CancelToken,
) -> Result<PageListSnapshot, EngineError> {
    cancel.check()?;
    let count = document.page_count();
    let mut pages = Vec::with_capacity(count as usize);
    // One budget across the whole listing, not one per page. PDFium's own
    // limits are per model, so a document with a script on every page could
    // otherwise build an unbounded snapshot out of individually reasonable
    // ones.
    let mut budget = ActionReadBudgetTracker::default();

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
            actions: actions_of(document, number.get(), &mut budget)?,
        });
    }

    Ok(PageListSnapshot {
        page_count: pages.len() as u32,
        pages,
        named_pages: read_named_pages(document)?,
    })
}

/// A page's action trees, or `None` when it has neither.
///
/// A page with an unreadable action model is an error rather than a page with
/// no actions: silently reporting none would tell a caller the document said
/// nothing when in fact it said something this build could not read.
fn actions_of(
    document: &Document,
    page_object_number: u32,
    budget: &mut ActionReadBudgetTracker,
) -> Result<Option<PageActions>, EngineError> {
    let mut read = |event: PageActionEvent| -> Result<Option<ActionTree>, EngineError> {
        match document.page_action_model(page_object_number, event) {
            Some(model) => read_action_model(&model, budget).map(Some),
            None => Ok(None),
        }
    };
    let open = read(PageActionEvent::Open)?;
    let close = read(PageActionEvent::Close)?;
    Ok((open.is_some() || close.is_some()).then_some(PageActions { open, close }))
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
