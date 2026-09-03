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

fn to_napi(err: epdf_ops::OpsError) -> Error {
    Error::from_reason(err.to_string())
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
    pub fn page_count(&self) -> Result<u32> {
        self.inner.page_count().map_err(to_napi)
    }

    #[napi]
    pub fn page_size(&self, index: u32) -> Result<JsSize> {
        let size = self.inner.page_size(index).map_err(to_napi)?;
        Ok(JsSize { width: size.width as f64, height: size.height as f64 })
    }

    #[napi]
    pub fn render_page(&self, index: u32, scale: f64) -> Result<JsBitmap> {
        let b = self.inner.render_page(index, scale as f32).map_err(to_napi)?;
        Ok(JsBitmap { width: b.width, height: b.height, stride: b.stride, bgra: b.bgra.into() })
    }

    #[napi]
    pub fn page_text(&self, index: u32) -> Result<String> {
        self.inner.page_text(index).map_err(to_napi)
    }

    #[napi]
    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<JsSearchHit>> {
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
fn engine() -> &'static Arc<epdf_ops::Engine> {
    static ENGINE: std::sync::OnceLock<Arc<epdf_ops::Engine>> = std::sync::OnceLock::new();
    ENGINE.get_or_init(|| epdf_ops::Engine::new().expect("PDFium engine"))
}

#[napi]
pub fn open_document(bytes: Buffer, password: Option<String>) -> Result<Document> {
    let inner = engine().open(bytes.to_vec(), password).map_err(to_napi)?;
    Ok(Document { inner })
}
