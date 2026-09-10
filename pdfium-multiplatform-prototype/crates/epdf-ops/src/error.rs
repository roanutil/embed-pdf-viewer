#[derive(Debug, Clone, PartialEq)]
pub enum OpsError {
    LoadFailed,
    PasswordRequired,
    PageOutOfRange { index: u32 },
    RenderFailed,
    /// The document this call named is closed: either the caller closed it,
    /// or it never existed in this engine's slab. The remedy is on the
    /// caller: open a new document.
    Closed,
    /// The worker thread that owns every document in this engine is gone
    /// (it panicked past recovery, or its channel is otherwise dead). Every
    /// document opened from this engine is unusable; the remedy is a new
    /// `Engine`, not a new document.
    EngineFailed { message: String },
    Internal { message: String },
}

impl std::fmt::Display for OpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoadFailed => write!(f, "the bytes are not a document PDFium can open"),
            Self::PasswordRequired => write!(f, "the document is encrypted and the password was wrong or missing"),
            Self::PageOutOfRange { index } => write!(f, "page index {index} is past the end of the document"),
            Self::RenderFailed => write!(f, "rendering the page failed"),
            Self::Closed => write!(f, "the document is closed"),
            Self::EngineFailed { message } => write!(f, "the engine's worker thread is gone: {message}"),
            Self::Internal { message } => write!(f, "internal error: {message}"),
        }
    }
}

impl OpsError {
    /// A stable, host-neutral code for switching on the failure without
    /// matching the human-readable message. Every host binding (napi, wasm
    /// glue, uniffi) reports this same string so their error contracts cannot
    /// drift. Exhaustive on purpose: a new variant fails the build here.
    pub fn code(&self) -> &'static str {
        match self {
            Self::LoadFailed => "LOAD_FAILED",
            Self::PasswordRequired => "PASSWORD_REQUIRED",
            Self::PageOutOfRange { .. } => "PAGE_OUT_OF_RANGE",
            Self::RenderFailed => "RENDER_FAILED",
            Self::Closed => "CLOSED",
            Self::EngineFailed { .. } => "ENGINE_FAILED",
            Self::Internal { .. } => "INTERNAL",
        }
    }
}

impl std::error::Error for OpsError {}

impl From<pdfium::PdfiumError> for OpsError {
    fn from(err: pdfium::PdfiumError) -> Self {
        match err {
            pdfium::PdfiumError::Load => Self::LoadFailed,
            pdfium::PdfiumError::Password => Self::PasswordRequired,
            pdfium::PdfiumError::PageOutOfRange(index) => Self::PageOutOfRange { index },
            pdfium::PdfiumError::PageLoad(index) => Self::Internal {
                message: format!("page {index} exists but PDFium could not load it"),
            },
            pdfium::PdfiumError::Bitmap => Self::RenderFailed,
            other => Self::Internal { message: other.to_string() },
        }
    }
}

#[cfg(test)]
mod code_tests {
    use super::OpsError;

    #[test]
    fn every_variant_has_a_stable_code_distinct_from_its_message() {
        let cases = [
            (OpsError::LoadFailed, "LOAD_FAILED"),
            (OpsError::PasswordRequired, "PASSWORD_REQUIRED"),
            (OpsError::PageOutOfRange { index: 3 }, "PAGE_OUT_OF_RANGE"),
            (OpsError::RenderFailed, "RENDER_FAILED"),
            (OpsError::Closed, "CLOSED"),
            (OpsError::EngineFailed { message: "gone".into() }, "ENGINE_FAILED"),
            (OpsError::Internal { message: "oops".into() }, "INTERNAL"),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code);
            assert_ne!(err.to_string(), code);
        }
    }
}
