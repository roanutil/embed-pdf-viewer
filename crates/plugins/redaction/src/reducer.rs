//! `reducer.ts`.

use crate::types::{RedactionAction, RedactionState};

pub fn initial_redaction_state() -> RedactionState {
    RedactionState { applying: false, last_result: None }
}

/// Pure. Both transitions make a new state; the TypeScript's `default` arm
/// (`return state`) has no Rust action to match.
pub fn redaction_reducer(
    state: &RedactionState,
    action: &RedactionAction,
) -> Option<RedactionState> {
    match action {
        RedactionAction::ApplyStarted => Some(RedactionState { applying: true, ..state.clone() }),
        RedactionAction::ApplyFinished { result } => Some(RedactionState {
            applying: false,
            // `action.result ?? state.lastResult`
            last_result: result.clone().or_else(|| state.last_result.clone()),
        }),
    }
}
