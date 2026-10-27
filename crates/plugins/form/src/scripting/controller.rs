//! `scripting/controller.ts`: the K/V/C/F passes. Keystroke, validate,
//! calculate and format scripts over one working copy of the form
//! (`overlay.rs`), inside one realm transaction. The controller reads the
//! form it is given and returns what the scripts changed; writing it is the
//! caller's, inside the same transaction, so the next transaction reads the
//! form with it.
//!
//! The controller never owns a realm. It rides a realm port: this
//! document's own (the actions plugin's `script_realm`) or a detached one
//! the actions plugin mints for a document that is not displayed. Either
//! way the owner disposes the realm; [`FormScriptingController::dispose`]
//! only fences further transactions.
//!
//! Kept divergence H4: the transaction's time budget runs on the host's
//! monotonic clock (`rt::monotonic_now`), not the virtual one. A real host
//! runs a whole K/V/C/F chain inside one drain, where the virtual clock
//! stands still, so the TypeScript's `Date.now()` arithmetic would never
//! run out (`tests/budget.rs`).

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, ScriptBudget, ScriptDiagnostic, ScriptEventInput, ScriptEventKind,
    ScriptExecutionError, ScriptExecutionErrorKind, ScriptFieldInput, ScriptOutput,
    ScriptTransaction, ScriptWorldInput, java_script_program_from_action_tree,
    script_fields_from_snapshot,
};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::types::DocumentMeta;
use epdf_plugin_actions::ScriptPhase;
use serde::Serialize;
use serde_json::{Value, json};

use super::overlay::*;
use super::results::*;
use crate::contract::FormUiEffect;
use crate::value::{get, items, truthy, utf16_slice};

/// A value a realm transaction carries, whatever the body made.
pub type AnyPromise = Promise<Rc<dyn Any>>;
/// What runs while the realm is held.
pub type TransactionBody = Box<dyn FnOnce(ScriptTransaction) -> Result<AnyPromise, Error>>;
/// The realm's transaction port (`ScriptTransaction`): one at a time.
pub type TransactionPort = Rc<dyn Fn(TransactionBody) -> AnyPromise>;

/// `FormScriptingControllerOptions`.
pub struct FormScriptingControllerOptions {
    /// What the scripts read of the document: its name, page count and pages.
    pub document: Rc<dyn Fn() -> Option<DocumentMeta>>,
    /// The realm's transaction port.
    pub transaction: TransactionPort,
    /// The transaction aggregate the realm owner hands over.
    pub budget: Option<ScriptBudget>,
}

/// How the scripts took a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptStatus {
    Accepted,
    Rejected,
    Failed,
}

/// `FormScriptResult`: what the form's scripts made of one write, before
/// anything is written.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormScriptResult {
    pub status: ScriptStatus,
    /// What the scripts changed in the form, in the order to write it.
    pub effects: Vec<Value>,
    /// What the document's open scripts changed: written even when the
    /// value is refused.
    pub boot_effects: Vec<Value>,
    pub ui_effects: Vec<FormUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
}

struct Inner {
    document: Rc<dyn Fn() -> Option<DocumentMeta>>,
    transaction: TransactionPort,
    budget: Option<ScriptBudget>,
    disposed: Cell<bool>,
}

/// `FormScriptingController`.
#[derive(Clone)]
pub struct FormScriptingController(Rc<Inner>);

/// `createFormScriptingController(options)`.
pub fn create_form_scripting_controller(
    options: FormScriptingControllerOptions,
) -> FormScriptingController {
    FormScriptingController::new(options)
}

/// `FormScripts`: the scripts one transaction can run.
#[derive(Clone)]
pub struct FormScripts {
    inner: Rc<Inner>,
    txn: ScriptTransaction,
}

impl FormScripts {
    /// A field's new value through its keystroke and validate scripts, then
    /// the calculations and the formats. `form` is the form without it.
    pub fn commit(&self, form: Rc<Value>, field: Value, value: Value) -> Promise<FormScriptResult> {
        Promise::spawn(run(
            self.inner.clone(),
            self.txn.clone(),
            form,
            Some(field),
            Some(value),
            None,
        ))
    }

    /// One widget's activation action (`/A`, with `/Next`).
    pub fn activate(
        &self,
        form: Rc<Value>,
        field: Value,
        action: Value,
    ) -> Promise<FormScriptResult> {
        Promise::spawn(run(
            self.inner.clone(),
            self.txn.clone(),
            form,
            Some(field),
            None,
            Some(action),
        ))
    }

    /// The document's open scripts (once per realm) and the calculations.
    pub fn recalculate(&self, form: Rc<Value>) -> Promise<FormScriptResult> {
        Promise::spawn(run(self.inner.clone(), self.txn.clone(), form, None, None, None))
    }
}

impl FormScriptingController {
    pub fn new(options: FormScriptingControllerOptions) -> Self {
        let FormScriptingControllerOptions { document, transaction, budget } = options;
        FormScriptingController(Rc::new(Inner {
            document,
            transaction,
            budget,
            disposed: Cell::new(false),
        }))
    }

    /// The realm belongs to its owner: this only fences further transactions.
    pub fn dispose(&self) {
        self.0.disposed.set(true);
    }

    /// One realm transaction: `body` runs its scripts and writes what they
    /// changed before the next transaction starts.
    pub fn transaction<T: Clone + 'static>(
        &self,
        body: impl FnOnce(FormScripts) -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        if self.0.disposed.get() {
            return Promise::rejected(Error::new("Form scripting controller is disposed"));
        }
        let inner = self.0.clone();
        let wrapped: TransactionBody = Box::new(move |txn| {
            let made = body(FormScripts { inner, txn })?;
            Ok(made.convert(|value: &T| Ok(Rc::new(value.clone()) as Rc<dyn Any>)))
        });
        (self.0.transaction)(wrapped).convert(|any: &Rc<dyn Any>| {
            any.downcast_ref::<T>()
                .cloned()
                .ok_or_else(|| Error::new("[form] a transaction answered another type"))
        })
    }

    /// Load the realm before the first write needs it: the scripts' engine
    /// starts, and the open scripts run, their effects kept for the first
    /// write. Nothing is written.
    pub fn warm(&self, form: Rc<Value>) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            if this.0.disposed.get() {
                return Ok(());
            }
            let budget = this.0.budget.clone().unwrap_or(DEFAULT_SCRIPT_BUDGET);
            let body: TransactionBody = Box::new(move |txn| {
                let fields = script_fields_from_snapshot(&form)?;
                let world = ScriptWorldInput {
                    fields,
                    annots: None,
                    annot_pages: None,
                    annots_cover_document: None,
                    page_number: None,
                    event: ScriptEventInput::of(ScriptEventKind::NameTreeBoot),
                };
                Ok(txn
                    .run("", world, Some(budget))
                    .convert(|o: &ScriptOutput| Ok(Rc::new(o.clone()) as Rc<dyn Any>)))
            });
            (this.0.transaction)(body).await?;
            Ok(())
        })
    }

    /// `commit` in a transaction of its own.
    pub fn commit(&self, form: Rc<Value>, field: Value, value: Value) -> Promise<FormScriptResult> {
        self.transaction(move |scripts| Ok(scripts.commit(form, field, value)))
    }

    /// `activate` in a transaction of its own.
    pub fn activate(
        &self,
        form: Rc<Value>,
        field: Value,
        action: Value,
    ) -> Promise<FormScriptResult> {
        self.transaction(move |scripts| Ok(scripts.activate(form, field, action)))
    }

    /// `recalculate` in a transaction of its own.
    pub fn recalculate(&self, form: Rc<Value>) -> Promise<FormScriptResult> {
        self.transaction(move |scripts| Ok(scripts.recalculate(form)))
    }
}

enum Run {
    Output(std::boxed::Box<ScriptOutput>),
    Error(ScriptExecutionError),
}

/// The accumulators `run` closes over.
struct Pass {
    txn: ScriptTransaction,
    page_number: f64,
    budget: ScriptBudget,
    execution_ms: f64,
    aggregate_output_size: usize,
    ui_effects: Vec<FormUiEffect>,
    diagnostics: Vec<ScriptDiagnostic>,
}

impl Pass {
    fn world(&self, fields: &[ScriptFieldInput], event: ScriptEventInput) -> ScriptWorldInput {
        ScriptWorldInput {
            fields: fields.to_vec(),
            annots: None,
            annot_pages: None,
            annots_cover_document: None,
            page_number: Some(self.page_number),
            event,
        }
    }

    /// `consume(output, phase)`.
    fn consume(
        &mut self,
        output: &ScriptOutput,
        phase: ScriptPhase,
    ) -> Option<ScriptExecutionError> {
        let text = crate::value::json_stringify(&epdf_kernel::js::to_value(output));
        self.aggregate_output_size += utf8_length(&text);
        self.ui_effects.extend(output.ui_effects.iter().map(|effect| FormUiEffect {
            effect: effect.clone(),
            phase,
            origin: None,
        }));
        self.diagnostics.extend(output.diagnostics.iter().cloned());
        if self.aggregate_output_size as f64 > self.budget.max_output_bytes {
            return Some(budget_error(format!(
                "Form script transaction output exceeded {} bytes",
                epdf_geometry::js::number_to_string(self.budget.max_output_bytes)
            )));
        }
        output.error.clone()
    }

    /// `remainingBudget()`.
    fn remaining_budget(&self) -> Option<ScriptBudget> {
        let remaining = self.budget.max_execution_ms - self.execution_ms;
        if remaining <= 0.0 {
            return None;
        }
        Some(ScriptBudget { max_execution_ms: remaining, ..self.budget.clone() })
    }

    /// `run(source, input)`.
    async fn run(&mut self, source: &str, input: ScriptWorldInput) -> Run {
        let Some(remaining) = self.remaining_budget() else {
            return Run::Error(budget_error("Form script transaction timed out"));
        };
        // H4: wall time, not the virtual clock.
        let started_at = rt::monotonic_now();
        let output = match self.txn.run(source, input, Some(remaining)).await {
            Ok(output) => output,
            Err(error) => return Run::Error(script_error(error.message)),
        };
        self.execution_ms += rt::monotonic_now() - started_at;
        match self.consume(&output, ScriptPhase::User) {
            Some(error) => Run::Error(error),
            None => Run::Output(std::boxed::Box::new(output)),
        }
    }

    fn failed(&mut self, error: ScriptExecutionError) -> FormScriptResult {
        FormScriptResult {
            status: ScriptStatus::Failed,
            effects: Vec::new(),
            boot_effects: Vec::new(),
            ui_effects: std::mem::take(&mut self.ui_effects),
            diagnostics: std::mem::take(&mut self.diagnostics),
            error: Some(error),
        }
    }

    /// The value is refused; the open scripts' effects are still written.
    fn rejected(&mut self, boot_effects: Vec<Value>) -> FormScriptResult {
        FormScriptResult {
            status: ScriptStatus::Rejected,
            effects: Vec::new(),
            boot_effects,
            ui_effects: std::mem::take(&mut self.ui_effects),
            diagnostics: std::mem::take(&mut self.diagnostics),
            error: None,
        }
    }
}

fn failed_now(error: ScriptExecutionError) -> FormScriptResult {
    FormScriptResult {
        status: ScriptStatus::Failed,
        effects: Vec::new(),
        boot_effects: Vec::new(),
        ui_effects: Vec::new(),
        diagnostics: Vec::new(),
        error: Some(error),
    }
}

fn event(kind: ScriptEventKind) -> ScriptEventInput {
    ScriptEventInput::of(kind)
}

/// `javaScriptProgramFromActionTree(tree)`, a throw as a script error.
fn program_of(tree: &Value) -> Result<String, ScriptExecutionError> {
    java_script_program_from_action_tree(tree).map_err(|error| script_error(error.message))
}

/// `this.run(txn, snapshot, refInput, proposed, activation)`.
async fn run(
    inner: Rc<Inner>,
    txn: ScriptTransaction,
    snapshot: Rc<Value>,
    ref_input: Option<Value>,
    proposed: Option<Value>,
    activation: Option<Value>,
) -> Result<FormScriptResult, Error> {
    let calculation_order = items(&snapshot, "calculationOrder").to_vec();
    // Recalculate's anchor: the first live `/CO` field, else the first field.
    let asked = ref_input.or_else(|| {
        calculation_order
            .iter()
            .find(|c| !c.is_null() && snapshot_field(&snapshot, c).is_some())
            .cloned()
            .or_else(|| items(&snapshot, "fields").first().and_then(|f| get(f, "ref")).cloned())
    });
    let Some(asked) = asked else {
        return Ok(FormScriptResult {
            status: ScriptStatus::Accepted,
            effects: Vec::new(),
            boot_effects: Vec::new(),
            ui_effects: Vec::new(),
            diagnostics: Vec::new(),
            error: None,
        });
    };
    let Some(target) = snapshot_field(&snapshot, &asked).cloned() else {
        return Ok(failed_now(script_error(format!(
            "Form field '{}' no longer exists",
            ref_key(&asked)
        ))));
    };
    // The scripts know the field by its own ref, whichever ref asked for it.
    let field_ref = get(&target, "ref").cloned().unwrap_or(Value::Null);

    let original = script_fields_from_snapshot(&snapshot)?;
    let mut overlay = Overlay {
        fields: original.clone(),
        original,
        reset_keys: Default::default(),
        appearances: Default::default(),
    };
    let Some(target_index) = field_index(&overlay.fields, &field_ref) else {
        return Ok(failed_now(script_error(format!(
            "Form field '{}' has no script view",
            ref_key(&field_ref)
        ))));
    };

    let has_proposed_value = proposed.is_some();
    let mut proposed_value = overlay.fields[target_index].value.clone();
    if let Some(proposed) = proposed.as_ref().filter(|p| truthy(Some(p))) {
        match script_value_from_form_value(&overlay.fields[target_index], proposed) {
            Ok(value) => proposed_value = Some(value),
            Err(error) => return Ok(failed_now(script_error(error.message))),
        }
    }

    let meta = (inner.document)();
    let mut pass = Pass {
        txn: txn.clone(),
        page_number: page_number_for(meta.as_ref(), &target),
        budget: inner.budget.clone().unwrap_or(DEFAULT_SCRIPT_BUDGET),
        execution_ms: 0.0,
        aggregate_output_size: 0,
        ui_effects: Vec::new(),
        diagnostics: Vec::new(),
    };

    // Boot runs once per realm and can only degrade, never brick.
    {
        let world = pass.world(&overlay.fields, event(ScriptEventKind::NameTreeBoot));
        let boot = match txn.boot(world, Some(pass.budget.clone())).await {
            Ok(boot) => boot,
            Err(error) => {
                pass.diagnostics.push(script_diagnostic(format!(
                    "Document boot script failed (continuing without it): {}",
                    error.message
                )));
                None
            }
        };
        if let Some(boot) = boot {
            match pass.consume(&boot, ScriptPhase::Boot) {
                None => apply_effects(&mut overlay, &boot.form_effects)?,
                Some(boot_error) => pass.diagnostics.push(script_diagnostic(format!(
                    "Document boot script failed (continuing without it): {}",
                    boot_error.message
                ))),
            }
        }
    }

    let boot_effects = canonical_effects(&overlay);

    if let Some(activation) = &activation {
        let program = match program_of(activation) {
            Ok(program) => program,
            Err(error) => return Ok(pass.failed(error)),
        };
        if !program.is_empty() {
            let mut e = event(ScriptEventKind::WidgetActivate);
            e.target = Some(field_ref.clone());
            e.source = Some(field_ref.clone());
            e.value = overlay.fields[target_index].value.clone();
            let world = pass.world(&overlay.fields, e);
            match rt::returned(pass.run(&program, world)).await {
                Run::Error(error) => return Ok(pass.failed(error)),
                Run::Output(output) => apply_effects(&mut overlay, &output.form_effects)?,
            }
        }
    }

    let actions = get(&target, "actions");
    let tree_of =
        |key: &str| actions.and_then(|a| get(a, key)).filter(|t| truthy(Some(t))).cloned();
    let text_field = crate::value::str_of(&target, "family") == Some("text");

    if activation.is_none()
        && has_proposed_value
        && text_field
        && let Some(keystroke) = tree_of("keystroke")
    {
        let program = match program_of(&keystroke) {
            Ok(program) => program,
            Err(error) => return Ok(pass.failed(error)),
        };
        if !program.is_empty() {
            // Typing compressed into one paste-shaped replacement, then the
            // Acrobat-faithful willCommit pass.
            let old_value = string_or_empty(overlay.fields[target_index].value.as_ref());
            let mut e = event(ScriptEventKind::FieldKeystroke);
            e.target = Some(field_ref.clone());
            e.source = Some(field_ref.clone());
            e.value = Some(json!(old_value));
            e.change = Some(string_or_empty(proposed_value.as_ref()));
            e.sel_start = Some(0.0);
            e.sel_end = Some(old_value.encode_utf16().count() as f64);
            e.will_commit = Some(false);
            let world = pass.world(&overlay.fields, e);
            match rt::returned(pass.run(&program, world)).await {
                Run::Error(error) if error.kind != ScriptExecutionErrorKind::Exception => {
                    return Ok(pass.failed(error));
                }
                Run::Error(error) => pass.diagnostics.push(script_diagnostic(format!(
                    "Keystroke script failed (kept the typed value): {}",
                    error.message
                ))),
                Run::Output(output) => {
                    if !output.event.rc {
                        return Ok(pass.rejected(boot_effects));
                    }
                    apply_effects(&mut overlay, &output.form_effects)?;
                    let ev = &output.event;
                    let base = string_or_empty(Some(&ev.value));
                    proposed_value = Some(json!(format!(
                        "{}{}{}",
                        utf16_slice(&base, 0.0, Some(ev.sel_start)),
                        ev.change,
                        utf16_slice(&base, epdf_geometry::js::max(ev.sel_start, ev.sel_end), None)
                    )));
                }
            }
            let mut e = event(ScriptEventKind::FieldKeystroke);
            e.target = Some(field_ref.clone());
            e.source = Some(field_ref.clone());
            e.value = proposed_value.clone();
            e.change = Some(String::new());
            e.sel_start = Some(0.0);
            e.sel_end = Some(0.0);
            e.will_commit = Some(true);
            let world = pass.world(&overlay.fields, e);
            match rt::returned(pass.run(&program, world)).await {
                Run::Error(error) if error.kind != ScriptExecutionErrorKind::Exception => {
                    return Ok(pass.failed(error));
                }
                Run::Error(error) => pass.diagnostics.push(script_diagnostic(format!(
                    "Keystroke commit script failed (kept the typed value): {}",
                    error.message
                ))),
                Run::Output(output) => {
                    if !output.event.rc {
                        return Ok(pass.rejected(boot_effects));
                    }
                    apply_effects(&mut overlay, &output.form_effects)?;
                    proposed_value = Some(output.event.value.clone());
                }
            }
        }
    }

    if activation.is_none() && has_proposed_value {
        overlay.fields[target_index].value = proposed_value.clone();
        overlay.reset_keys.remove(&ref_key(&field_ref));
    }

    if activation.is_none()
        && has_proposed_value
        && let Some(validate) = tree_of("validate")
    {
        let program = match program_of(&validate) {
            Ok(program) => program,
            Err(error) => return Ok(pass.failed(error)),
        };
        if !program.is_empty() {
            let mut e = event(ScriptEventKind::FieldValidate);
            e.target = Some(field_ref.clone());
            e.source = Some(field_ref.clone());
            e.value = proposed_value.clone();
            e.will_commit = Some(true);
            let world = pass.world(&overlay.fields, e);
            match rt::returned(pass.run(&program, world)).await {
                Run::Error(error) if error.kind != ScriptExecutionErrorKind::Exception => {
                    return Ok(pass.failed(error));
                }
                Run::Error(error) => pass.diagnostics.push(script_diagnostic(format!(
                    "Validate script failed (accepted the value): {}",
                    error.message
                ))),
                Run::Output(output) => {
                    if !output.event.rc {
                        return Ok(pass.rejected(boot_effects));
                    }
                    overlay.fields[target_index].value = Some(output.event.value.clone());
                    apply_effects(&mut overlay, &output.form_effects)?;
                }
            }
        }
    }

    let mut format_refs: Vec<Value> = if activation.is_none() && has_proposed_value {
        vec![field_ref.clone()]
    } else {
        Vec::new()
    };
    let chain: &[Value] = if activation.is_some() { &[] } else { &calculation_order };
    for calculation_ref in chain {
        if calculation_ref.is_null() {
            continue;
        }
        let calculated = snapshot_field(&snapshot, calculation_ref);
        let tree = calculated
            .and_then(|c| get(c, "actions"))
            .and_then(|a| get(a, "calculate"))
            .filter(|t| truthy(Some(t)));
        let (Some(_), Some(tree)) = (calculated, tree) else { continue };
        let program = match program_of(tree) {
            Ok(program) => program,
            Err(error) => return Ok(pass.failed(error)),
        };
        let calculated_index = field_index(&overlay.fields, calculation_ref);
        if let (false, Some(ci)) = (program.is_empty(), calculated_index) {
            let mut e = event(ScriptEventKind::FieldCalculate);
            e.target = Some(calculation_ref.clone());
            if has_proposed_value {
                e.source = Some(field_ref.clone());
            }
            e.value = overlay.fields[ci].value.clone();
            let world = pass.world(&overlay.fields, e);
            match rt::returned(pass.run(&program, world)).await {
                Run::Error(error) if error.kind != ScriptExecutionErrorKind::Exception => {
                    return Ok(pass.failed(error));
                }
                Run::Error(error) => pass.diagnostics.push(script_diagnostic(format!(
                    "Calculate script failed (field left unchanged): {}",
                    error.message
                ))),
                Run::Output(output) => apply_effects(&mut overlay, &output.form_effects)?,
            }
        }
        if !format_refs.iter().any(|candidate| same_ref(candidate, calculation_ref)) {
            format_refs.push(calculation_ref.clone());
        }
    }

    for format_ref in &format_refs {
        let field = snapshot_field(&snapshot, format_ref);
        let tree = field
            .and_then(|f| get(f, "actions"))
            .and_then(|a| get(a, "format"))
            .filter(|t| truthy(Some(t)));
        let overlay_index = field_index(&overlay.fields, format_ref);
        let (Some(_), Some(tree), Some(oi)) = (field, tree, overlay_index) else { continue };
        let program = match program_of(tree) {
            Ok(program) => program,
            Err(error) => return Ok(pass.failed(error)),
        };
        if program.is_empty() {
            continue;
        }
        let mut e = event(ScriptEventKind::FieldFormat);
        e.target = Some(format_ref.clone());
        if has_proposed_value {
            e.source = Some(field_ref.clone());
        }
        e.value = overlay.fields[oi].value.clone();
        let world = pass.world(&overlay.fields, e);
        match rt::returned(pass.run(&program, world)).await {
            Run::Error(error) if error.kind != ScriptExecutionErrorKind::Exception => {
                return Ok(pass.failed(error));
            }
            Run::Error(error) => {
                pass.diagnostics.push(script_diagnostic(format!(
                    "Format script failed (kept the raw value): {}",
                    error.message
                )));
                continue;
            }
            Run::Output(output) => apply_effects(&mut overlay, &output.form_effects)?,
        }
    }

    let effects = canonical_effects(&overlay);
    if (effects.len() + pass.ui_effects.len()) as f64 > pass.budget.max_effects {
        let message = format!(
            "Form script transaction exceeded {} effects",
            epdf_geometry::js::number_to_string(pass.budget.max_effects)
        );
        return Ok(pass.failed(budget_error(message)));
    }
    Ok(FormScriptResult {
        status: ScriptStatus::Accepted,
        effects,
        boot_effects,
        ui_effects: pass.ui_effects,
        diagnostics: pass.diagnostics,
        error: None,
    })
}
