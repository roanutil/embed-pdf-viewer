//! Building an action and the destination it points at.
//!
//! G11's link writer. The read side is [`crate::action`]; this is the half
//! that makes a `/A` rather than walking one.
//!
//! # Two handles with different lifetimes
//!
//! An `FPDF_DEST` and an `FPDF_ACTION` built here are OWNED by the document
//! from the moment they are created: PDFium allocates them into the
//! document's object pool, and there is no close call and no way to free one
//! that never gets attached. So a destination built for an action that then
//! fails to attach is a small leak inside the document rather than a leak in
//! this process, and the engine's job is to not build one it will not use.
//!
//! The page a destination names is loaded only to reference its dictionary.
//! Destinations routinely point at pages nothing else in the mutation
//! touches, so it is loaded and closed around the one call rather than being
//! taken from whatever page pool the caller has.

use std::ffi::CString;

use crate::{Document, PdfiumError};

/// A `/Dest` array PDFium has built into the document.
///
/// A handle rather than a value: what it points at is the document's, and it
/// is passed to [`Action::goto`] and then forgotten.
#[derive(Debug, Clone, Copy)]
pub struct Destination {
    dest: pdfium_sys::FPDF_DEST,
}

/// An action PDFium has built into the document.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    action: pdfium_sys::FPDF_ACTION,
}

/// `PDFDEST_VIEW_*` from `public/fpdf_doc.h`, the shared vocabulary of
/// `FPDFDest_GetView` and `EPDFDest_CreateView`.
///
/// `XYZ` is absent deliberately: it has its own constructor, because its
/// three components are individually present-or-retain and a parameter array
/// has nowhere to put that distinction.
pub mod view {
    pub const FIT: u32 = 2;
    pub const FIT_H: u32 = 3;
    pub const FIT_V: u32 = 4;
    pub const FIT_R: u32 = 5;
    pub const FIT_B: u32 = 6;
    pub const FIT_BH: u32 = 7;
    pub const FIT_BV: u32 = 8;
}

impl Document {
    /// Build an `/XYZ` destination on the page with this object number.
    ///
    /// Each component is present or retains the current value, which is what
    /// the spec's null array entry means, so each arrives as an `Option`
    /// rather than as a sentinel a reader would have to know about.
    pub fn create_xyz_destination(
        &self,
        page_object_number: u32,
        left: Option<f32>,
        top: Option<f32>,
        zoom: Option<f32>,
    ) -> Result<Destination, PdfiumError> {
        self.with_destination_page(page_object_number, |page| {
            // SAFETY: the page handle is live for this call.
            unsafe {
                pdfium_sys::EPDFDest_CreateXYZ(
                    page,
                    i32::from(left.is_some()),
                    left.unwrap_or(0.0),
                    i32::from(top.is_some()),
                    top.unwrap_or(0.0),
                    i32::from(zoom.is_some()),
                    zoom.unwrap_or(0.0),
                )
            }
        })
    }

    /// Build one of the fit destinations.
    ///
    /// `params` is as long as the fit type takes and no longer. This layer
    /// passes the count through: which arities are legal is §12.3.2.2's
    /// business and the engine's.
    pub fn create_view_destination(
        &self,
        page_object_number: u32,
        view: u32,
        params: &[f32],
    ) -> Result<Destination, PdfiumError> {
        self.with_destination_page(page_object_number, |page| {
            let (ptr, count) = if params.is_empty() {
                (std::ptr::null(), 0)
            } else {
                (params.as_ptr(), params.len() as std::os::raw::c_ulong)
            };
            // SAFETY: the page handle is live and `params` outlives the call.
            unsafe {
                pdfium_sys::EPDFDest_CreateView(page, std::os::raw::c_ulong::from(view), ptr, count)
            }
        })
    }

    /// Load the destination's page, run the builder, close the page.
    fn with_destination_page(
        &self,
        page_object_number: u32,
        build: impl FnOnce(pdfium_sys::FPDF_PAGE) -> pdfium_sys::FPDF_DEST,
    ) -> Result<Destination, PdfiumError> {
        // SAFETY: the document handle is live.
        let page = unsafe {
            pdfium_sys::EPDFDoc_LoadPageByObjectNumber(self.as_raw(), page_object_number)
        };
        if page.is_null() {
            return Err(PdfiumError::PageLoadByObjectNumber(page_object_number));
        }
        let dest = build(page);
        // SAFETY: loaded just above and closed exactly once.
        unsafe { pdfium_sys::FPDF_ClosePage(page) };
        if dest.is_null() {
            return Err(PdfiumError::DestinationBuild);
        }
        Ok(Destination { dest })
    }

    /// Build a `/URI` action.
    pub fn create_uri_action(&self, uri: &str) -> Result<Action, PdfiumError> {
        let uri = CString::new(uri).map_err(|_| PdfiumError::InvalidArgument("uri"))?;
        // SAFETY: the handle is live and the string outlives the call.
        let action = unsafe { pdfium_sys::EPDFAction_CreateURI(self.as_raw(), uri.as_ptr()) };
        if action.is_null() {
            return Err(PdfiumError::ActionBuild);
        }
        Ok(Action { action })
    }

    /// Build a `/Named` action for one of the standard page verbs
    /// (`NextPage`, `PrevPage`, `FirstPage`, `LastPage`).
    pub fn create_named_action(&self, name: &str) -> Result<Action, PdfiumError> {
        let name = CString::new(name).map_err(|_| PdfiumError::InvalidArgument("name"))?;
        // SAFETY: the handle is live and the string outlives the call.
        let action = unsafe { pdfium_sys::EPDFAction_CreateNamed(self.as_raw(), name.as_ptr()) };
        if action.is_null() {
            return Err(PdfiumError::ActionBuild);
        }
        Ok(Action { action })
    }

    /// Build a `/GoTo` action around a destination.
    pub fn create_goto_action(&self, destination: Destination) -> Result<Action, PdfiumError> {
        // SAFETY: both handles belong to this document and are live.
        let action = unsafe { pdfium_sys::EPDFAction_CreateGoTo(self.as_raw(), destination.dest) };
        if action.is_null() {
            return Err(PdfiumError::ActionBuild);
        }
        Ok(Action { action })
    }
}

impl crate::Annotation<'_> {
    /// Attach an action as this annotation's `/A`, replacing any it had.
    pub fn set_action(&self, action: Action) -> bool {
        // SAFETY: both handles are live.
        unsafe { pdfium_sys::EPDFAnnot_SetAction(self.as_raw(), action.action) != 0 }
    }

    /// Remove `/A`.
    pub fn remove_action(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_RemoveAction(self.as_raw()) != 0 }
    }

    /// Remove a direct `/Dest`.
    ///
    /// Separate from [`Self::remove_action`] because the two entries are
    /// separate: a link can carry either, and removing one leaves the other
    /// as the live target.
    pub fn remove_destination(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_RemoveDest(self.as_raw()) != 0 }
    }
}
