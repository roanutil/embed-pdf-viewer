//! `scripting.ts`: the K/V/C/F transaction. One commit runs, inside one
//! realm transaction, the document boot (once per realm), the keystroke
//! pair, validate, the `/CO` calculation chain and format, over a detached
//! field overlay, then commits the canonical effects as one engine batch.
//!
//! The realm is either the actions plugin's shared one (its
//! `script_transaction` port) or a standalone one this controller builds and
//! owns from [`FormScriptingOptions`] (stamp's detached documents). The
//! sandbox is a host capability: Rust has no default, so the standalone
//! options require a factory.

use std::cell::Cell;
use std::rc::Rc;

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, ScriptBudget, ScriptDiagnostic, ScriptDiagnosticCode,
    ScriptDocumentInput, ScriptEnvironment, ScriptEventInput, ScriptEventKind,
    ScriptExecutionError, ScriptExecutionErrorKind, ScriptFieldInput, ScriptHost,
    ScriptHostOptions, ScriptOutput, ScriptTransaction, ScriptWorldInput,
    java_script_program_from_action_tree, resolve_script_identity, script_fields_from_snapshot,
    seed_from,
};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::types::DocumentMeta;
use epdf_kernel::{Error, undefined};
use epdf_plugin_actions::ScriptPhase;
use serde_json::{Value, json};

use crate::promise::{adopt, adopted, await_engine};
use crate::types::{FormCommitResult, FormCommitStatus, FormScriptingOptions, FormUiEffect};
use crate::value::{get, json_stringify, num, str_of, strict_eq, string_of, truthy, utf16_slice};

/// A transaction body: what runs while the realm is held.
pub type TransactionBody =
    Box<dyn FnOnce(ScriptTransaction) -> Result<Promise<FormCommitResult>, Error>>;

/// The realm mutex: `transaction(body)`.
pub type TransactionPort = Rc<dyn Fn(TransactionBody) -> Promise<FormCommitResult>>;

/// `FormScriptingControllerOptions`.
pub enum FormScriptingControllerOptions {
    /// The shared realm's transaction port (the actions plugin's host).
    Transaction {
        doc: Rc<dyn DocumentHandle>,
        document: Rc<dyn Fn() -> Option<DocumentMeta>>,
        transaction: TransactionPort,
        budget: Option<ScriptBudget>,
    },
    /// A standalone realm the controller builds and owns.
    Config {
        doc: Rc<dyn DocumentHandle>,
        document: Rc<dyn Fn() -> Option<DocumentMeta>>,
        config: FormScriptingOptions,
    },
}

/// What [`create_form_scripting_host`] returns.
#[derive(Clone)]
pub struct FormScriptingHost {
    pub transaction: TransactionPort,
    host: ScriptHost,
}

impl FormScriptingHost {
    pub fn dispose(&self) {
        self.host.dispose();
    }
}

/// `createFormScriptingHost({doc, document, config})`: a standalone realm
/// for one document.
pub fn create_form_scripting_host(
    doc: Rc<dyn DocumentHandle>,
    document: Rc<dyn Fn() -> Option<DocumentMeta>>,
    config: FormScriptingOptions,
) -> FormScriptingHost {
    let document_input = {
        let (doc, config) = (doc.clone(), config.clone());
        Rc::new(move || {
            let meta = document();
            ScriptDocumentInput {
                id: doc.id(),
                file_name: match &config.file_name {
                    Some(f) => f(),
                    None => meta.as_ref().and_then(|m| m.name.clone()).unwrap_or_else(|| doc.id()),
                },
                page_count: meta.map_or(0.0, |m| f64::from(m.page_count)),
                page_number: 0.0,
            }
        })
    };
    let identity = {
        let (doc, overrides) = (doc.clone(), config.identity.clone());
        Rc::new(move || resolve_script_identity(doc.as_ref(), overrides.as_ref()))
    };
    let environment = {
        let (doc, config) = (doc.clone(), config.clone());
        Rc::new(move |sequence: f64| {
            let now_ms = config.now.as_ref().map_or_else(rt::now, |f| f());
            ScriptEnvironment {
                now_ms,
                utc_offset_minutes: match &config.utc_offset_minutes {
                    Some(f) => f(),
                    None => config.host_utc_offset_minutes.as_ref().map_or(0.0, |f| f(now_ms)),
                },
                random_seed: match &config.random_seed {
                    Some(f) => f(),
                    None => seed_from(&doc.id(), sequence),
                },
            }
        })
    };
    let boot_sources = {
        let doc = doc.clone();
        Rc::new(move || {
            let read = doc.has("actions").then(|| doc.call("actions.read", None, Vec::new()));
            Ok(Promise::spawn(async move {
                let Some(read) = read else { return Ok(Vec::new()) };
                let actions = await_engine(read).await?;
                if actions.is_null() {
                    return Ok(Vec::new());
                }
                get(&actions, "nameTreeScripts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|entry| {
                        java_script_program_from_action_tree(
                            get(entry, "action").unwrap_or(&Value::Null),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
            }))
        })
    };
    let host = ScriptHost::new(ScriptHostOptions {
        sandbox_factory: config.sandbox_factory.clone(),
        document: document_input,
        identity,
        environment,
        boot_sources,
        budget: config.budget.clone(),
    });
    let transaction: TransactionPort = {
        let host = host.clone();
        Rc::new(move |body: TransactionBody| host.transaction(body))
    };
    FormScriptingHost { transaction, host }
}

// ── the overlay ───────────────────────────────────────────────────────────────

struct Appearance {
    key: String,
    field_ref: Value,
    text: Value,
}

struct Overlay {
    original: Vec<ScriptFieldInput>,
    fields: Vec<ScriptFieldInput>,
    reset_keys: Vec<String>,
    appearances: Vec<Appearance>,
}

impl Overlay {
    fn add_reset(&mut self, key: String) {
        if !self.reset_keys.contains(&key) {
            self.reset_keys.push(key);
        }
    }

    fn remove_reset(&mut self, key: &str) {
        self.reset_keys.retain(|k| k != key);
    }
}

/// `refKey(ref)`.
pub fn ref_key(r: &Value) -> String {
    if str_of(r, "kind") == Some("objectNumber") {
        format!("obj:{}", string_of(get(r, "fieldObjectNumber")))
    } else {
        format!("fqn:{}", string_of(get(r, "name")))
    }
}

fn same_ref(left: &Value, right: &Value) -> bool {
    ref_key(left) == ref_key(right)
}

/// `sameValue(left, right)`: arrays element-wise with `===`, anything else
/// with `===`.
fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| strict_eq(Some(x), Some(y)))
        }
        _ => strict_eq(left, right),
    }
}

fn field_index(fields: &[ScriptFieldInput], r: &Value) -> Option<usize> {
    fields.iter().position(|field| same_ref(&field.field_ref, r))
}

fn snapshot_fields(snapshot: &Value) -> &[Value] {
    get(snapshot, "fields").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn snapshot_field<'a>(snapshot: &'a Value, r: &Value) -> Option<&'a Value> {
    snapshot_fields(snapshot)
        .iter()
        .find(|field| same_ref(get(field, "ref").unwrap_or(&Value::Null), r))
}

/// `scriptValueFromFormValue(field, value)`. `None` is `undefined`.
fn script_value_from_form_value(
    field: &ScriptFieldInput,
    value: &Value,
) -> Result<Option<Value>, Error> {
    let family = field.family.as_str();
    let kind = str_of(value, "type");
    if family == "text" && kind == Some("text") {
        return Ok(get(value, "value").cloned());
    }
    if (family == "checkbox" || family == "radio") && kind == Some("toggle") {
        return Ok(Some(match get(value, "state") {
            Some(state) if !state.is_null() => state.clone(),
            _ => json!("Off"),
        }));
    }
    let values = || get(value, "values").and_then(Value::as_array).cloned().unwrap_or_default();
    if family == "combobox" && kind == Some("choice") {
        return Ok(Some(values().into_iter().next().filter(|v| !v.is_null()).unwrap_or(json!(""))));
    }
    if family == "listbox" && kind == Some("choice") {
        return Ok(Some(Value::Array(values())));
    }
    Err(Error::new(format!(
        "Form value type '{}' does not match field family '{}'",
        string_of(get(value, "type")),
        field.family
    )))
}

/// `String(value ?? '')`.
fn string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) => string_of(Some(v)),
    }
}

/// `formValueFromScriptValue(field)`.
fn form_value_from_script_value(field: &ScriptFieldInput) -> Option<Value> {
    let value = field.value.as_ref().filter(|v| !epdf_kernel::is_undefined(v));
    match field.family.as_str() {
        "text" => Some(json!({ "type": "text", "value": string_or_empty(value) })),
        "checkbox" | "radio" => {
            let state = match value {
                None | Some(Value::Null) => Value::Null,
                Some(v) if string_of(Some(v)) == "Off" => Value::Null,
                Some(v) => json!(string_of(Some(v))),
            };
            Some(json!({ "type": "toggle", "state": state }))
        }
        "combobox" => {
            let values = match value {
                Some(Value::Array(items)) => items.iter().take(1).cloned().collect(),
                _ => vec![json!(string_or_empty(value))],
            };
            Some(json!({ "type": "choice", "values": values }))
        }
        "listbox" => {
            let values = match value {
                Some(Value::Array(items)) => items.clone(),
                _ => vec![json!(string_or_empty(value))],
            };
            Some(json!({ "type": "choice", "values": values }))
        }
        _ => None,
    }
}

/// `applyEffects(overlay, effects)`: fold script effects into the overlay.
fn apply_effects(overlay: &mut Overlay, effects: &[Value]) -> Result<(), Error> {
    for effect in effects {
        if str_of(effect, "kind") == Some("reset") {
            for r in get(effect, "refs").and_then(Value::as_array).into_iter().flatten() {
                let Some(i) = field_index(&overlay.fields, r) else { continue };
                let field = &mut overlay.fields[i];
                field.value = field.default_value.clone();
                overlay.add_reset(ref_key(r));
            }
            continue;
        }
        let r = get(effect, "ref").cloned().unwrap_or_else(undefined);
        let Some(i) = field_index(&overlay.fields, &r) else { continue };
        let key = ref_key(&r);
        match str_of(effect, "kind") {
            Some("setValue") => {
                let value = get(effect, "value").cloned().unwrap_or_else(undefined);
                overlay.fields[i].value = script_value_from_form_value(&overlay.fields[i], &value)?;
                overlay.remove_reset(&key);
            }
            Some("setDisplay") => {
                overlay.fields[i].display = string_of(get(effect, "display"));
            }
            _ => {
                let text = get(effect, "text").cloned().unwrap_or_else(undefined);
                match overlay.appearances.iter_mut().find(|a| a.key == key) {
                    Some(existing) => {
                        existing.field_ref = r;
                        existing.text = text;
                    }
                    None => overlay.appearances.push(Appearance { key, field_ref: r, text }),
                }
            }
        }
    }
    Ok(())
}

/// `canonicalEffects(overlay)`: the overlay's difference from the original,
/// resets first, appearance text last.
fn canonical_effects(overlay: &Overlay) -> Vec<Value> {
    let mut effects = Vec::new();
    let mut resets = Vec::new();
    for field in &overlay.fields {
        let Some(i) = field_index(&overlay.original, &field.field_ref) else { continue };
        let original = &overlay.original[i];
        if !same_value(original.value.as_ref(), field.value.as_ref()) {
            let key = ref_key(&field.field_ref);
            if overlay.reset_keys.contains(&key)
                && same_value(field.value.as_ref(), field.default_value.as_ref())
            {
                resets.push(field.field_ref.clone());
            } else if let Some(value) = form_value_from_script_value(field) {
                effects.push(json!({ "kind": "setValue", "ref": field.field_ref, "value": value }));
            }
        }
        if original.display != field.display {
            effects.push(
                json!({ "kind": "setDisplay", "ref": field.field_ref, "display": field.display }),
            );
        }
    }
    if !resets.is_empty() {
        effects.insert(0, json!({ "kind": "reset", "refs": resets }));
    }
    for a in &overlay.appearances {
        effects.push(json!({ "kind": "setAppearanceText", "ref": a.field_ref, "text": a.text }));
    }
    effects
}

fn script_error(message: impl Into<String>) -> ScriptExecutionError {
    ScriptExecutionError {
        kind: ScriptExecutionErrorKind::InvalidOutput,
        message: message.into(),
        stack: None,
    }
}

fn budget_error(message: impl Into<String>) -> ScriptExecutionError {
    ScriptExecutionError {
        kind: ScriptExecutionErrorKind::Budget,
        message: message.into(),
        stack: None,
    }
}

fn script_diagnostic(message: String) -> ScriptDiagnostic {
    ScriptDiagnostic { code: ScriptDiagnosticCode::ScriptError, message }
}

/// `pageNumberFor(meta, field)`: the zero-based page of the field's first
/// placed widget, 0 when there is none or the page is unknown.
fn page_number_for(meta: Option<&DocumentMeta>, field: &Value) -> f64 {
    let pon = get(field, "widgets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| num(w, "pageObjectNumber"))
        .find(|&pon| pon > 0.0);
    let (Some(meta), Some(pon)) = (meta, pon) else { return 0.0 };
    let index = meta
        .pages
        .as_array()
        .and_then(|pages| pages.iter().position(|p| num(p, "pageObjectNumber") == Some(pon)));
    index.map_or(0.0, |i| i as f64)
}

/// `statusFromEffects(result)`.
fn status_from_effects(result: &Value) -> FormCommitStatus {
    let statuses: Vec<&str> = get(result, "results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| str_of(entry, "status").unwrap_or_default())
        .collect();
    if statuses.iter().any(|s| *s == "failed" || *s == "skipped") {
        return FormCommitStatus::Failed;
    }
    if statuses.contains(&"rejected") {
        return FormCommitStatus::Failed;
    }
    if statuses.contains(&"applied") {
        FormCommitStatus::Applied
    } else {
        FormCommitStatus::Unchanged
    }
}

fn failed(
    ui_effects: Vec<FormUiEffect>,
    diagnostics: Vec<ScriptDiagnostic>,
    error: ScriptExecutionError,
) -> FormCommitResult {
    FormCommitResult {
        status: FormCommitStatus::Failed,
        scripted: true,
        effects_result: None,
        ui_effects,
        diagnostics,
        error: Some(error),
    }
}

// ── the controller ────────────────────────────────────────────────────────────

struct ControllerInner {
    doc: Rc<dyn DocumentHandle>,
    document: Rc<dyn Fn() -> Option<DocumentMeta>>,
    transaction: TransactionPort,
    transaction_budget: Option<ScriptBudget>,
    owned_host: Option<FormScriptingHost>,
    disposed: Cell<bool>,
}

/// `FormScriptingController`.
#[derive(Clone)]
pub struct FormScriptingController(Rc<ControllerInner>);

/// `createFormScriptingController(options)`.
pub fn create_form_scripting_controller(
    options: FormScriptingControllerOptions,
) -> FormScriptingController {
    FormScriptingController::new(options)
}

enum Run {
    Output(std::boxed::Box<ScriptOutput>),
    Error(ScriptExecutionError),
}

/// The per-transaction accumulators the TypeScript closes over.
struct Pass {
    txn: ScriptTransaction,
    page_number: f64,
    budget: ScriptBudget,
    execution_ms: f64,
    aggregate_output_size: f64,
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
        let size = json_stringify(&serde_json::to_value(output).unwrap_or(Value::Null)).len();
        self.aggregate_output_size += size as f64;
        self.ui_effects.extend(output.ui_effects.iter().map(|effect| FormUiEffect {
            effect: effect.clone(),
            phase,
            origin: None,
        }));
        self.diagnostics.extend(output.diagnostics.iter().cloned());
        if self.aggregate_output_size > self.budget.max_output_bytes {
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
        let started_at = rt::now();
        let output = match self.txn.run(source, input, Some(remaining)).await {
            Ok(output) => output,
            Err(error) => return Run::Error(script_error(error.message)),
        };
        self.execution_ms += rt::now() - started_at;
        match self.consume(&output, ScriptPhase::User) {
            Some(error) => Run::Error(error),
            None => Run::Output(std::boxed::Box::new(output)),
        }
    }

    fn failed(&mut self, error: ScriptExecutionError) -> FormCommitResult {
        failed(std::mem::take(&mut self.ui_effects), std::mem::take(&mut self.diagnostics), error)
    }
}

fn event(kind: ScriptEventKind) -> ScriptEventInput {
    ScriptEventInput::of(kind)
}

impl FormScriptingController {
    pub fn new(options: FormScriptingControllerOptions) -> Self {
        let inner = match options {
            FormScriptingControllerOptions::Transaction { doc, document, transaction, budget } => {
                ControllerInner {
                    doc,
                    document,
                    transaction,
                    transaction_budget: budget,
                    owned_host: None,
                    disposed: Cell::new(false),
                }
            }
            FormScriptingControllerOptions::Config { doc, document, config } => {
                let budget = config.budget.clone();
                let host = create_form_scripting_host(doc.clone(), document.clone(), config);
                ControllerInner {
                    doc,
                    document,
                    transaction: host.transaction.clone(),
                    transaction_budget: budget,
                    owned_host: Some(host),
                    disposed: Cell::new(false),
                }
            }
        };
        FormScriptingController(Rc::new(inner))
    }

    /// A shared realm belongs to its host: this only fences further
    /// transactions. A standalone realm is released.
    pub fn dispose(&self) {
        self.0.disposed.set(true);
        if let Some(host) = &self.0.owned_host {
            host.dispose();
        }
    }

    /// `commit(ref, proposed)`: one originating-client value transaction.
    pub fn commit(&self, field_ref: Value, proposed: Value) -> Promise<FormCommitResult> {
        let this = self.clone();
        let proposed = Some(proposed).filter(|v| !epdf_kernel::is_undefined(v));
        Promise::spawn(async move { adopt(this.transact(Some(field_ref), proposed, None)).await })
    }

    /// `activate(ref, action)`: one widget's activation action.
    pub fn activate(&self, field_ref: Value, action: Value) -> Promise<FormCommitResult> {
        let this = self.clone();
        Promise::spawn(
            async move { adopt(this.transact(Some(field_ref), None, Some(action))).await },
        )
    }

    /// `recalculate()`: the lazy boot plus the `/CO` chain, with no user
    /// field change. The anchor resolves inside the transaction.
    pub fn recalculate(&self) -> Promise<FormCommitResult> {
        let this = self.clone();
        Promise::spawn(async move { adopt(this.transact(None, None, None)).await })
    }

    fn transact(
        &self,
        ref_input: Option<Value>,
        proposed: Option<Value>,
        activation: Option<Value>,
    ) -> Promise<FormCommitResult> {
        let inner = self.0.clone();
        Promise::spawn(async move {
            if inner.disposed.get() {
                return Err(Error::new("Form scripting controller is disposed"));
            }
            let body_inner = inner.clone();
            let body: TransactionBody = Box::new(move |txn| {
                Ok(Promise::spawn(transact_body(body_inner, txn, ref_input, proposed, activation)))
            });
            adopt((inner.transaction)(body)).await
        })
    }
}

/// `javaScriptProgramFromActionTree(tree)`, a throw as a script error.
fn program_of(tree: &Value) -> Result<String, ScriptExecutionError> {
    java_script_program_from_action_tree(tree).map_err(|error| script_error(error.message))
}

async fn transact_body(
    inner: Rc<ControllerInner>,
    txn: ScriptTransaction,
    ref_input: Option<Value>,
    proposed: Option<Value>,
    activation: Option<Value>,
) -> Result<FormCommitResult, Error> {
    let snapshot = await_engine(inner.doc.call("forms.list", None, Vec::new())).await?;
    let calculation_order: Vec<Value> =
        get(&snapshot, "calculationOrder").and_then(Value::as_array).cloned().unwrap_or_default();
    let field_ref = ref_input.filter(|r| !r.is_null()).or_else(|| {
        calculation_order
            .iter()
            .find(|c| !c.is_null() && snapshot_field(&snapshot, c).is_some())
            .cloned()
            .or_else(|| snapshot_fields(&snapshot).first().and_then(|f| get(f, "ref")).cloned())
    });
    let Some(field_ref) = field_ref else {
        return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, true));
    };
    let Some(target) = snapshot_field(&snapshot, &field_ref).cloned() else {
        return Ok(failed(
            Vec::new(),
            Vec::new(),
            script_error(format!("Form field '{}' no longer exists", ref_key(&field_ref))),
        ));
    };

    let original = script_fields_from_snapshot(&snapshot)?;
    let mut overlay = Overlay {
        fields: original.clone(),
        original,
        reset_keys: Vec::new(),
        appearances: Vec::new(),
    };
    let Some(target_index) = field_index(&overlay.fields, &field_ref) else {
        return Ok(failed(
            Vec::new(),
            Vec::new(),
            script_error(format!("Form field '{}' has no script view", ref_key(&field_ref))),
        ));
    };

    let has_proposed_value = proposed.is_some();
    let mut proposed_value = overlay.fields[target_index].value.clone();
    if let Some(proposed) = proposed.as_ref().filter(|p| truthy(Some(p))) {
        match script_value_from_form_value(&overlay.fields[target_index], proposed) {
            Ok(value) => proposed_value = value,
            Err(error) => return Ok(failed(Vec::new(), Vec::new(), script_error(error.message))),
        }
    }

    let meta = (inner.document)();
    let mut pass = Pass {
        txn: txn.clone(),
        page_number: page_number_for(meta.as_ref(), &target),
        budget: inner.transaction_budget.clone().unwrap_or(DEFAULT_SCRIPT_BUDGET),
        execution_ms: 0.0,
        aggregate_output_size: 0.0,
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

    if activation.is_none()
        && has_proposed_value
        && str_of(&target, "family") == Some("text")
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
                        return adopted(finish_rejected(&inner, boot_effects, pass)).await;
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
                        return adopted(finish_rejected(&inner, boot_effects, pass)).await;
                    }
                    apply_effects(&mut overlay, &output.form_effects)?;
                    proposed_value = Some(output.event.value.clone());
                }
            }
        }
    }

    if activation.is_none() && has_proposed_value {
        overlay.fields[target_index].value = proposed_value.clone();
        overlay.remove_reset(&ref_key(&field_ref));
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
                        return adopted(finish_rejected(&inner, boot_effects, pass)).await;
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
    if effects.is_empty() {
        return Ok(FormCommitResult {
            ui_effects: pass.ui_effects,
            diagnostics: pass.diagnostics,
            ..FormCommitResult::bare(FormCommitStatus::Unchanged, true)
        });
    }
    if !inner.doc.has("forms.applyEffects") {
        return Ok(pass.failed(script_error("This engine does not support batched form effects")));
    }

    let effects_result =
        await_engine(inner.doc.call("forms.applyEffects", None, vec![Value::Array(effects)]))
            .await?;
    let status = status_from_effects(&effects_result);
    Ok(FormCommitResult {
        status,
        scripted: true,
        effects_result: Some(effects_result),
        ui_effects: pass.ui_effects,
        diagnostics: pass.diagnostics,
        error: (status == FormCommitStatus::Failed)
            .then(|| script_error("One or more native form effects failed")),
    })
}

/// `finishRejected(bootEffects, uiEffects, diagnostics)`: a rejected value
/// still commits the boot's effects.
async fn finish_rejected(
    inner: &ControllerInner,
    boot_effects: Vec<Value>,
    mut pass: Pass,
) -> Result<FormCommitResult, Error> {
    if boot_effects.is_empty() {
        return Ok(FormCommitResult {
            ui_effects: pass.ui_effects,
            diagnostics: pass.diagnostics,
            ..FormCommitResult::bare(FormCommitStatus::Rejected, true)
        });
    }
    if !inner.doc.has("forms.applyEffects") {
        return Ok(pass.failed(script_error("Batched form effects are unavailable")));
    }
    let effects_result =
        await_engine(inner.doc.call("forms.applyEffects", None, vec![Value::Array(boot_effects)]))
            .await?;
    if status_from_effects(&effects_result) == FormCommitStatus::Failed {
        return Ok(pass.failed(script_error("A name-tree boot effect failed")));
    }
    Ok(FormCommitResult {
        status: FormCommitStatus::Rejected,
        scripted: true,
        effects_result: Some(effects_result),
        ui_effects: pass.ui_effects,
        diagnostics: pass.diagnostics,
        error: None,
    })
}
