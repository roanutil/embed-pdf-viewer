//! `packages/plugin/search` in Rust: a find service plus one user-visible
//! search session per document, over the engine's budgeted,
//! cursor-resumable `search.query` slices. The session (`search`, streamed
//! into state, navigated with `next_hit`, `previous_hit` and `go_to_hit`)
//! runs on the kernel's `latest` lane; the session-free service
//! (`find_all`) runs on its caller's signal. The Stage is optional: with
//! one, scans start at the current page and navigation reveals the hit.
//!
//! The engine answers matches with their segments already in page space,
//! so the plugin builds no text layout of its own (Ruling13.15 names search
//! among the layout's consumers; the current TypeScript reads none).
//!
//! The module split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], [`state`], [`token`], and the wiring in
//! [`search_plugin`] (`search.plugin.ts`).

pub mod contract;
pub mod controller;
#[cfg(feature = "uniffi")]
pub mod ffi;
pub mod model;
pub mod state;
pub mod token;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_stage::STAGE;
use serde_json::Value;

pub use contract::*;
pub use controller::{SearchCapability, create_search_controller};
pub use model::{
    HitList, SearchState, append_hits, cancel_session, clear_session, complete_session,
    fail_session, index_of_hit, initial_search_state, no_progress, pages_with_hits, set_active_hit,
    start_session,
};
pub use state::{SearchStateView, search_state};
pub use token::SEARCH;

/// `searchPlugin(config)`: document-scoped, with the Stage optional. No
/// pointer handling. `config` is the settings the app registers, over
/// [`search_defaults`].
pub fn search_plugin(config: SearchConfig) -> AnyPlugin {
    define::<SearchState, Value>("search")
        .document_scoped()
        .optional(STAGE.any())
        .initial_state(initial_search_state)
        .settings(SettingsDeclaration {
            defaults: serde_json::to_value(search_defaults()).expect("plain data"),
            registered: Some(config.to_json()),
            whole: Vec::new(),
        })
        .create(&SEARCH, |cx| {
            let (api, connect) = create_search_controller(cx)?;
            Ok((api, Some(connect)))
        })
        .build()
}
