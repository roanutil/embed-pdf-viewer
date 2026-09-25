//! `reducer.ts`.

use crate::types::{InteractionAction, InteractionConfig, InteractionState};

pub fn initial_interaction_state(config: &InteractionConfig) -> InteractionState {
    InteractionState {
        active_tool_id: config.default_tool.clone().unwrap_or_else(|| "pointer".to_string()),
        cursor: "default".to_string(),
    }
}

/// Pure. `None` is the TypeScript `return state`: no change, no notification.
pub fn interaction_reducer(
    state: &InteractionState,
    action: &InteractionAction,
) -> Option<InteractionState> {
    match action {
        InteractionAction::SetTool { tool_id } => (state.active_tool_id != *tool_id)
            .then(|| InteractionState { active_tool_id: tool_id.clone(), ..state.clone() }),
        InteractionAction::SetCursor { cursor } => (state.cursor != *cursor)
            .then(|| InteractionState { cursor: cursor.clone(), ..state.clone() }),
    }
}
