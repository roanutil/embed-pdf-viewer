//! Server binding layer: `napi-rs` exports for Linux and macOS.
//!
//! The Phase 5 server milestone hosts this crate from
//! `cloudpdf/server/src/runtime/worker-entry.ts:18`, behind a supervised host
//! with `QuarantiningEnginePool` and `EngineRecycler`.
//!
//! Exports must return `Result` rather than panic: `napi-derive` only wraps
//! functions marked `#[napi(catch_unwind)]`, which this crate does not use.
