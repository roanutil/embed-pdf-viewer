//! Detached action models, and the destinations they point at.
//!
//! PDFium builds an action model out of a `/A` entry: a graph of nodes with a
//! root, `/Next` children, and per-node payloads. The model is detached, so
//! reading it does not hold the document's action dictionaries open, and it
//! is closed explicitly.
//!
//! Everything here is a thin accessor. Deciding what a node means, budgeting
//! the walk and degrading an unreadable payload are the engine's job, in
//! `epdf_engine::features::actions`.

use crate::{Document, PdfiumError};

/// A node's index within its model.
pub type ActionNodeId = u32;

/// An action model, closed on drop.
///
/// Borrows its document, because every payload accessor that resolves a
/// destination or a URI needs the document the action came from.
#[derive(Debug)]
pub struct ActionModel<'doc> {
    handle: pdfium_sys::EPDF_ACTION_MODEL,
    document: &'doc Document,
}

/// Which of a page's two action slots to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageActionEvent {
    Open = 0,
    Close = 1,
}

impl Document {
    /// The action model for one of a page's events, or `None` when the page
    /// has no action there.
    pub fn page_action_model(
        &self,
        page_object_number: u32,
        event: PageActionEvent,
    ) -> Option<ActionModel<'_>> {
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe {
            pdfium_sys::EPDFDoc_GetPageActionModel(self.handle, page_object_number, event as i32)
        };
        (!handle.is_null()).then_some(ActionModel { handle, document: self })
    }
}

/// Which catalog-level event to read.
///
/// `EPDF_DOC_ACTION_*`. `/OpenAction` is not in this list because it is a
/// separate entry with a separate accessor, and because it may be a
/// destination rather than an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentActionEvent {
    WillClose = 0,
    WillSave = 1,
    DidSave = 2,
    WillPrint = 3,
    DidPrint = 4,
}

impl Document {
    /// The catalog's `/OpenAction`, in its action form.
    ///
    /// `None` also covers the destination form, which is a different entry
    /// this build cannot read: `EPDFDoc_GetOpenActionDest` is absent from the
    /// pinned artifact's headers.
    pub fn open_action_model(&self) -> Option<ActionModel<'_>> {
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::EPDFDoc_GetOpenActionModel(self.handle) };
        (!handle.is_null()).then_some(ActionModel { handle, document: self })
    }

    /// One of the catalog's additional actions.
    pub fn document_action_model(&self, event: DocumentActionEvent) -> Option<ActionModel<'_>> {
        // SAFETY: as above.
        let handle =
            unsafe { pdfium_sys::EPDFDoc_GetAdditionalActionModel(self.handle, event as i32) };
        (!handle.is_null()).then_some(ActionModel { handle, document: self })
    }

    /// How many scripts the document's JavaScript name tree holds.
    pub fn name_tree_script_count(&self) -> i32 {
        unsafe { pdfium_sys::FPDFDoc_GetJavaScriptActionCount(self.handle) }
    }

    /// One name-tree script's key.
    ///
    /// The handle is opened and closed here, because the name is the only
    /// thing it is good for: the script itself comes through
    /// [`Document::name_tree_script_model`].
    pub fn name_tree_script_name(&self, index: i32) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the handle is live; a null result is the documented failure.
        let handle = unsafe { pdfium_sys::FPDFDoc_GetJavaScriptAction(self.handle, index) };
        if handle.is_null() {
            return Ok(None);
        }
        let name = read_utf16(|buf, len| unsafe {
            pdfium_sys::FPDFJavaScriptAction_GetName(handle, buf, len)
        });
        // SAFETY: opened above and closed exactly once, whatever the read did.
        unsafe { pdfium_sys::FPDFDoc_CloseJavaScriptAction(handle) };
        name
    }

    /// One name-tree script's action model.
    pub fn name_tree_script_model(&self, index: i32) -> Option<ActionModel<'_>> {
        // SAFETY: as above.
        let handle =
            unsafe { pdfium_sys::EPDFDoc_GetNamedJavaScriptActionModel(self.handle, index) };
        (!handle.is_null()).then_some(ActionModel { handle, document: self })
    }
}

impl ActionModel<'_> {
    /// How many nodes the model holds. The bound on any walk of it.
    pub fn node_count(&self) -> i32 {
        unsafe { pdfium_sys::EPDFAction_GetNodeCount(self.handle) }
    }

    /// The native warning bits.
    pub fn warning_flags(&self) -> u32 {
        unsafe { pdfium_sys::EPDFAction_GetWarningFlags(self.handle) }
    }

    /// Whether PDFium walked the whole thing.
    pub fn is_complete(&self) -> bool {
        unsafe { pdfium_sys::EPDFAction_IsComplete(self.handle) != 0 }
    }

    /// The root node's id, as PDFium reports it. The caller validates.
    pub fn root_node_raw(&self) -> i64 {
        i64::from(unsafe { pdfium_sys::EPDFAction_GetRootNode(self.handle) })
    }

    pub fn node_type(&self, node: ActionNodeId) -> i32 {
        unsafe { pdfium_sys::EPDFAction_GetNodeType(self.handle, node) }
    }

    pub fn next_count(&self, node: ActionNodeId) -> i32 {
        unsafe { pdfium_sys::EPDFAction_GetNextCount(self.handle, node) }
    }

    /// The id of one `/Next` child, as reported. The caller validates.
    pub fn next_at_raw(&self, node: ActionNodeId, index: i32) -> i64 {
        i64::from(unsafe { pdfium_sys::EPDFAction_GetNextAt(self.handle, node, index) })
    }

    /// The raw `/S` name, kept for unknown and future action types.
    pub fn node_subtype(&self, node: ActionNodeId) -> Result<Option<String>, PdfiumError> {
        // SAFETY: measure-then-fill with a null probe, as every string
        // accessor here does.
        read_utf8(|buf, len| unsafe {
            pdfium_sys::EPDFAction_GetNodeSubtype(self.handle, node, buf.cast(), len)
        })
    }

    pub fn node_javascript(&self, node: ActionNodeId) -> Result<Option<String>, PdfiumError> {
        if unsafe { pdfium_sys::EPDFAction_NodeHasJavaScript(self.handle, node) } == 0 {
            return Ok(None);
        }
        read_utf16(|buf, len| unsafe {
            pdfium_sys::EPDFAction_GetNodeJavaScript(self.handle, node, buf, len)
        })
        .map(|s| Some(s.unwrap_or_default()))
    }

    /// Whether the node carries a script at all, which `rendition` needs to
    /// distinguish from a node whose script is empty.
    pub fn node_has_javascript(&self, node: ActionNodeId) -> bool {
        unsafe { pdfium_sys::EPDFAction_NodeHasJavaScript(self.handle, node) != 0 }
    }

    pub fn node_uri(&self, node: ActionNodeId) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buf, len| unsafe {
            pdfium_sys::EPDFAction_GetNodeURI(self.document.handle, self.handle, node, buf, len)
        })
    }

    pub fn node_name(&self, node: ActionNodeId) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buf, len| unsafe {
            pdfium_sys::EPDFAction_GetNodeName(self.handle, node, buf, len)
        })
    }

    pub fn node_file_path(&self, node: ActionNodeId) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buf, len| unsafe {
            pdfium_sys::EPDFAction_GetNodeFilePath(self.handle, node, buf, len)
        })
    }

    /// The destination a `goto` node points at.
    pub fn node_destination(&self, node: ActionNodeId) -> Option<RawDestination<'_>> {
        // SAFETY: both handles are live for this borrow.
        let dest =
            unsafe { pdfium_sys::EPDFAction_GetNodeDest(self.document.handle, self.handle, node) };
        (!dest.is_null()).then_some(RawDestination { dest, document: self.document })
    }
}

impl Drop for ActionModel<'_> {
    fn drop(&mut self) {
        // SAFETY: loaded above and closed exactly once.
        unsafe { pdfium_sys::EPDFAction_CloseModel(self.handle) };
    }
}

/// A live `FPDF_DEST` and the document it belongs to.
///
/// Borrowed rather than owned: PDFium hands out a pointer into the document's
/// own structures, and there is nothing to close.
#[derive(Debug, Clone, Copy)]
pub struct RawDestination<'doc> {
    dest: pdfium_sys::FPDF_DEST,
    document: &'doc Document,
}

/// A destination's fit type and parameters, straight from PDFium.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DestinationView {
    /// `PDFDEST_VIEW_*`.
    pub code: i32,
    pub params: [f32; 4],
    pub param_count: usize,
}

/// The three `/XYZ` components, each present or retaining the current value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DestinationLocation {
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub zoom: Option<f32>,
}

impl RawDestination<'_> {
    /// The page this destination names, or zero when it names none.
    pub fn page_object_number(&self) -> u32 {
        unsafe { pdfium_sys::EPDFDest_GetPageObjectNumber(self.document.handle, self.dest) }
    }

    pub fn view(&self) -> DestinationView {
        // `count` is an unsigned long here, not an int: FPDFDest_GetView
        // reports how many parameters it wrote.
        let mut count: std::os::raw::c_ulong = 0;
        // Four is the most any fit type writes, which is `/FitR`.
        let mut params = [0f32; 4];
        // SAFETY: `params` is four floats and `count` is an initialized local.
        let code =
            unsafe { pdfium_sys::FPDFDest_GetView(self.dest, &mut count, params.as_mut_ptr()) };
        DestinationView { code: code as i32, params, param_count: (count as usize).min(4) }
    }

    pub fn location(&self) -> DestinationLocation {
        let (mut has_left, mut has_top, mut has_zoom) = (0i32, 0i32, 0i32);
        let (mut left, mut top, mut zoom) = (0f32, 0f32, 0f32);
        // SAFETY: six initialized locals, as the signature asks for.
        let ok = unsafe {
            pdfium_sys::FPDFDest_GetLocationInPage(
                self.dest,
                &mut has_left,
                &mut has_top,
                &mut has_zoom,
                &mut left,
                &mut top,
                &mut zoom,
            )
        };
        if ok == 0 {
            return DestinationLocation::default();
        }
        DestinationLocation {
            left: (has_left != 0).then_some(left),
            top: (has_top != 0).then_some(top),
            zoom: (has_zoom != 0).then_some(zoom),
        }
    }
}

/// Measure then fill, for a UTF-8 accessor.
///
/// The reported length counts the terminator, and a length of one or less is
/// an absent value rather than an empty one.
fn read_utf8<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut std::ffi::c_void, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 1 {
        return Ok(None);
    }
    let mut buffer: Vec<u8> = Vec::new();
    buffer.try_reserve_exact(bytes as usize).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(bytes as usize, 0u8);

    let written = call(buffer.as_mut_ptr().cast(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = (written as usize).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf8_lossy(&buffer[..produced]).into_owned()))
}

/// Measure then fill, for a UTF-16 accessor. The length is in bytes.
fn read_utf16<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 2 {
        return Ok(None);
    }
    let units = (bytes as usize).div_ceil(2);
    let mut buffer: Vec<u16> = Vec::new();
    buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(units, 0u16);

    let written = call(buffer.as_mut_ptr(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
}
