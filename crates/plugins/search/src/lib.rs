//! `packages/plugin/search` in Rust: a find service plus one user-visible
//! search session per document, over the engine's budgeted,
//! cursor-resumable `search.query` slices. The session (`search`, streamed
//! into state, navigated with `next_hit`, `previous_hit` and `go_to_hit`)
//! runs on the kernel's `latest` lane; the session-free service
//! (`find_all`) runs on its own signal. The Stage is optional: with one,
//! scans start at the current page and navigation reveals the hit.
//!
//! The module split follows the TypeScript package: [`contract`] (with the
//! engine-core search DTOs), [`model`], [`controller`], [`token`], and the
//! wiring in [`search_plugin`] (`search.plugin.ts`).

pub mod contract;
pub mod controller;
#[cfg(feature = "uniffi")]
pub mod ffi;
pub mod model;
pub mod token;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_stage::STAGE;

pub use contract::*;
pub use controller::{SearchCapability, create_search_controller};
pub use model::{SearchAction, SearchState, initial_search_state, pages_with_hits, reduce_search};
pub use token::SEARCH;

/// `searchPlugin(config)`: document-scoped, with the Stage optional. No
/// pointer handling, so no interaction hub.
pub fn search_plugin(config: SearchConfig) -> AnyPlugin {
    define::<SearchState, SearchAction>("search")
        .document_scoped()
        .optional(STAGE.any())
        .initial_state(initial_search_state)
        .reduce(reduce_search)
        .create(&SEARCH, move |cx| Ok(create_search_controller(cx, config)))
        .build()
}
