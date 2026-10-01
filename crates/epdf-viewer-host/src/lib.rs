//! The kernel and plugins over `epdf-engine`, exported through uniffi: the
//! P6 crate `multiplatform-plugins-plan.md` §4 names.

pub mod engine_worker;
pub mod host_table;
pub mod native_engine;
pub mod translate;

uniffi::setup_scaffolding!();
