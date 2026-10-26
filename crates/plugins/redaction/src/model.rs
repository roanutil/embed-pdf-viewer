//! `model.ts`: the redaction session, whether an apply is running and the
//! last confirmed apply result. Marks are annotations and live on the
//! annotation plane. Every function below is a pure transition; the
//! controller applies them with `ctx.state.update` (the kernel's
//! `update_state`). A transition that changes nothing hands back the same
//! `Rc`, as the TypeScript hands back the same object.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::contract::RedactionApplyResult;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionState {
    /// An apply of this session is running at the engine.
    pub applying: bool,
    /// The last confirmed apply result, from this session or another.
    pub last_result: Option<RedactionApplyResult>,
}

/// `initialRedactionState()`.
pub fn initial_redaction_state() -> RedactionState {
    RedactionState { applying: false, last_result: None }
}

/// `startApply(state)`.
pub fn start_apply(state: Rc<RedactionState>) -> Rc<RedactionState> {
    if state.applying {
        return state;
    }
    Rc::new(RedactionState { applying: true, ..(*state).clone() })
}

/// `finishApply(state)`.
pub fn finish_apply(state: Rc<RedactionState>) -> Rc<RedactionState> {
    if !state.applying {
        return state;
    }
    Rc::new(RedactionState { applying: false, ..(*state).clone() })
}

/// `setLastResult(state, result)`: the same state when it already holds
/// this result object.
pub fn set_last_result(
    state: Rc<RedactionState>,
    result: &RedactionApplyResult,
) -> Rc<RedactionState> {
    if state.last_result.as_ref().is_some_and(|last| Rc::ptr_eq(last, result)) {
        return state;
    }
    Rc::new(RedactionState { last_result: Some(result.clone()), ..(*state).clone() })
}
