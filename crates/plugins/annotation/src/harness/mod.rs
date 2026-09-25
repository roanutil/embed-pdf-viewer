//! The conformance adapter: how `crates/plugin-conformance` builds these
//! plugins from a vector and calls their capabilities by name, plus the
//! function tables.

pub mod functions;

pub use functions::{pure_functions, repository_functions};

/// The generator's `crypto.randomUUID` stand-in: the `n`th id of a run.
pub fn uuid_at(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
