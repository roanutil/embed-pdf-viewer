//! Six coarse operations over the EmbedPDF PDFium fork, and the only surface
//! any host language sees. Recommendation 3 of
//! docs/research/pdfium-rust-wrapper-decision.md is why it is coarse: a
//! per-item host boundary measured 2.6x worse in wasm and 7.16x worse natively
//! than a per-operation one.
mod dispatch;
mod engine;
mod error;
mod state;
mod types;

pub use engine::{Document, Engine};
pub use error::OpsError;
pub(crate) use state::State;
pub use types::{Bitmap, Rect, SearchHit, Size};
