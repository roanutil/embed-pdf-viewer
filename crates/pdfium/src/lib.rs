//! A safe, thread-confined wrapper over `pdfium-sys`.
//!
//! Group G0 (multiplatform-port-order.md). This layer owns handle lifetimes,
//! checked lengths and every `unsafe` boundary. It decides nothing about what
//! a PDF means; that is `epdf-engine`.
//!
//! Every handle type here is `!Send` by construction. PDFium's globals are
//! per-thread in the fork's native builds
//! (`build-target.sh` sets `embedpdf_thread_local_globals=true` for every
//! target except wasm32), and the engine gives each instance one owner OS
//! thread. `Rc` rather than `Arc` throughout is deliberate: it reinforces the
//! confinement rather than papering over it with atomics that would let a
//! handle cross threads and misbehave later.

mod base;
mod bitmap;
mod document;
mod error;
mod geom;
mod library;
mod page;
mod text;

pub use base::{BaseDocument, LayerDocument, LayerOpenError};
pub use bitmap::Bitmap;
pub use document::{Document, PasswordPermissions};
pub use error::PdfiumError;
pub use geom::{MatrixF, QuadF, RectF};
pub use library::Library;
pub use page::{Page, PageView, SharedPage};
pub use text::{CharGeometry, CharGeometryFlags, TextObjectKey, TextPage};
