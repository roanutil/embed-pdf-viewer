//! `model.ts`: the form slice, the pure core `Model` plus the hydration
//! status. The pure `update` runs in the store service; the reducer only
//! stores.

use std::rc::Rc;

use serde::Serialize;

use crate::contract::ResourceStatus;
use crate::core::model::{Model, initial_model};

/// `FormState`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FormState {
    pub model: Rc<Model>,
    /// Hydration: `loading` until the field tree is in, then `ready`;
    /// `forbidden` without `doc.forms.read`.
    pub status: ResourceStatus,
}

/// `FormAction`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum FormAction {
    #[serde(rename = "SET_MODEL")]
    SetModel { model: Rc<Model> },
    #[serde(rename = "SET_STATUS")]
    SetStatus { status: ResourceStatus },
}

/// `initialFormState()`.
pub fn initial_form_state() -> FormState {
    FormState { model: Rc::new(initial_model()), status: ResourceStatus::Idle }
}

/// `formReducer(state, action)`. `None` is the TypeScript's `return state`.
pub fn form_reducer(state: &FormState, action: &FormAction) -> Option<FormState> {
    match action {
        FormAction::SetModel { model } => {
            Some(FormState { model: model.clone(), status: state.status })
        }
        FormAction::SetStatus { status } => (state.status != *status)
            .then(|| FormState { model: state.model.clone(), status: *status }),
    }
}
