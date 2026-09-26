//! `model.ts`: the slice holds the active tool, the default, the tool stack
//! and the resolved cursor, so UI can react. Everything else is runtime
//! registries in the controller.

use serde::{Deserialize, Serialize};

use crate::contract::{Cursor, InteractionConfig, ToolId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub active_tool_id: ToolId,
    pub default_tool_id: ToolId,
    /// Tools armed with `pushTool`, oldest first; `popTool` restores the last.
    pub tool_stack: Vec<ToolId>,
    pub cursor: Cursor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InteractionAction {
    #[serde(rename = "SET_TOOL", rename_all = "camelCase")]
    SetTool { tool_id: ToolId },
    #[serde(rename = "PUSH_TOOL", rename_all = "camelCase")]
    PushTool { tool_id: ToolId },
    #[serde(rename = "POP_TOOL")]
    PopTool,
    #[serde(rename = "SET_CURSOR")]
    SetCursor { cursor: Cursor },
}

/// `initialInteractionState(config)`.
pub fn initial_interaction_state(config: &InteractionConfig) -> InteractionState {
    let default_tool = config.default_tool.clone().unwrap_or_else(|| "pointer".to_string());
    InteractionState {
        active_tool_id: default_tool.clone(),
        default_tool_id: default_tool,
        tool_stack: Vec::new(),
        cursor: "default".to_string(),
    }
}

/// `reduceInteraction(state, action)`. `None` is `return state`.
pub fn reduce_interaction(
    state: &InteractionState,
    action: &InteractionAction,
) -> Option<InteractionState> {
    match action {
        InteractionAction::SetTool { tool_id } => {
            if state.active_tool_id == *tool_id && state.tool_stack.is_empty() {
                return None;
            }
            Some(InteractionState {
                active_tool_id: tool_id.clone(),
                tool_stack: Vec::new(),
                ..state.clone()
            })
        }
        InteractionAction::PushTool { tool_id } => {
            let mut tool_stack = state.tool_stack.clone();
            tool_stack.push(state.active_tool_id.clone());
            Some(InteractionState { tool_stack, active_tool_id: tool_id.clone(), ..state.clone() })
        }
        InteractionAction::PopTool => {
            let mut tool_stack = state.tool_stack.clone();
            let active_tool_id = tool_stack.pop()?;
            Some(InteractionState { tool_stack, active_tool_id, ..state.clone() })
        }
        InteractionAction::SetCursor { cursor } => (state.cursor != *cursor)
            .then(|| InteractionState { cursor: cursor.clone(), ..state.clone() }),
    }
}
