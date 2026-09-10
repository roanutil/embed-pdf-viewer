#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfiumError {
    Init,
    Load,
    Password,
    PageOutOfRange(u32),
    PageLoad(u32),
    TextLoad,
    TextRead,
    TextRect,
    Bitmap,
    Search,
}

impl std::fmt::Display for PdfiumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init => write!(f, "FPDF_InitLibrary failed"),
            Self::Load => write!(f, "the bytes are not a document PDFium can open"),
            Self::Password => write!(f, "the document is encrypted and the password was wrong or missing"),
            Self::PageOutOfRange(i) => write!(f, "page index {i} is past the end of the document"),
            Self::PageLoad(i) => write!(f, "FPDF_LoadPage failed for page index {i}, which is within range"),
            Self::TextLoad => write!(f, "FPDFText_LoadPage failed"),
            Self::TextRead => write!(f, "EPDFText_GetTextFull failed"),
            Self::TextRect => write!(f, "FPDFText_GetRect failed"),
            Self::Bitmap => write!(f, "FPDFBitmap_CreateEx failed"),
            Self::Search => write!(f, "FPDFText_FindStart failed"),
        }
    }
}

impl std::error::Error for PdfiumError {}
