//! `model.ts`: the redaction slice, the apply latch and the last result.
//! Marks live on the annotation plane.

use serde::{Deserialize, Serialize};

use crate::contract::RedactionApplyResult;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionState {
    /// A destructive apply is in flight.
    pub applying: bool,
    /// The last apply result seen (own or remote), for status UI.
    pub last_result: Option<RedactionApplyResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RedactionAction {
    #[serde(rename = "APPLY_STARTED")]
    ApplyStarted,
    #[serde(rename = "APPLY_FINISHED")]
    ApplyFinished {
        #[serde(default)]
        result: Option<RedactionApplyResult>,
    },
}

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
