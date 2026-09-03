#[derive(Debug, Clone, PartialEq)]
pub enum OpsError {
    LoadFailed,
    PasswordRequired,
    PageOutOfRange { index: u32 },
    RenderFailed,
    Closed,
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
            Self::Internal { message } => write!(f, "internal error: {message}"),
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
