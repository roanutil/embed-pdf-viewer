//! The Node adapter. Six methods, no pointers.
//!
//! Recommendation 4 of docs/research/pdfium-rust-wrapper-decision.md prefers f64
//! pointers to BigInt for a Node addon over PDFium. It is moot here: a coarse
//! per-operation boundary hands JavaScript no handles at all, so there is no
//! pointer representation to get wrong. That is a reason to prefer a coarse
//! boundary, not evidence the recommendation was unnecessary.
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Maps every `epdf_ops::OpsError` variant to a stable code on the thrown
/// error's `.code` property, so a JS caller can `switch` on `err.code`
/// instead of regex-matching `err.message` (which stays the unchanged,
/// human-readable half). The codes:
///
/// - `LOAD_FAILED`: the bytes are not a document PDFium can open.
/// - `PASSWORD_REQUIRED`: the document is encrypted and the password was
///   wrong or missing.
/// - `PAGE_OUT_OF_RANGE`: the page index named is past the end of the
///   document.
/// - `RENDER_FAILED`: rendering the page failed.
/// - `CLOSED`: the document is closed, either by the caller or because it
///   never existed in this engine's slab. Caller error: open a new document.
/// - `ENGINE_FAILED`: the worker thread that owns every document in this
///   engine is gone. Every document from this engine is unusable; the
///   remedy is a new `Engine`, not a new document.
/// - `INTERNAL`: an unexpected failure with no more specific code.
///
/// The match is exhaustive with no `_` arm: a new `OpsError` variant must
/// fail this build rather than silently collapse into `INTERNAL`, the same
/// guarantee `crates/epdf-uniffi`'s mirror gives Swift and Kotlin.
fn to_napi(err: epdf_ops::OpsError) -> Error<String> {
    let code = err.code();
    Error::new(code.to_string(), err.to_string())
}

#[napi(object)]
pub struct JsSize {
    pub width: f64,
    pub height: f64,
}

#[napi(object)]
pub struct JsRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[napi(object)]
pub struct JsSearchHit {
    pub char_index: u32,
    pub char_count: u32,
    pub rects: Vec<JsRect>,
}

#[napi(object)]
pub struct JsBitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Buffer,
}

#[napi]
pub struct Document {
    inner: Arc<epdf_ops::Document>,
}

#[napi]
impl Document {
    #[napi]
    pub fn page_count(&self) -> Result<u32, String> {
        self.inner.page_count().map_err(to_napi)
    }

    #[napi]
    pub fn page_size(&self, index: u32) -> Result<JsSize, String> {
        let size = self.inner.page_size(index).map_err(to_napi)?;
        Ok(JsSize { width: size.width as f64, height: size.height as f64 })
    }

    #[napi]
    pub fn render_page(&self, index: u32, scale: f64) -> Result<JsBitmap, String> {
        let b = self.inner.render_page(index, scale as f32).map_err(to_napi)?;
        Ok(JsBitmap { width: b.width, height: b.height, stride: b.stride, bgra: b.bgra.into() })
    }

    #[napi]
    pub fn page_text(&self, index: u32) -> Result<String, String> {
        self.inner.page_text(index).map_err(to_napi)
    }

    #[napi]
    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<JsSearchHit>, String> {
        Ok(self
            .inner
            .search(index, query, case_sensitive)
            .map_err(to_napi)?
            .into_iter()
            .map(|hit| JsSearchHit {
                char_index: hit.char_index,
                char_count: hit.char_count,
                rects: hit
                    .rects
                    .into_iter()
                    .map(|r| JsRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                    .collect(),
            })
            .collect())
    }

    #[napi]
    pub fn close(&self) {
        self.inner.close();
    }
}

/// One process-wide engine, and therefore one PDFium thread. A scaffold does
/// not need a pool, and a second engine would mean a second thread with its own
/// FPDF_InitLibrary, which is legal but pointless here.
/// Initializes the shared value once, but only caches a success. A failed
/// `Engine::new` (a thread-spawn EAGAIN, say) is returned to this caller and
/// the next call tries again, rather than one transient failure poisoning
/// every later `openDocument` in the process.
fn initialize_engine<T: Clone>(
    cell: &std::sync::Mutex<Option<T>>,
    initialize: impl FnOnce() -> std::result::Result<T, epdf_ops::OpsError>,
) -> std::result::Result<T, epdf_ops::OpsError> {
    let mut slot = cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = slot.as_ref() {
        return Ok(value.clone());
    }
    let value = initialize()?;
    *slot = Some(value.clone());
    Ok(value)
}

fn engine() -> Result<Arc<epdf_ops::Engine>, String> {
    static ENGINE: std::sync::Mutex<Option<Arc<epdf_ops::Engine>>> = std::sync::Mutex::new(None);
    initialize_engine(&ENGINE, epdf_ops::Engine::new).map_err(to_napi)
}

#[napi]
pub fn open_document(bytes: Buffer, password: Option<String>) -> Result<Document, String> {
    let inner = engine()?.open(bytes.to_vec(), password).map_err(to_napi)?;
    Ok(Document { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_failure_is_reported_and_retried_on_the_next_call() {
        let failure = epdf_ops::OpsError::Internal { message: "thread creation failed".into() };
        let cell = std::sync::Mutex::new(None);
        let first = initialize_engine(&cell, || Err(failure.clone())).unwrap_err();
        assert_eq!(first, failure);
        // A transient failure must not poison the process: the next call
        // initializes again instead of replaying the cached error.
        assert_eq!(initialize_engine(&cell, || Ok(7u32)).unwrap(), 7);
        assert_eq!(initialize_engine(&cell, || panic!("must not initialize twice")).unwrap(), 7);
    }
}
