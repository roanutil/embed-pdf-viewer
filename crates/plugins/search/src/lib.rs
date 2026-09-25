//! `packages/plugin/search` in Rust: document text search over the engine's
//! budgeted, cursor-resumable `search.query` slices. One user-visible search
//! session per document (`search`, streamed into state, navigated with
//! `next`/`prev`/`go_to`) and a session-free find service (`find_all`).
//! The Stage is optional: with one, scans start at the current page and
//! navigation reveals the hit.
//!
//! The file split follows the TypeScript package: `types` (with the
//! engine-core search DTOs), `reducer`, `capability`, `effects`, and the
//! wiring in [`search_plugin`] (`search.plugin.ts`).

pub mod capability;
pub mod effects;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_stage::STAGE;

pub use capability::{SearchCapability, create_search_capability};
pub use effects::register_search_effects;
pub use reducer::{initial_search_state, search_reducer};
pub use types::*;

/// `searchPlugin(config)`: document-scoped, with the Stage optional. No
/// pointer handling, so no interaction hub.
pub fn search_plugin(config: SearchPluginConfig) -> AnyPlugin {
    define::<SearchState, SearchAction>("search")
        .document_scoped()
        .optional(STAGE.any())
        .initial_state(initial_search_state)
        .reduce(search_reducer)
        .capability(&SEARCH, move |ctx| Ok(create_search_capability(ctx, config)))
        .effects(register_search_effects)
        .build()
}
