//! Shared `build.rs` logic for every crate that links PDFium.
//!
//! This crate exists because Cargo does not propagate a dependency's
//! `cargo:rustc-link-arg` to a dependent's test or bin targets. It propagates
//! only `rustc-link-lib` and `rustc-link-search`, so each crate that needs the
//! link arguments has to emit them from its own build script, and the logic
//! lives here rather than being copied.
//!
//! Group G0 (multiplatform-port-order.md). Nothing is implemented yet: no
//! member declares a `build.rs`, so a stub function here would only be a
//! panic waiting to be called from a build script.
