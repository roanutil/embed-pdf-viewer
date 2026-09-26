//! `model.ts`: store-visible observability only (a dispatch counter and a
//! policy revision). Every real state lives with its owning area.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionsState {
    /// Monotonic dispatch counter.
    pub seq: f64,
    /// Bumped by `updatePolicy`, so store selectors re-read `getPolicy()`.
    pub policy_revision: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionsAction {
    #[serde(rename = "ACTIONS_DISPATCHED")]
    ActionsDispatched,
    #[serde(rename = "ACTIONS_POLICY_CHANGED")]
    ActionsPolicyChanged,
}

/// `initialActionsState()`.
pub fn initial_actions_state() -> ActionsState {
    ActionsState { seq: 0.0, policy_revision: 0.0 }
}

/// `actionsReducer(state, action)`. Every action is a change.
pub fn actions_reducer(state: &ActionsState, action: &ActionsAction) -> Option<ActionsState> {
    Some(match action {
        ActionsAction::ActionsDispatched => ActionsState { seq: state.seq + 1.0, ..state.clone() },
        ActionsAction::ActionsPolicyChanged => {
            ActionsState { policy_revision: state.policy_revision + 1.0, ..state.clone() }
        }
    })
}
