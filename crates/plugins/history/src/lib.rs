//! `packages/plugin/history` in Rust: undo and redo, one history per open
//! document, built on the document's change queue (`ctx.changes`).
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], [`state`], and the wiring in [`history_plugin`].

pub mod contract;
pub mod controller;
pub mod model;
pub mod state;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub use contract::*;
pub use controller::HistoryCapability;
pub use model::{History, HistoryEntry, HistoryPart, empty_history};
pub use state::{HistoryStateView, history_state};

/// `historyPlugin(config)`: undo and redo, one history per open document, of
/// what this session changes through the viewer's plugins. Every change they
/// stage on the document's change queue is a step. No dependencies and no
/// engine state of its own. `config` is the settings the app registers, over
/// [`history_defaults`].
pub fn history_plugin(config: HistoryConfig) -> AnyPlugin {
    history_plugin_json(Some(serde_json::to_value(config).unwrap_or_default()))
}

/// [`history_plugin`] with the registered settings as JSON, as the
/// TypeScript `config` object arrives (`None` registers nothing).
pub fn history_plugin_json(registered: Option<Value>) -> AnyPlugin {
    define::<History, Value>("history")
        .document_scoped()
        .initial_state(empty_history)
        .settings(SettingsDeclaration {
            defaults: contract::defaults_json(),
            registered,
            whole: Vec::new(),
        })
        .create(&HISTORY, controller::create_history_controller)
        .build()
}
