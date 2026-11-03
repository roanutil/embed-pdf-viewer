//! `model.ts`: the interaction state (the active tool, the stack `push_tool`
//! saves, and the resolved cursor, so UI can react) and its pure
//! transitions. Tools, handlers and cursor claims are registries in the
//! controller.
//!
//! Each transition takes the state's `Rc` and returns it unchanged when
//! nothing changes, as the TypeScript returns `state`: the slice lease
//! publishes by identity.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::contract::{Cursor, Tool, ToolId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub active_tool_id: ToolId,
    /// Tools activated with `push_tool`, oldest first; `pop_tool` restores the last.
    pub tool_stack: Vec<ToolId>,
    pub cursor: Cursor,
}

/// `initialInteractionState()`: a document starts on `pointer`; the
/// controller then activates the `defaultTool` setting.
pub fn initial_interaction_state() -> InteractionState {
    InteractionState {
        active_tool_id: "pointer".to_string(),
        tool_stack: Vec::new(),
        cursor: "default".to_string(),
    }
}

/// `activateTool(state, toolId)`: activate a tool and forget every pushed one.
pub fn activate_tool(state: &Rc<InteractionState>, tool_id: &str) -> Rc<InteractionState> {
    if state.active_tool_id == tool_id && state.tool_stack.is_empty() {
        return state.clone();
    }
    Rc::new(InteractionState {
        active_tool_id: tool_id.to_string(),
        tool_stack: Vec::new(),
        cursor: state.cursor.clone(),
    })
}

/// `pushTool(state, toolId)`: activate a tool for a moment, saving the
/// active one for [`pop_tool`].
pub fn push_tool(state: &Rc<InteractionState>, tool_id: &str) -> Rc<InteractionState> {
    let mut tool_stack = state.tool_stack.clone();
    tool_stack.push(state.active_tool_id.clone());
    Rc::new(InteractionState {
        active_tool_id: tool_id.to_string(),
        tool_stack,
        cursor: state.cursor.clone(),
    })
}

/// `popTool(state)`: activate the tool saved last; an empty stack changes nothing.
pub fn pop_tool(state: &Rc<InteractionState>) -> Rc<InteractionState> {
    let mut tool_stack = state.tool_stack.clone();
    let Some(active_tool_id) = tool_stack.pop() else {
        return state.clone();
    };
    Rc::new(InteractionState { active_tool_id, tool_stack, cursor: state.cursor.clone() })
}

/// `setCursor(state, cursor)`.
pub fn set_cursor(state: &Rc<InteractionState>, cursor: &str) -> Rc<InteractionState> {
    if state.cursor == cursor {
        return state.clone();
    }
    Rc::new(InteractionState { cursor: cursor.to_string(), ..(**state).clone() })
}

/// `builtinTools()`: the two built-in tools. Features add tools with
/// `register_tool`. `enables` is the composition seam: `pointer` turns on
/// text selection, annotation editing and marquee selection, `pan`
/// scrolling and annotation editing, with no text select. Both carry
/// `form-fill` and `link-nav`: filling forms and following links is the
/// resting state of a viewer.
pub fn builtin_tools() -> Vec<Rc<Tool>> {
    vec![
        Rc::new(Tool::new(
            "pointer",
            "default",
            ["text-select", "annotation-edit", "annotation-marquee", "form-fill", "link-nav"],
        )),
        Rc::new(
            Tool::new("pan", "grab", ["scroll", "annotation-edit", "form-fill", "link-nav"])
                .with_gap_cursor("grab"),
        ),
    ]
}
