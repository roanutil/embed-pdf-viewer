//! The document outline (`/Root /Outlines`, the bookmarks).
//!
//! Wraps the readers of `public/fpdf_doc.h` (`FPDFBookmark_*`) and the
//! fork's `public/epdf_outline.h`, which names items by object number and
//! writes them. The fork keeps the links and `/Count` right on every write;
//! which item goes where, and what a read may skip, is the engine's
//! (`epdf_engine::features::outline`).
//!
//! A parent is an item's object number, or [`OUTLINE_TOP`] for the outline
//! itself. An [`OutlineItem`] is a handle into the document as it is now:
//! take a new one after a write.

use std::os::raw::c_ulong;

use crate::action::{ActionModel, RawDestination, action_model_from_raw, read_utf16};
use crate::{Action, Document, PdfiumError};

/// The outline itself, as a parent: its children are the top-level items.
pub const OUTLINE_TOP: u32 = 0;

/// One item of the outline, read through `FPDFBookmark_*`.
#[derive(Debug, Clone, Copy)]
pub struct OutlineItem<'doc> {
    handle: pdfium_sys::FPDF_BOOKMARK,
    document: &'doc Document,
}

fn item(handle: pdfium_sys::FPDF_BOOKMARK, document: &Document) -> Option<OutlineItem<'_>> {
    (!handle.is_null()).then_some(OutlineItem { handle, document })
}

/// UTF-16LE with its terminator, as `FPDF_WIDESTRING` takes it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Document {
    /// The first top-level item (`FPDFBookmark_GetFirstChild` with no
    /// parent); `None` for a document without an outline or an empty one.
    pub fn outline_first(&self) -> Option<OutlineItem<'_>> {
        // SAFETY: the handle is live; a null bookmark names the outline.
        item(
            unsafe { pdfium_sys::FPDFBookmark_GetFirstChild(self.handle, std::ptr::null_mut()) },
            self,
        )
    }

    /// The item `object_number` (`EPDFOutline_GetItem`); `None` when it is
    /// not an item in the outline.
    pub fn outline_item(&self, object_number: u32) -> Option<OutlineItem<'_>> {
        // SAFETY: the handle is live.
        item(unsafe { pdfium_sys::EPDFOutline_GetItem(self.handle, object_number) }, self)
    }

    /// `EPDFOutline_CreateItem`: a new item titled `title`, with no target,
    /// closed, under `parent`, right after its child `after` (0: first), at
    /// `object_number` (0: the next free one). Answers its object number, or
    /// 0 when `parent` or `after` is not where it should be, the number is
    /// not free, or the fork refused.
    pub fn create_outline_item(
        &self,
        object_number: u32,
        parent: u32,
        after: u32,
        title: &str,
    ) -> u32 {
        let title = wide(title);
        // SAFETY: the handle is live and `title` outlives the call.
        unsafe {
            pdfium_sys::EPDFOutline_CreateItem(
                self.handle,
                object_number,
                parent,
                after,
                title.as_ptr(),
            )
        }
    }

    /// `EPDFOutline_MoveItem`: the item, with the items under it, under
    /// `parent` right after its child `after` (0: first).
    pub fn move_outline_item(&self, object_number: u32, parent: u32, after: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFOutline_MoveItem(self.handle, object_number, parent, after) != 0 }
    }

    /// `EPDFOutline_DeleteItem`: the item and the items under it.
    pub fn delete_outline_item(&self, object_number: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFOutline_DeleteItem(self.handle, object_number) != 0 }
    }

    /// `EPDFOutline_SetTitle`.
    pub fn set_outline_title(&self, object_number: u32, title: &str) -> bool {
        let title = wide(title);
        // SAFETY: the handle is live and `title` outlives the call.
        unsafe { pdfium_sys::EPDFOutline_SetTitle(self.handle, object_number, title.as_ptr()) != 0 }
    }

    /// `EPDFOutline_SetOpen`: the sign of its `/Count`. Nothing is written
    /// for an item without children, which has no open state.
    pub fn set_outline_open(&self, object_number: u32, open: bool) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFOutline_SetOpen(self.handle, object_number, i32::from(open)) != 0 }
    }

    /// `EPDFOutline_SetColor`: `/C` as DeviceRGB components from 0 to 1;
    /// `None` removes it.
    pub fn set_outline_color(&self, object_number: u32, color: Option<[f32; 3]>) -> bool {
        let [r, g, b] = color.unwrap_or_default();
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFOutline_SetColor(
                self.handle,
                object_number,
                i32::from(color.is_some()),
                r,
                g,
                b,
            ) != 0
        }
    }

    /// `EPDFOutline_SetStyle`: `/F` as given (bit 1 italic, bit 2 bold); 0
    /// removes it.
    pub fn set_outline_style(&self, object_number: u32, flags: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFOutline_SetStyle(self.handle, object_number, flags) != 0 }
    }

    /// `EPDFOutline_SetAction`: `/A` refers to `action` and `/Dest` goes;
    /// `None` removes both.
    pub fn set_outline_action(&self, object_number: u32, action: Option<Action>) -> bool {
        let action = action.map_or(std::ptr::null_mut(), Action::as_raw);
        // SAFETY: the handle is live; the action, when given, is this
        // document's.
        unsafe { pdfium_sys::EPDFOutline_SetAction(self.handle, object_number, action) != 0 }
    }
}

impl<'doc> OutlineItem<'doc> {
    /// The item's object number; 0 when it isn't an indirect object
    /// (`EPDFOutline_GetObjectNumber`).
    pub fn object_number(&self) -> u32 {
        // SAFETY: the bookmark is live for the borrow of its document.
        unsafe { pdfium_sys::EPDFOutline_GetObjectNumber(self.handle) }
    }

    /// Its first child.
    pub fn first_child(&self) -> Option<OutlineItem<'doc>> {
        // SAFETY: both handles are live.
        item(
            unsafe { pdfium_sys::FPDFBookmark_GetFirstChild(self.document.handle, self.handle) },
            self.document,
        )
    }

    /// The sibling after it; a loop ends the list (PDFium stops it).
    pub fn next_sibling(&self) -> Option<OutlineItem<'doc>> {
        // SAFETY: both handles are live.
        item(
            unsafe { pdfium_sys::FPDFBookmark_GetNextSibling(self.document.handle, self.handle) },
            self.document,
        )
    }

    /// `/Title`; `None` when it has none or it is empty.
    pub fn title(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16(|buffer, length: c_ulong| {
            // SAFETY: `buffer` is null or holds `length` bytes.
            unsafe { pdfium_sys::FPDFBookmark_GetTitle(self.handle, buffer.cast(), length) }
        })
    }

    /// `/Count`, signed: above 0 when open.
    pub fn count(&self) -> i32 {
        // SAFETY: the bookmark is live.
        unsafe { pdfium_sys::FPDFBookmark_GetCount(self.handle) }
    }

    /// `/F`.
    pub fn style(&self) -> i32 {
        // SAFETY: the bookmark is live.
        unsafe { pdfium_sys::FPDFBookmark_GetStyle(self.handle) }
    }

    /// `/C` as DeviceRGB components; `None` when it has none.
    pub fn color(&self) -> Option<[f32; 3]> {
        let (mut r, mut g, mut b) = (0f32, 0f32, 0f32);
        // SAFETY: the bookmark is live and the out pointers are locals.
        let ok = unsafe {
            pdfium_sys::FPDFBookmark_GetColor(self.handle, &raw mut r, &raw mut g, &raw mut b)
        };
        (ok != 0).then_some([r, g, b])
    }

    /// Whether it has an `/A`.
    pub fn has_action(&self) -> bool {
        // SAFETY: the bookmark is live.
        !unsafe { pdfium_sys::FPDFBookmark_GetAction(self.handle) }.is_null()
    }

    /// Its `/A` as a detached action model (`EPDFAction_LoadModel`); `None`
    /// without `/A`, or when the model could not be loaded.
    pub fn action_model(&self) -> Option<ActionModel<'doc>> {
        // SAFETY: the bookmark is live; the action it answers belongs to the
        // document the model borrows.
        let action = unsafe { pdfium_sys::FPDFBookmark_GetAction(self.handle) };
        if action.is_null() {
            return None;
        }
        // SAFETY: `action` is live; the model is closed on drop.
        action_model_from_raw(unsafe { pdfium_sys::EPDFAction_LoadModel(action) }, self.document)
    }

    /// Its `/Dest`, a named one resolved (`FPDFBookmark_GetDest`); `None`
    /// without one.
    pub fn destination(&self) -> Option<RawDestination<'doc>> {
        // SAFETY: both handles are live.
        let dest = unsafe { pdfium_sys::FPDFBookmark_GetDest(self.document.handle, self.handle) };
        RawDestination::from_raw(dest, self.document)
    }
}
