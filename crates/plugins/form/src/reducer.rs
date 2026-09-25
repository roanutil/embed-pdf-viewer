//! `reducer.ts`: a dumb store. The pure core computes the next model and
//! the shell dispatches it.

use std::rc::Rc;

use crate::core::model::initial_model;
use crate::types::{FormAction, FormState};

/// `initialFormState()`.
pub fn initial_form_state() -> FormState {
    FormState { model: Rc::new(initial_model()) }
}

/// `formReducer(state, action)`. `SET_MODEL` is the only action, so there
/// is no `return state` branch.
pub fn form_reducer(_state: &FormState, action: &FormAction) -> Option<FormState> {
    match action {
        FormAction::SetModel { model } => Some(FormState { model: model.clone() }),
    }
}
