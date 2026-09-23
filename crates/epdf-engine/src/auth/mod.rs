//! G3. Authorization, as far as the engine owns it.
//!
//! Only [`scope`] today. Session establishment and security probing live in
//! [`crate::session`]; this module is the pure vocabulary they enforce
//! against, with no PDFium dependency and no I/O.

pub mod scope;
