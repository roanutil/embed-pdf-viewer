//! `reducer.ts`. `None` is the TypeScript's `return state`: no change, and
//! no notification.

use crate::types::{CommandsAction, CommandsState};

pub fn initial_commands_state() -> CommandsState {
    CommandsState { disabled_categories: Vec::new() }
}

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
