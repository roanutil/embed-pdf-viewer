use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    /// PDFium's globals are per-thread in every non-wasm fork build, because
    /// `runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77` sets
    /// `embedpdf_thread_local_globals=true` for every target except wasm32. So
    /// this refcount is per-thread too: FPDF_InitLibrary has to run on each
    /// thread that touches PDFium, and FPDF_DestroyLibrary on that same
    /// thread.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// A per-thread init guard.
///
/// Not `Send`: dropping it on another thread would call FPDF_DestroyLibrary
/// against globals that thread never initialized.
#[derive(Debug)]
pub struct Library {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Library {
    /// Initializes PDFium on this thread if it is not already, and returns a
    /// guard that un-initializes it when the last one on the thread drops.
    ///
    /// Returns `Rc<Self>` rather than a bare `Library` so a `Document` can
    /// hold a clone and keep the library alive for its own lifetime: PDFium
    /// handles created under one init are invalid after the matching destroy,
    /// and a `Document` has no other tie to the library's lifetime.
    ///
    /// Infallible on purpose. `FPDF_InitLibrary` returns `void` and reports
    /// nothing, so a `Result` here could only ever be `Ok`, and a dead error
    /// variant invites callers to write handling that never runs.
    pub fn acquire() -> Rc<Self> {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                // SAFETY: no PDFium handle exists on this thread while the
                // depth is zero, so there is nothing this call can invalidate.
                unsafe { pdfium_sys::FPDF_InitLibrary() };
            }
            depth.set(depth.get() + 1);
        });
        Rc::new(Self { _not_send: std::marker::PhantomData })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let next = depth.get() - 1;
            depth.set(next);
            if next == 0 {
                // SAFETY: every Document holds an Rc<Library>, so the depth
                // cannot reach zero while a document handle from this thread
                // is still open.
                unsafe { pdfium_sys::FPDF_DestroyLibrary() };
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_and_unwinds_on_one_thread() {
        let outer = Library::acquire();
        assert_eq!(DEPTH.with(|d| d.get()), 1);
        {
            let _inner = Library::acquire();
            assert_eq!(DEPTH.with(|d| d.get()), 2);
        }
        assert_eq!(DEPTH.with(|d| d.get()), 1);
        drop(outer);
        assert_eq!(DEPTH.with(|d| d.get()), 0);
    }
}
