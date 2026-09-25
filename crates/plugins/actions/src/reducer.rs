//! `reducer.ts`.

use crate::types::{ActionsAction, ActionsState};

pub fn initial_actions_state() -> ActionsState {
    ActionsState { seq: 0.0 }
}

/// Pure. The one transition counts a dispatch.
pub fn actions_reducer(state: &ActionsState, action: &ActionsAction) -> Option<ActionsState> {
    match action {
        ActionsAction::ActionsDispatched => Some(ActionsState { seq: state.seq + 1.0 }),
    }
}
