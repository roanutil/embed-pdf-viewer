//! `packages/plugin/outline` in Rust: the document's outline (its
//! bookmarks), mirrored from the engine, with which items are expanded and
//! selected, the item for the reading position, and its edits as undoable
//! changes.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! `sync` (`sync/tree.ts`), [`controller`], [`state`], and the wiring of
//! `outline.plugin.ts` in [`outline_plugin`]. The outline is a kernel
//! mirror, read on demand: nothing reads it when a document opens.

mod sync;

pub mod contract;
pub mod controller;
pub mod model;
pub mod state;

#[cfg(feature = "harness")]
pub mod harness;

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_i18n::I18N;
use epdf_plugin_link::LINK;
use epdf_plugin_selection::SELECTION;
use epdf_plugin_stage::STAGE;
use serde_json::Value;

pub use contract::*;
pub use controller::{OutlineCapability, create_outline_controller};
pub use model::{OutlineState, initial_outline_state};
pub use state::{OutlineStateView, outline_state, outline_state_ensure};
pub use sync::{action_tree_of, outline_item_after, outline_item_of_draft};

/// `OutlineToken` (`token.ts`).
pub static OUTLINE: LazyLock<Token<OutlineCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "outline",
        "add outlinePlugin() from '@embedpdf/plugin-outline' to your plugins list",
    )
});

/// `outlinePlugin()`: the document's outline. Document-scoped; optionally
/// uses the stage (where items go, the reading position), actions (an
/// item's `/A`), link (websites), selection (a new item's title) and i18n
/// (its default title).
pub fn outline_plugin() -> AnyPlugin {
    define::<OutlineState, Value>("outline")
        .document_scoped()
        .optional(STAGE.any())
        .optional(ACTIONS.any())
        .optional(LINK.any())
        .optional(SELECTION.any())
        .optional(I18N.any())
        .initial_state(initial_outline_state)
        .create(&OUTLINE, create_outline_controller)
        .build()
}
