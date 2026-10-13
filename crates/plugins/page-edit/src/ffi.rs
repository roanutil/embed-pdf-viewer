//! The page-edit plugin through uniffi. The plugin answers every verb with
//! the engine's JSON (`PageMoveResult`, `PageInsertResult`, ...), so every
//! record here is new, modelled on `packages/plugin/page-edit`'s contract
//! and `packages/engine/core/src/mutation/Page*Result.ts`. A result that
//! does not decode is an `invalid-state` error.
//!
//! The plugin's promises take no abort. A token cancelled before the job
//! runs skips the edit with `operation-cancelled`; once the job has run,
//! the edit is on its way to the engine and lands.

use std::sync::Arc;

use epdf_ffi::page::decode_pages;
use epdf_ffi::{CancelToken, KernelBound, KernelPost, PageInfo, PageRef, Size, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef as KernelPageRef;
use serde_json::Value;

use crate::contract::{InsertBlankOptions, InsertFromBytesOptions, PagePlacement, PdfSize};
use crate::controller::PageEditCapability;

/// `PagePlacement`: where pages land. The ref anchors stay correct across a
/// concurrent reorder; `Index` is an absolute position and `End` appends.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum PageEditPlacement {
    After { page: PageRef },
    Before { page: PageRef },
    Index { index: u32 },
    End,
}

impl From<PageEditPlacement> for PagePlacement {
    fn from(placement: PageEditPlacement) -> Self {
        match placement {
            PageEditPlacement::After { page } => PagePlacement::After(page.into()),
            PageEditPlacement::Before { page } => PagePlacement::Before(page.into()),
            PageEditPlacement::Index { index } => PagePlacement::Index(f64::from(index)),
            PageEditPlacement::End => PagePlacement::End,
        }
    }
}

/// `rotateBy`'s delta: a quarter turn either way, or a half turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PageEditTurn {
    Clockwise,
    CounterClockwise,
    Half,
}

impl PageEditTurn {
    fn degrees(self) -> f64 {
        match self {
            PageEditTurn::Clockwise => 90.0,
            PageEditTurn::CounterClockwise => -90.0,
            PageEditTurn::Half => 180.0,
        }
    }
}

/// `PageRotation`: an absolute `/Rotate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PageEditRotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl PageEditRotation {
    fn degrees(self) -> f64 {
        match self {
            PageEditRotation::None => 0.0,
            PageEditRotation::Clockwise90 => 90.0,
            PageEditRotation::Clockwise180 => 180.0,
            PageEditRotation::Clockwise270 => 270.0,
        }
    }
}

/// `insertBlank(options)`. `size` defaults to the page the new pages sit
/// beside, `count` to 1, `placement` to `End`.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageEditBlankOptions {
    #[uniffi(default = None)]
    pub count: Option<u32>,
    #[uniffi(default = None)]
    pub size: Option<Size>,
    #[uniffi(default = None)]
    pub placement: Option<PageEditPlacement>,
}

impl From<PageEditBlankOptions> for InsertBlankOptions {
    fn from(options: PageEditBlankOptions) -> Self {
        InsertBlankOptions {
            size: options.size.map(|s| PdfSize { width: s.width, height: s.height }),
            count: options.count.map(f64::from),
            placement: options.placement.map(Into::into),
        }
    }
}

/// `insertFromBytes(bytes, options)`: a subset of the PDF's pages by index,
/// every page when `page_indexes` is `None`.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageEditBytesOptions {
    #[uniffi(default = None)]
    pub page_indexes: Option<Vec<u32>>,
    #[uniffi(default = None)]
    pub placement: Option<PageEditPlacement>,
}

impl From<PageEditBytesOptions> for InsertFromBytesOptions {
    fn from(options: PageEditBytesOptions) -> Self {
        InsertFromBytesOptions {
            page_indexes: options.page_indexes.map(|i| i.into_iter().map(f64::from).collect()),
            placement: options.placement.map(Into::into),
        }
    }
}

/// `PageMoveResult`, `PageRotateResult` and `PageDeleteResult`: the new
/// layout, every page in display order. The engine's `cache` is `null` for
/// a local engine and is dropped.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditLayout {
    pub pages: Vec<PageInfo>,
}

/// `PageInsertResult`: the new pages in insertion order, and the new layout.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditInsertion {
    pub inserted: Vec<PageRef>,
    pub pages: Vec<PageInfo>,
}

fn invalid_state(what: String) -> Error {
    Error::named("InvalidState", format!("[page-edit] {what}")).with("code", "invalid-state")
}

fn layout(result: &Value) -> Result<Vec<PageInfo>, Error> {
    let pages = result
        .get("layout")
        .and_then(|l| l.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("the engine answered no layout: {result}")))?;
    decode_pages(pages)
}

fn decode_layout(result: Value) -> Result<PageEditLayout, ViewerError> {
    Ok(PageEditLayout { pages: layout(&result)? })
}

fn decode_insertion(result: Value) -> Result<PageEditInsertion, ViewerError> {
    let inserted = result
        .get("insertedPages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("an insert answered no insertedPages: {result}")))?
        .iter()
        .map(|r| {
            KernelPageRef::from_json(r)
                .map(PageRef::from)
                .ok_or_else(|| invalid_state(format!("an inserted page that is not a ref: {r}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PageEditInsertion { inserted, pages: layout(&result)? })
}

fn refs(pages: Vec<PageRef>) -> Vec<KernelPageRef> {
    pages.into_iter().map(Into::into).collect()
}

/// The capability, if its document is still open.
fn open(cap: &PageEditCapability) -> Result<&PageEditCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "page-edit",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct PageEditHandle {
    cap: KernelBound<PageEditCapability>,
}

impl PageEditHandle {
    /// On the kernel thread.
    pub fn new(cap: PageEditCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(PageEditHandle { cap: KernelBound::new(cap, post) })
    }
}

#[uniffi::export]
impl PageEditHandle {
    /// Whether this caller may make structural page edits
    /// (`doc.pages.assemble`). Every verb shares that one grant.
    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| open(c)?.can_edit()).await
    }

    /// Turn each page from its own rotation. Cancelling skips an edit that
    /// has not started.
    pub async fn rotate_by(
        &self,
        pages: Vec<PageRef>,
        turn: PageEditTurn,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.rotate_by(refs(pages), turn.degrees())),
                decode_layout,
            )
            .await?
    }

    /// One absolute rotation for every page given.
    pub async fn set_rotation(
        &self,
        pages: Vec<PageRef>,
        rotation: PageEditRotation,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_rotation(refs(pages), rotation.degrees())),
                decode_layout,
            )
            .await?
    }

    /// Reorder pages as a contiguous block at `placement` (`move`).
    pub async fn move_pages(
        &self,
        pages: Vec<PageRef>,
        placement: PageEditPlacement,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.move_pages(refs(pages), Some(placement.into()))),
                decode_layout,
            )
            .await?
    }

    /// Delete pages (`delete`). The engine refuses deleting every page.
    pub async fn delete_pages(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete(refs(pages))),
                decode_layout,
            )
            .await?
    }

    pub async fn insert_blank(
        &self,
        options: PageEditBlankOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.insert_blank(options.into())),
                decode_insertion,
            )
            .await?
    }

    /// Pages of another PDF, given as its bytes.
    pub async fn insert_from_bytes(
        &self,
        bytes: Vec<u8>,
        options: PageEditBytesOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.insert_from_bytes(Bytes(bytes), options.into())),
                decode_insertion,
            )
            .await?
    }

    /// Pages of another open document, by its id.
    pub async fn insert_from_document(
        &self,
        document_id: String,
        pages: Vec<PageRef>,
        placement: Option<PageEditPlacement>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.insert_from_document(
                        document_id,
                        refs(pages),
                        placement.map(Into::into),
                    ))
                },
                decode_insertion,
            )
            .await?
    }

    /// Copies of pages; by default right after the last of them.
    pub async fn duplicate(
        &self,
        pages: Vec<PageRef>,
        placement: Option<PageEditPlacement>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.duplicate(refs(pages), placement.map(Into::into))),
                decode_insertion,
            )
            .await?
    }

    /// A new PDF from a page subset, as its bytes.
    pub async fn extract(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.extract(refs(pages))), |b| b.0)
            .await
    }
}
