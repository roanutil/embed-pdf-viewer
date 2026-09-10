/// What a PDFium call refused to do. Names the operation, not a policy: the
/// translation to `EngineErrorCode` happens in `epdf-engine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfiumError {
    Load,
    Password,
    PageOutOfRange(u32),
    PageLoad(u32),
    TextLoad,
    TextRead,
    TextRect,
    Bitmap,
    Search,
    /// An argument this layer rejected before calling PDFium at all.
    InvalidArgument(&'static str),
    /// An allocation this layer refused to attempt or that the allocator
    /// refused. Never an abort: sizes are checked and reserved fallibly.
    OutOfMemory,
}

impl std::fmt::Display for PdfiumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load => write!(f, "the bytes are not a document PDFium can open"),
            Self::Password => write!(
                f,
                "the document is encrypted and the password was wrong or missing"
            ),
            Self::PageOutOfRange(i) => {
                write!(f, "page index {i} is past the end of the document")
            }
            Self::PageLoad(i) => write!(
                f,
                "FPDF_LoadPage failed for page index {i}, which is within range"
            ),
            Self::TextLoad => write!(f, "FPDFText_LoadPage failed"),
            Self::TextRead => write!(f, "EPDFText_GetTextFull failed"),
            Self::TextRect => write!(f, "FPDFText_GetRect failed"),
            Self::Bitmap => write!(f, "FPDFBitmap_CreateEx failed"),
            Self::Search => write!(f, "FPDFText_FindStart failed"),
            Self::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Self::OutOfMemory => write!(f, "allocation refused"),
        }
    }
}

impl std::error::Error for PdfiumError {}
