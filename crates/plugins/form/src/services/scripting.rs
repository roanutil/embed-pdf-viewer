//! `services/scripting.ts`: the keystroke, validate, calculate and format
//! script pipeline. Scripts run in the actions plugin's per-document realm:
//! when that realm offers a transaction port, JavaScript is on. The
//! `validation` setting decides, at each write, whether the pipeline runs;
//! this plugin owns only the pipeline.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError};
use epdf_kernel::Error;
use epdf_plugin_actions::{
    ActionOrigin, PhasedUiEffect, ScriptCommitContext, ScriptCommitSurface, ScriptRealmKind,
};
use serde_json::Value;

use crate::contract::FormUiEffect;
use crate::model::{FieldIndex, form_has_scripts};
use crate::scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
};

#[derive(Default)]
pub struct ScriptingSeam {
    /// The pipeline over the actions plugin's realm, made the first time
    /// one is there.
    pipeline: RefCell<Option<FormScriptingController>>,
    warned_no_realm: Cell<bool>,
    warmed: Cell<bool>,
}

/// What `surface` takes: a script result's UI effects, diagnostics and error.
pub struct Surfaced<'a> {
    pub ui_effects: &'a [FormUiEffect],
    pub diagnostics: &'a [ScriptDiagnostic],
    pub error: Option<&'a ScriptExecutionError>,
}

impl crate::controller::Core {
    /// `pipelineOf()`.
    fn pipeline_of(&self) -> Option<FormScriptingController> {
        if let Some(pipeline) = &*self.scripting.pipeline.borrow() {
            return Some(pipeline.clone());
        }
        let realm = self.actions()?.script_realm()?;
        let budget = realm.budget.clone();
        let transaction: TransactionPort =
            Rc::new(move |body: TransactionBody| realm.transaction::<Rc<dyn Any>>(body));
        let cx = self.cx.clone();
        let made = FormScriptingController::new(FormScriptingControllerOptions {
            document: Rc::new(move || cx.document()),
            transaction,
            budget: Some(budget),
        });
        let disposed = made.clone();
        self.cx.cleanup_fn(move || disposed.dispose());
        *self.scripting.pipeline.borrow_mut() = Some(made.clone());
        Some(made)
    }

    fn validation_runs_scripts(&self) -> bool {
        self.settings_now().get("validation").and_then(Value::as_str) == Some("scripts")
    }

    /// `scripting.controller()`: the pipeline when the document's scripts
    /// run, else `None`.
    pub(crate) fn script_controller(&self) -> Option<FormScriptingController> {
        if self.validation_runs_scripts() { self.pipeline_of() } else { None }
    }

    /// `scripting.scriptsFor(form)`: also `None` when no field has a script.
    pub(crate) fn scripts_for(&self, form: &FieldIndex) -> Option<FormScriptingController> {
        if !form_has_scripts(form) {
            return None;
        }
        if self.pipeline_of().is_none()
            && !self.scripting.warned_no_realm.get()
            && self.validation_runs_scripts()
        {
            self.scripting.warned_no_realm.set(true);
            crate::console::warn(
                "[form] this form has scripts, but they don't run: add the actions plugin with \
                 JavaScript on (`actionsPlugin({ javascript: { enabled: true, sandboxFactory } })`), \
                 or set the form's `validation` to 'none' to fill it in without them.",
            );
        }
        self.script_controller()
    }

    /// `scripting.warm(form)`: start the realm before the first write needs
    /// it, once, when the form has scripts.
    pub(crate) fn warm_scripts(&self, form: &FieldIndex) {
        if self.scripting.warmed.get() {
            return;
        }
        let Some(snapshot) = form.snapshot.clone() else { return };
        let Some(scripts) = self.scripts_for(form) else { return };
        self.scripting.warmed.set(true);
        // The first write starts the realm, and reports why it can't.
        scripts.warm(snapshot).ignore();
    }

    /// `scripting.surface(result, origin)`: script results go out through
    /// the actions plugin, with the origin that caused them. `Err` is a UI
    /// adapter that threw, which the TypeScript lets through to the write.
    pub(crate) fn surface(&self, result: Surfaced<'_>, origin: ActionOrigin) -> Result<(), Error> {
        let Some(actions) = self.actions() else { return Ok(()) };
        let commit = ScriptCommitSurface {
            ui_effects: result
                .ui_effects
                .iter()
                .map(|e| PhasedUiEffect { effect: e.effect.clone(), phase: e.phase })
                .collect(),
            diagnostics: result.diagnostics.to_vec(),
            error: result.error.cloned(),
        };
        let context =
            ScriptCommitContext { origin, realm: ScriptRealmKind::Document, source: None };
        actions.surface_script_commit(&commit, &context)
    }
}
