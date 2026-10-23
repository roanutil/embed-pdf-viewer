//! `packages/core/main` in Rust: the viewer kernel.
//!
//! The kernel owns a store (one state tree: `core` plus keyed plugin
//! slices), a dependency-ordered plan of plugins, one resource scope per open
//! document, and the document lifecycle that opens, unlocks and closes them
//! against an [`engine::Engine`]. Plugins are [`plugin::PluginDef`]s; plugin
//! crates build them with [`plugin::define`].
//!
//! Everything runs on one thread, on the runtime in [`rt`]. The conformance
//! vectors under `tooling/plugin-conformance/vectors/kernel/` pin this crate
//! to the TypeScript; `crates/plugin-conformance/tests/kernel.rs` replays them.

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures; naming each one would add an alias per call site and hide the
// signature the TypeScript spells out.
#![allow(clippy::type_complexity)]

pub mod action;
pub mod bytes;
pub mod cancellable;
pub mod changes;
pub mod clock;
pub mod compose;
pub mod engine;
pub mod error;
pub mod event_hook;
pub mod geometry;
pub mod guarded;
pub mod identity;
pub mod js;
pub mod kernel;
pub mod lanes;
pub mod mirror;
pub mod page_mirror;
pub mod plan;
pub mod plugin;
pub mod rt;
pub mod scope;
pub mod serial_queue;
pub mod settings;
pub mod settle;
pub mod signal;
pub mod state;
pub mod store;
pub mod token;
pub mod types;

pub use action::Action;
pub use error::Error;
pub use kernel::{
    DOCUMENTS, Documents, Kernel, KernelOptions, KernelStatus, OpenOptions, OpenSource,
};
pub use plugin::{AnyPlugin, Controller, ControllerContext, Ctx, EffectCtx, define};
pub use rt::Promise;
pub use store::Unsubscribe;
pub use token::{AnyToken, Token};

/// JavaScript's `undefined` where a JSON value has to carry it: an argument
/// the TypeScript passes explicitly, or a member it reads back as absent.
pub fn undefined() -> serde_json::Value {
    serde_json::json!({ "$undefined": true })
}

pub fn is_undefined(value: &serde_json::Value) -> bool {
    value.get("$undefined").is_some_and(|v| v == true)
}
