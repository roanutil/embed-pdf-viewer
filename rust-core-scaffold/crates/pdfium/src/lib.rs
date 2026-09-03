//! A safe, thread-confined wrapper over pdfium-sys. Every handle type here is
//! !Send by construction, because PDFium's globals are per-thread in the fork's
//! native builds. epdf-ops is what makes these usable from another thread.
mod bitmap;
mod document;
mod error;
mod library;
mod page;
mod text;

pub use bitmap::Bitmap;
pub use document::Document;
pub use error::PdfiumError;
pub use library::Library;
pub use page::Page;
pub use text::TextPage;
