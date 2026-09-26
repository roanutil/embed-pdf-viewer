//! `services/scripting.ts`: the K/V/C/F seam. Scripting rides the actions
//! plugin's per-document realm: its presence is the "JavaScript is on"
//! signal, and form owns only the keystroke, validate, calculate and format
//! pipeline.

use std::rc::Rc;

use epdf_kernel::Controller;
use epdf_kernel::Error;
use epdf_kernel::scope::Teardown;
use epdf_plugin_actions::{
    ActionOrigin, ActionsCapability, PhasedUiEffect, ScriptCommitContext, ScriptCommitSurface,
    ScriptRealmKind,
};

use crate::contract::{FormCommitResult, FormConfig, Validation};
use crate::controller::FormCapability;
use crate::model::{FormAction, FormState};
use crate::scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
};

/// `createScriptingSeam(ctx, config, siblings).controller`: a controller over
/// the actions plugin's realm, or `None` with scripting off, without a
/// document, or with `validation: 'none'`.
pub(crate) fn create_scripting_controller(
    cx: &Controller<FormState, FormAction>,
    config: &FormConfig,
    actions: &Option<ActionsCapability>,
) -> Option<FormScriptingController> {
    let realm = actions.as_ref()?.script_realm()?;
    let doc = cx.doc().ok()?;
    if config.validation == Some(Validation::None) {
        return None;
    }
    let budget = realm.budget.clone();
    let transaction: TransactionPort =
        Rc::new(move |body: TransactionBody| realm.transaction(body));
    let document_cx = cx.clone();
    let scripting = FormScriptingController::new(FormScriptingControllerOptions {
        doc,
        document: Rc::new(move || document_cx.document()),
        transaction,
        budget: Some(budget),
    });
    let disposing = scripting.clone();
    let teardown: Teardown = Box::new(move || {
        disposing.dispose();
        Ok(None)
    });
    cx.defer(teardown);
    Some(scripting)
}

/// A commit result as the actions port reads it.
pub fn commit_surface(result: &FormCommitResult) -> ScriptCommitSurface {
    ScriptCommitSurface {
        ui_effects: result
            .ui_effects
            .iter()
            .map(|e| PhasedUiEffect { effect: e.effect.clone(), phase: e.phase })
            .collect(),
        diagnostics: result.diagnostics.clone(),
        error: result.error.clone(),
    }
}

impl FormCapability {
    /// `surfaceViaActions(result, origin)`: every script surface flows
    /// through the actions plugin's one port. `Err` is an adapter that threw.
    pub(crate) fn surface_via_actions(
        &self,
        result: &FormCommitResult,
        origin: ActionOrigin,
    ) -> Result<(), Error> {
        let Some(actions) = &self.0.actions else { return Ok(()) };
        actions.surface_script_commit(
            &commit_surface(result),
            ScriptCommitContext { origin, realm: ScriptRealmKind::Document },
        )
    }
}
