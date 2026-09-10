//! Safe RAII wrappers over `pdfium-sys`.
//!
//! Owns library init, handle lifetimes, checked lengths and every `unsafe`
//! boundary. Document semantics belong in `epdf-engine`, not here.
//!
//! Group G0 (multiplatform-port-order.md). Its unsafe boundaries and lifecycle
//! assumptions are audited before promotion; the prototype's versions are a
//! reference, not an inheritance.

pub mod bitmap;
pub mod document;
pub mod error;
pub mod library;
pub mod page;
pub mod text;
