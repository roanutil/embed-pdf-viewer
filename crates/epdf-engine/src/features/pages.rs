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
//!
//! The listing is read in the file's coordinates ([`PdfCoordinates`]); the
//! wire answers it in page space through `crate::page_space::pages`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use pdfium::{Document, PageActionEvent, PageBoxType};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::reader::{ActionTree, read_action_model};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::geom::{
    PageBoxes, PdfRect, PdfRotation, PdfSize, WrittenPageBoxes, normalize_pdf_rect, page_boxes_of,
    page_rotation_of, pdf_rect_size,
};
use crate::identity::PageObjectNumber;
use crate::page_space::{Coordinates, PdfCoordinates};

/// The order a page's annotations take keyboard focus in (`/Tabs`,
/// `PageTabOrder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageTabOrder {
    Rows,
    Columns,
    Structure,
    /// PDF 2.0: in the order the page lists them.
    Annotations,
    /// PDF 2.0: its form fields first, in that order.
    Widgets,
}

impl PageTabOrder {
    /// A `/Tabs` name as the file writes it; `None` for one PDF doesn't
    /// define.
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "R" => Self::Rows,
            "C" => Self::Columns,
            "S" => Self::Structure,
            "A" => Self::Annotations,
            "W" => Self::Widgets,
            _ => return None,
        })
    }
}

/// One page, as `pages.list` reports it (`PageLayout<C>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct PageLayout<C: Coordinates = PdfCoordinates> {
    /// Zero-based position today. Display metadata: an index is not identity.
    pub index: u32,
    /// The durable key, `ref` on the wire.
    #[serde(rename = "ref", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    /// `/PageLabel`, or absent. An empty label reads as absent.
    pub label: Option<String>,
    /// The visible box's size (`boxes.crop`), never swapped for rotation.
    pub size: PdfSize,
    pub rotation: PdfRotation,
    /// `/UserUnit`, defaulting to 1.
    pub user_unit: f32,
    /// The order its annotations take keyboard focus in; `None` when the
    /// page doesn't say, or says something PDF doesn't define.
    pub tab_order: Option<PageTabOrder>,
    pub boxes: PageBoxes<C::Rect>,
    /// Where the page sits in the file: its visible box (`boxes.crop`) in
    /// PDF space, in either coordinates. The only PDF-space value the wire
    /// hands out.
    pub pdf_crop_box: PdfRect,
    /// Open and close actions, absent when the page has neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<PageActions<C::Destination>>,
}

/// What a page asks a viewer to do when it opens or closes
/// (`PdfPageActions`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "D: Deserialize<'de>"))]
pub struct PageActions<D = crate::features::actions::destination::PdfDestination> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<ActionTree<D>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<ActionTree<D>>,
}

impl<D> PageActions<D> {
    /// The actions with every destination in them converted
    /// (`mapPageActions`).
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<PageActions<E>, Err> {
        Ok(PageActions {
            open: self.open.map(|tree| tree.try_map_destinations(&mut *convert)).transpose()?,
            close: self.close.map(|tree| tree.try_map_destinations(&mut *convert)).transpose()?,
        })
    }
}

/// The whole listing (`PageListSnapshot<C>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct PageListSnapshot<C: Coordinates = PdfCoordinates> {
    pub page_count: u32,
    pub pages: Vec<PageLayout<C>>,
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

        let boxes = read_boxes(document, index);
        pages.push(PageLayout {
            index,
            page_object_number: number,
            label: document.page_label_by_index(index).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("reading the label of page {index} failed: {e}"),
                )
            })?,
            size: pdf_rect_size(boxes.crop),
            rotation: rotation_at(document, index),
            user_unit: document.page_user_unit_by_index(index),
            tab_order: tab_order_of(document, index)?,
            boxes,
            pdf_crop_box: boxes.crop,
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

/// The turn of the page at `index` as it reads: its own `/Rotate`, or the
/// one it inherits, by the rules of `pageRotationOf` (`rotationAt`). A page
/// that cannot be read is upright.
pub fn rotation_at(document: &Document, index: u32) -> PdfRotation {
    document.page_rotate_by_index(index).map_or(PdfRotation::None, page_rotation_of)
}

/// The page's `/Tabs`; `None` when it sets none, or one PDF doesn't define.
fn tab_order_of(document: &Document, index: u32) -> Result<Option<PageTabOrder>, EngineError> {
    let name = document.page_tab_order_by_index(index).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the tab order of page {index} failed: {e}"),
        )
    })?;
    Ok(name.as_deref().and_then(PageTabOrder::from_name))
}

/// The page's five boxes as ISO 32000 defines them; `crop` is the visible
/// page (`readBoxes`). Each box is read as the file writes it and
/// normalized, then [`page_boxes_of`] applies the defaults.
pub fn read_boxes(document: &Document, index: u32) -> PageBoxes {
    let read = |which: PageBoxType| -> Option<PdfRect> {
        document.written_page_box_by_index(index, which).map(|r| {
            normalize_pdf_rect(PdfRect {
                left: f64::from(r.left),
                bottom: f64::from(r.bottom),
                right: f64::from(r.right),
                top: f64::from(r.top),
            })
        })
    };
    page_boxes_of(WrittenPageBoxes {
        media: read(PageBoxType::Media),
        crop: read(PageBoxType::Crop),
        bleed: read(PageBoxType::Bleed),
        trim: read(PageBoxType::Trim),
        art: read(PageBoxType::Art),
    })
}

/// Each page's visible box, read the first time it is asked for: what a
/// result's destinations are measured on (`visibleBoxReader`). A page that
/// isn't in the document has none, which is `NotFound`.
pub fn visible_box_reader(
    document: &Document,
) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + '_ {
    let mut boxes: HashMap<u32, PdfRect> = HashMap::new();
    let mut indexes: Option<HashMap<u32, u32>> = None;
    move |page| {
        if let Some(known) = boxes.get(&page.get()) {
            return Ok(*known);
        }
        let indexes = indexes.get_or_insert_with(|| {
            (0..document.page_count())
                .map(|index| (document.page_object_number_by_index(index), index))
                .collect()
        });
        let index = *indexes.get(&page.get()).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no page with object number {}", page.get()),
            )
        })?;
        let visible = read_boxes(document, index).crop;
        boxes.insert(page.get(), visible);
        Ok(visible)
    }
}
