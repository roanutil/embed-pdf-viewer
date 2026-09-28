//! Evaluation only. The kernel and every plugin on the browser main thread,
//! driven by name through the conformance harness adapters.
//!
//! docs/superpowers/plans/2026-09-27-web-rust-plugins.md is the plan.

pub mod adapters;
pub mod engine;
pub mod exports;
pub mod runtime;
