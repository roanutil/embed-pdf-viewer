//! The page-edit plugin through uniffi, modelled on
//! `packages/plugin/page-edit`'s contract. Migrated by hand in Task 13.19
//! and compiled and tested since Task 13.33.
//!
//! The TypeScript verbs resolve nothing (an insert resolves its new pages):
//! the confirmed layout reaches the document's page list before the verb's
//! promise settles. The records here keep the layout they always answered,
//! read from that page list when the verb settles. A layout that does not
//! decode is an `invalid-state` error.
//!
//! A token cancelled before the job runs skips the edit with
//! `operation-cancelled`; once the job has run, the edit is on its way to the
//! engine and lands. The plugin's own `signal` option is not wired to the
//! token yet.

use std::sync::Arc;

use epdf_ffi::page::decode_pages;
use epdf_ffi::{CancelToken, KernelBound, KernelPost, PageInfo, PageRef, Size, ViewerError};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef as KernelPageRef;
use epdf_kernel::{Error, Promise};
use serde_json::{Value, json};

use crate::contract::{
    InsertBlankOptions, InsertFromBytesOptions, OperationOptions, PageArg, PageEditInsertResult,
    PagePosition, PagePositionOptions, PdfSize,
};
use crate::controller::PageEditCapability;

/// `PagePosition`: where pages go. A ref anchor stays right across a
/// concurrent reorder. The retired absolute `Index` is gone (Task 13.19).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum PageEditPlacement {
    After { page: PageRef },
    Before { page: PageRef },
    Start,
    End,
}

impl From<PageEditPlacement> for PagePosition {
    fn from(placement: PageEditPlacement) -> Self {
        match placement {
            PageEditPlacement::After { page } => PagePosition::After(PageArg::Ref(page.into())),
            PageEditPlacement::Before { page } => PagePosition::Before(PageArg::Ref(page.into())),
            PageEditPlacement::Start => PagePosition::Start,
            PageEditPlacement::End => PagePosition::End,
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

/// `PdfRotation`: an absolute `/Rotate`.
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

/// `insertBlank(options)`. `size` defaults to the page the new pages go next
/// to, `count` to 1, `placement` to `End`.
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
            position: options.placement.map(Into::into),
            signal: None,
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
            position: options.placement.map(Into::into),
            signal: None,
        }
    }
}

fn position_options(placement: Option<PageEditPlacement>) -> PagePositionOptions {
    PagePositionOptions { position: placement.map(Into::into), signal: None }
}

/// The document's layout once an edit applied: every page in display order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditLayout {
    pub pages: Vec<PageInfo>,
}

/// An insert: the new pages in insertion order, and the layout after it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditInsertion {
    pub inserted: Vec<PageRef>,
    pub pages: Vec<PageInfo>,
}

fn invalid_state(what: String) -> Error {
    Error::named("InvalidState", format!("[page-edit] {what}")).with("code", "invalid-state")
}

fn layout(answer: &Value) -> Result<Vec<PageInfo>, Error> {
    let pages = answer
        .get("layout")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("no page list to answer: {answer}")))?;
    decode_pages(pages)
}

fn decode_layout(answer: Value) -> Result<PageEditLayout, ViewerError> {
    Ok(PageEditLayout { pages: layout(&answer)? })
}

fn decode_insertion(answer: Value) -> Result<PageEditInsertion, ViewerError> {
    let inserted = answer
        .get("inserted")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("an insert answered no pages: {answer}")))?
        .iter()
        .map(|r| {
            KernelPageRef::from_json(r)
                .map(PageRef::from)
                .ok_or_else(|| invalid_state(format!("an inserted page that is not a ref: {r}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PageEditInsertion { inserted, pages: layout(&answer)? })
}

/// A verb's promise, settled with the page list it leaves.
fn with_layout(cap: &PageEditCapability, verb: Promise<()>) -> Promise<Value> {
    let cap = cap.clone();
    let (answer, resolver) = Promise::pending();
    verb.on_settle(move |result| {
        resolver.settle(result.clone().map(|()| json!({ "layout": *cap.registry() })))
    });
    answer
}

/// An insert's promise, settled with its pages and the page list it leaves.
fn with_insertion(cap: &PageEditCapability, verb: Promise<PageEditInsertResult>) -> Promise<Value> {
    let cap = cap.clone();
    let (answer, resolver) = Promise::pending();
    verb.on_settle(move |result| {
        resolver.settle(
            result.clone().map(|r| json!({ "inserted": r.pages, "layout": *cap.registry() })),
        )
    });
    answer
}

fn refs(pages: Vec<PageRef>) -> Vec<PageArg> {
    pages.into_iter().map(|page| PageArg::Ref(page.into())).collect()
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
    /// Whether rotating, reordering, deleting and inserting pages is allowed
    /// (`doc.pages.assemble`).
    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_edit())).await
    }

    /// Whether copying pages out is allowed (`doc.download`): `extract`,
    /// `duplicate` and `insert_from_document` need it.
    pub async fn can_extract(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_extract())).await
    }

    /// The selected pages, in the order they were selected.
    pub async fn get_selected(&self) -> Result<Vec<PageRef>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.get_selected().iter().copied().map(PageRef::from).collect()))
            .await
    }

    /// Make these pages the selection; an empty list clears it.
    pub async fn select(&self, pages: Vec<PageRef>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.select(&refs(pages));
                Ok(())
            })
            .await
    }

    /// Turn each page from its own rotation, the one it has once the edits
    /// on their way apply. Cancelling skips an edit that has not started.
    pub async fn rotate_by(
        &self,
        pages: Vec<PageRef>,
        turn: PageEditTurn,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let c = open(c)?;
                    Ok(with_layout(
                        c,
                        c.rotate_by(refs(pages), turn.degrees(), OperationOptions::default()),
                    ))
                },
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
                move |c| {
                    let c = open(c)?;
                    Ok(with_layout(
                        c,
                        c.set_rotation(
                            refs(pages),
                            rotation.degrees(),
                            OperationOptions::default(),
                        ),
                    ))
                },
                decode_layout,
            )
            .await?
    }

    /// Move pages to `placement` (`reorder`); several pages keep their
    /// order and land together. `End` is the engine's own `'end'`.
    pub async fn move_pages(
        &self,
        pages: Vec<PageRef>,
        placement: PageEditPlacement,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let c = open(c)?;
                    Ok(with_layout(
                        c,
                        c.reorder(refs(pages), placement.into(), OperationOptions::default()),
                    ))
                },
                decode_layout,
            )
            .await?
    }

    /// Delete pages (`delete`). Deleting every page is `invalid-input`.
    pub async fn delete_pages(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let c = open(c)?;
                    Ok(with_layout(c, c.delete(refs(pages), OperationOptions::default())))
                },
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
                move |c| {
                    let c = open(c)?;
                    Ok(with_insertion(c, c.insert_blank(options.into())))
                },
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
                move |c| {
                    let c = open(c)?;
                    Ok(with_insertion(c, c.insert_from_bytes(Bytes(bytes), options.into())))
                },
                decode_insertion,
            )
            .await?
    }

    /// Copies of pages of another open document, by its id.
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
                    let c = open(c)?;
                    let inserted = c.insert_from_document(
                        document_id,
                        refs(pages),
                        position_options(placement),
                    );
                    Ok(with_insertion(c, inserted))
                },
                decode_insertion,
            )
            .await?
    }

    /// Copies of pages as they show; by default right after the last of them.
    pub async fn duplicate(
        &self,
        pages: Vec<PageRef>,
        placement: Option<PageEditPlacement>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let c = open(c)?;
                    Ok(with_insertion(c, c.duplicate(refs(pages), position_options(placement))))
                },
                decode_insertion,
            )
            .await?
    }

    /// A new PDF of pages as they show, as its bytes.
    pub async fn extract(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.extract(refs(pages), OperationOptions::default())),
                |b| b.0,
            )
            .await
    }
}
