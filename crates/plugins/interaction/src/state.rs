//! `state.ts`: the tools page's state table as code, what
//! `useInteractionState()` returns in every framework. With no document
//! there is no active tool and no tool to switch to.

use std::rc::Rc;

use crate::contract::{Tool, ToolId};
use crate::controller::InteractionCapability;

/// What `interactionState` reads.
#[derive(Clone, Debug)]
pub struct InteractionStateView {
    /// `None` only in the empty state.
    pub active_tool_id: Option<ToolId>,
    pub tools: Rc<[Rc<Tool>]>,
}

/// `interactionState`: the read over a document's capability, or the empty
/// state (no active tool, no tools) without one.
pub fn interaction_state(interaction: Option<&InteractionCapability>) -> InteractionStateView {
    match interaction {
        Some(interaction) => InteractionStateView {
            active_tool_id: Some(interaction.get_active_tool_id()),
            tools: interaction.list_tools(),
        },
        None => InteractionStateView { active_tool_id: None, tools: Rc::new([]) },
    }
}
