//! uniffi's view of epdf-ops. Every type here is a thin newtype or a mirror,
//! because uniffi's derives have to sit on types this crate owns.
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SearchHit {
    pub char_index: u32,
    pub char_count: u32,
    pub rects: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// BGRA, `stride * height` bytes. uniffi copies this once on the way out:
    /// Data in Swift, ByteArray in Kotlin. At 4x zoom on a large page that copy
    /// is not free, and it is the first thing to revisit if render latency
    /// matters.
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum OpsError {
    #[error("the bytes are not a document PDFium can open")]
    LoadFailed,
    #[error("the document is encrypted and the password was wrong or missing")]
    PasswordRequired,
    #[error("page index {index} is past the end of the document")]
    PageOutOfRange { index: u32 },
    #[error("rendering the page failed")]
    RenderFailed,
    #[error("the document is closed")]
    Closed,
    #[error("the PDF engine failed and must be recreated: {message}")]
    EngineFailed { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl From<epdf_ops::OpsError> for OpsError {
    fn from(err: epdf_ops::OpsError) -> Self {
        match err {
            epdf_ops::OpsError::LoadFailed => Self::LoadFailed,
            epdf_ops::OpsError::PasswordRequired => Self::PasswordRequired,
            epdf_ops::OpsError::PageOutOfRange { index } => Self::PageOutOfRange { index },
            epdf_ops::OpsError::RenderFailed => Self::RenderFailed,
            epdf_ops::OpsError::Closed => Self::Closed,
            epdf_ops::OpsError::EngineFailed { message } => Self::EngineFailed { message },
            epdf_ops::OpsError::Internal { message } => Self::Internal { message },
        }
    }
}

#[derive(uniffi::Object)]
pub struct EpdfEngine {
    inner: Arc<epdf_ops::Engine>,
}

#[uniffi::export]
impl EpdfEngine {
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, OpsError> {
        Ok(Arc::new(Self { inner: epdf_ops::Engine::new()? }))
    }

    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<EpdfDocument>, OpsError> {
        Ok(Arc::new(EpdfDocument { inner: self.inner.open(bytes, password)? }))
    }
}

#[derive(uniffi::Object)]
pub struct EpdfDocument {
    inner: Arc<epdf_ops::Document>,
}

// epdf_ops::Document has no Debug impl of its own, so this can't be derived.
// Written by hand only so `Result<Arc<EpdfDocument>, OpsError>::unwrap_err()`
// type-checks in tests; the format is not meant to be read.
impl std::fmt::Debug for EpdfDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpdfDocument").finish_non_exhaustive()
    }
}

#[uniffi::export]
impl EpdfDocument {
    pub fn page_count(&self) -> Result<u32, OpsError> {
        Ok(self.inner.page_count()?)
    }

    pub fn page_size(&self, index: u32) -> Result<Size, OpsError> {
        let size = self.inner.page_size(index)?;
        Ok(Size { width: size.width, height: size.height })
    }

    pub fn render_page(&self, index: u32, scale: f32) -> Result<Bitmap, OpsError> {
        let b = self.inner.render_page(index, scale)?;
        Ok(Bitmap { width: b.width, height: b.height, stride: b.stride, bgra: b.bgra })
    }

    pub fn page_text(&self, index: u32) -> Result<String, OpsError> {
        Ok(self.inner.page_text(index)?)
    }

    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError> {
        Ok(self
            .inner
            .search(index, query, case_sensitive)?
            .into_iter()
            .map(|hit| SearchHit {
                char_index: hit.char_index,
                char_count: hit.char_count,
                rects: hit
                    .rects
                    .into_iter()
                    .map(|r| Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                    .collect(),
            })
            .collect())
    }

    /// Idempotent. Swift releases this object on ARC and would call it anyway;
    /// Kotlin releases on the GC, whenever that is, which is why it exists.
    pub fn close(&self) {
        self.inner.close();
    }
}
