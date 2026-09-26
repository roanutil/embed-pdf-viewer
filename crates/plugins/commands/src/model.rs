//! `model.ts`: the commands slice, only the serializable category gates.
//! Definitions hold functions and live in the controller's registry. `None`
//! is the TypeScript's `return state`: no change, and no notification.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandsState {
    pub disabled_categories: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CommandsAction {
    #[serde(rename = "COMMANDS/DISABLE_CATEGORY")]
    DisableCategory { category: String },
    #[serde(rename = "COMMANDS/ENABLE_CATEGORY")]
    EnableCategory { category: String },
    #[serde(rename = "COMMANDS/SET_DISABLED_CATEGORIES")]
    SetDisabledCategories { categories: Vec<String> },
}

/// `initialCommandsState(disabled)`.
pub fn initial_commands_state(disabled: &[String]) -> CommandsState {
    CommandsState { disabled_categories: disabled.to_vec() }
}

/// `commandsReducer(state, action)`.
pub fn commands_reducer(state: &CommandsState, action: &CommandsAction) -> Option<CommandsState> {
    let has = |category: &str| state.disabled_categories.iter().any(|c| c == category);
    match action {
        CommandsAction::DisableCategory { category } => {
            if has(category) {
                return None;
            }
            let mut disabled_categories = state.disabled_categories.clone();
            disabled_categories.push(category.clone());
            Some(CommandsState { disabled_categories })
        }
        CommandsAction::EnableCategory { category } => {
            if !has(category) {
                return None;
            }
            let disabled_categories =
                state.disabled_categories.iter().filter(|c| *c != category).cloned().collect();
            Some(CommandsState { disabled_categories })
        }
        CommandsAction::SetDisabledCategories { categories } => {
            Some(CommandsState { disabled_categories: categories.clone() })
        }
    }
}
