/// What a PDFium call refused to do. Names the operation, not a policy: the
/// translation to `EngineErrorCode` happens in `epdf-engine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfiumError {
    Load,
    Password,
    /// The document declares a security handler PDFium does not implement.
    ///
    /// Separate from [`PdfiumError::Password`] because the two send a caller
    /// somewhere different: a password prompt can resolve one and can never
    /// resolve the other.
    Security,
    PageOutOfRange(u32),
    PageLoad(u32),
    /// A page addressed by PDF object number could not be loaded.
    PageLoadByObjectNumber(u32),
    TextLoad,
    TextRead,
    TextRect,
    Bitmap,
    /// PDFium refused a font: unloadable, or it carries no glyphs.
    FontRegistration,
    Search,
    /// A serialization PDFium declined, or one that produced no bytes.
    Save,
    /// PDFium refused to build a destination array.
    DestinationBuild,
    /// PDFium refused to build an action dictionary.
    ActionBuild,
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
            Self::Security => {
                write!(f, "the document uses a security handler PDFium does not support")
            }
            Self::Password => {
                write!(f, "the document is encrypted and the password was wrong or missing")
            }
            Self::PageOutOfRange(i) => {
                write!(f, "page index {i} is past the end of the document")
            }
            Self::PageLoad(i) => {
                write!(f, "FPDF_LoadPage failed for page index {i}, which is within range")
            }
            Self::PageLoadByObjectNumber(number) => {
                write!(f, "no page with object number {number}")
            }
            Self::FontRegistration => {
                write!(f, "the font could not be registered: unloadable, or it has no glyphs")
            }
            Self::TextLoad => write!(f, "FPDFText_LoadPage failed"),
            Self::TextRead => write!(f, "EPDFText_GetTextFull failed"),
            Self::TextRect => write!(f, "FPDFText_GetRect failed"),
            Self::Bitmap => write!(f, "FPDFBitmap_CreateEx failed"),
            Self::Search => write!(f, "FPDFText_FindStart failed"),
            Self::Save => write!(f, "the document could not be serialized"),
            Self::DestinationBuild => write!(f, "the destination array could not be built"),
            Self::ActionBuild => write!(f, "the action dictionary could not be built"),
            Self::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Self::OutOfMemory => write!(f, "allocation refused"),
        }
    }
}

impl std::error::Error for PdfiumError {}
