//! Library initialization and teardown, and the thread affinity PDFium requires.
//!
//! Handle types are `!Send + !Sync`. One owner OS thread per engine, enforced in
//! Rust rather than by a caller's actor or dispatcher.
