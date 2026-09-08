use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dispatch::Worker;
use crate::{Bitmap, OpsError, Rect, SearchHit, Size};

/// Upper bound on a rendered bitmap's byte size (`stride * height`), checked
/// before `render_bgra` allocates. `scale` reaches `render_page` straight
/// from every host -- `epdf_render` in crates/epdf-wasm/src/lib.rs is
/// callable from a browser page, and crates/epdf-node/src/lib.rs takes any
/// JavaScript number -- and an allocation that fails aborts the process
/// instead of unwinding, which none of the five bindings can recover from.
///
/// 256 MiB is generous relative to every render this scaffold actually
/// produces: the frozen golden is 121,176 bytes at scale 0.25, and
/// `apps/ios` renders a 7.7 MB bitmap at scale 2.0, both for a 612x792-point
/// page. Bytes grow with the square of scale, so 256 MiB leaves room to
/// render that same page at just under scale 12 -- about 6x what apps/ios
/// asks for today -- while still refusing a multi-gigabyte allocation
/// outright before it reaches the allocator.
const MAX_RENDER_BYTES: u64 = 256 * 1024 * 1024;

pub struct Engine {
    worker: Arc<Worker>,
}

impl Engine {
    /// Spawns the PDFium thread eagerly, so a failure to initialize surfaces
    /// here rather than on the first `open`. Fallible on purpose: uniffi
    /// constructors may return `Result`, and Task 6 exports it that way.
    ///
    /// Each call spawns its own OS thread and calls `FPDF_InitLibrary` on it,
    /// so an `Engine` is meant to be constructed once and held for as long as
    /// documents need opening, not created per document. uniffi exports this
    /// as a plain constructor to Swift and Kotlin, where a per-document
    /// `Engine` is the obvious mistake to make.
    pub fn new() -> Result<Arc<Self>, OpsError> {
        Ok(Arc::new(Self { worker: Arc::new(Worker::spawn()?) }))
    }

    /// Test-only probe: unwind while dispatch holds its state borrow.
    #[cfg(feature = "panic-probe")]
    pub fn test_panic(&self) -> Result<(), OpsError> {
        self.worker.run(|state| {
            let _library = state.library();
            panic!("deliberate panic inside PDFium dispatch");
        })
    }

    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<Document>, OpsError> {
        let worker = Arc::clone(&self.worker);
        let id = worker.run(move |state| {
            // The immutable borrow of the library ends before `insert` takes a
            // mutable one; `from_bytes` does not retain the reference.
            let doc = pdfium::Document::from_bytes(state.library(), &bytes, password.as_deref())?;
            Ok(state.insert(doc))
        })?;
        Ok(Arc::new(Document { worker, id, closed: AtomicBool::new(false) }))
    }
}

pub struct Document {
    worker: Arc<Worker>,
    id: u64,
    closed: AtomicBool,
}

impl Document {
    /// A closure passed to `Worker::run` (or `Worker::send`) must never
    /// capture or drop an `epdf_ops::Document`: doing so re-enters the
    /// worker from inside a job running on it, which deadlocks on the
    /// `threaded` dispatch and is undefined behaviour on `inline`.
    fn with<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&pdfium::Document) -> Result<T, OpsError> + Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(OpsError::Closed);
        }
        let id = self.id;
        self.worker.run(move |state| f(state.get(id)?))
    }

    pub fn page_count(&self) -> Result<u32, OpsError> {
        self.with(|doc| Ok(doc.page_count()))
    }

    pub fn page_size(&self, index: u32) -> Result<Size, OpsError> {
        self.with(move |doc| {
            let page = doc.page(index)?;
            Ok(Size { width: page.width(), height: page.height() })
        })
    }

    pub fn render_page(&self, index: u32, scale: f32) -> Result<Bitmap, OpsError> {
        self.with(move |doc| {
            // A non-finite or non-positive scale, or one whose buffer would
            // blow past MAX_RENDER_BYTES, reaches here straight from every
            // host's own numeric type (an untrusted f64/Double/Float), and
            // `render_bgra`'s allocation aborts the process rather than
            // returning an error if we let it through. Checked inside the
            // closure, not before calling `with`, so a closed document
            // still reports `Closed` rather than `RenderFailed`.
            if !scale.is_finite() || scale <= 0.0 {
                return Err(OpsError::RenderFailed);
            }
            let page = doc.page(index)?;
            // Same rounding as build/generate-vectors.mjs. Changing it breaks
            // the golden buffer comparison in every host.
            let width = ((page.width() * scale).round() as u32).max(1);
            let height = ((page.height() * scale).round() as u32).max(1);
            // checked_mul, because both casts saturate at u32::MAX and
            // u32::MAX * 4 * u32::MAX is about 7.4e19, past u64::MAX. Any
            // scale over roughly 7e6 on the 612x792 fixture gets there. In
            // debug, which is what `cargo test` builds, the plain multiply
            // panicked inside the worker job and catch_unwind reported
            // Internal instead of the RenderFailed this guard promises;
            // release wrapped to a small number and let the render through.
            let too_big = u64::from(width)
                .checked_mul(4)
                .and_then(|b| b.checked_mul(u64::from(height)))
                .is_none_or(|bytes| bytes > MAX_RENDER_BYTES);
            if too_big {
                return Err(OpsError::RenderFailed);
            }
            let bitmap = page.render_bgra(width, height)?;
            Ok(Bitmap {
                width: bitmap.width,
                height: bitmap.height,
                stride: bitmap.stride,
                bgra: bitmap.bgra,
            })
        })
    }

    pub fn page_text(&self, index: u32) -> Result<String, OpsError> {
        self.with(move |doc| Ok(doc.page(index)?.text()?.text()?))
    }

    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError> {
        self.with(move |doc| {
            let page = doc.page(index)?;
            let text = page.text()?;
            let mut hits = Vec::new();
            for (char_index, char_count) in text.find(&query, case_sensitive)? {
                let rects = text
                    .rects(char_index, char_count)?
                    .into_iter()
                    .map(|[left, top, right, bottom]| Rect { left, top, right, bottom })
                    .collect();
                hits.push(SearchHit { char_index, char_count, rects });
            }
            Ok(hits)
        })
    }

    /// Blocks until every operation enqueued before this call has run,
    /// including the removal itself: the post-condition is that nothing
    /// enqueued earlier is still outstanding when this returns. Idempotent.
    /// `Drop` does not use this; it uses the weaker, non-blocking
    /// `drop_close` instead, so that dropping a `Document` never stalls the
    /// caller behind whatever else the worker is doing.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let id = self.id;
        let _ = self.worker.run(move |state| {
            state.remove(id);
            Ok(())
        });
    }

    /// The non-blocking half of `close`, used by `Drop`. Sets the closed
    /// flag synchronously, so no operation started after this call can
    /// reach the slab entry, then enqueues the removal without waiting for
    /// it to run. The removal still happens: the worker processes its queue
    /// in order and cannot skip a job. What `Drop` gives up, relative to
    /// `close`, is the guarantee that the removal has *already* happened by
    /// the time the call returns; a render already in flight ahead of it in
    /// the queue is not waited on.
    fn drop_close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let id = self.id;
        self.worker.send(move |state| state.remove(id));
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.drop_close();
    }
}
