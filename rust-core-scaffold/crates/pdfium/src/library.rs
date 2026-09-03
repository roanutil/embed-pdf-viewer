use std::cell::Cell;

use crate::PdfiumError;

thread_local! {
    /// PDFium's globals are per-thread in every non-wasm fork build, because
    /// runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77 sets
    /// embedpdf_thread_local_globals=true for every target except wasm32. So
    /// this refcount is per-thread too: FPDF_InitLibrary has to run on each
    /// thread that touches PDFium, and FPDF_DestroyLibrary on that same thread.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// A per-thread init guard. Not `Send`: dropping it on another thread would
/// call FPDF_DestroyLibrary against globals that thread never initialized.
pub struct Library {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Library {
    pub fn acquire() -> Result<Self, PdfiumError> {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                unsafe { pdfium_sys::FPDF_InitLibrary() };
            }
            depth.set(depth.get() + 1);
        });
        Ok(Self { _not_send: std::marker::PhantomData })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let next = depth.get() - 1;
            depth.set(next);
            if next == 0 {
                unsafe { pdfium_sys::FPDF_DestroyLibrary() };
            }
        });
    }
}
