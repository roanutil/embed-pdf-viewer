use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dispatch::Worker;
use crate::{Bitmap, OpsError, Rect, SearchHit, Size};

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
            let page = doc.page(index)?;
            // Same rounding as build/generate-vectors.mjs. Changing it breaks
            // the golden buffer comparison in every host.
            let width = ((page.width() * scale).round() as u32).max(1);
            let height = ((page.height() * scale).round() as u32).max(1);
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
