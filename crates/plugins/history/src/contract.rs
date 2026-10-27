//! `contract.ts`: the history's public contract, one undo and redo history
//! per open document, of the changes this session made through the viewer.

use std::sync::LazyLock;

use epdf_kernel::{Error, Token};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::controller::HistoryCapability;
use crate::model::ChangeLabel;

/// The package token.
pub static HISTORY: LazyLock<Token<HistoryCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "history",
        "add historyPlugin() from '@embedpdf/plugin-history' to your plugins list",
    )
});

/// `HistorySettings`: [`history_defaults`], with what the app registers
/// merged over them; `update_settings` changes them for every document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistorySettings {
    /// How many steps undo can go back. The oldest go first.
    pub limit: f64,
}

/// `HISTORY_DEFAULTS`.
pub fn history_defaults() -> HistorySettings {
    HistorySettings { limit: 100.0 }
}

/// The defaults as the kernel's JSON settings store holds them.
pub(crate) fn defaults_json() -> Value {
    json!({ "limit": 100 })
}

/// `HistoryConfig`: any of the settings, merged over the defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
}

/// `{settings, changed}`, as `onSettingsChanged` hands it over.
#[derive(Clone, Debug, PartialEq)]
pub struct HistorySettingsChanged {
    pub settings: HistorySettings,
    pub changed: Vec<String>,
}

/// `HistoryUndoneEvent`: an undo or a redo the engine answered.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HistoryUndoneEvent {
    /// What was undone or redone, as the change was called.
    pub label: ChangeLabel,
    /// Whether it was a redo.
    pub redo: bool,
    /// How many of its parts were left alone, in whole or in part, because
    /// someone changed them since. `0` when everything was undone.
    pub skipped: u64,
}

/// Why an undo or a redo was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoFailure {
    /// It can't be undone any more, and the history forgot it and
    /// everything before it.
    Unavailable,
    /// The engine refused it for another reason (`error` says which), and
    /// the history kept it: it can be tried again.
    Refused,
}

/// `HistoryUndoFailedEvent`: an undo or a redo the engine refused.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryUndoFailedEvent {
    pub label: ChangeLabel,
    pub redo: bool,
    pub reason: UndoFailure,
    /// The `PluginError` the change was refused with.
    pub error: Error,
}
