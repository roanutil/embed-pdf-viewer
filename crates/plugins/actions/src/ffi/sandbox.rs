//! JavaScript through a foreign engine. `ForeignSandbox` implements
//! `epdf_acrojs::ScriptSandbox` the way `packages/core/js-sandbox/src/
//! QuickJsSandbox.ts` does: it builds the `__acrojsBoot`/`__acrojsRun` call,
//! hands it to a `ScriptEngine` that only evaluates JavaScript, and checks
//! the answer. The engine never sees anything but program text.

use std::cell::Cell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, PRELUDE_SOURCE, ScriptBudget, ScriptDocumentInput, ScriptEnvironment,
    ScriptEventInput, ScriptEventKind, ScriptEventOutput, ScriptExecutionError,
    ScriptExecutionErrorKind, ScriptIdentity, ScriptInput, ScriptOutput, ScriptSandbox,
    ScriptSandboxFactory, ScriptUiEffect,
};
use epdf_ffi::{HostError, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::Value;

/// What one evaluation came to.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptEvaluation {
    /// The program's completion value, which was a string.
    Value { json: String },
    /// It threw, or completed with something that isn't a string.
    Threw { detail: String },
    /// It ran past `time_limit_ms`. The engine is unusable afterwards.
    TimedOut,
}

/// One JavaScript realm: persistent globals across evaluations. Called on
/// the kernel thread, which waits for the answer, so give up at
/// `time_limit_ms`.
#[uniffi::export(with_foreign)]
pub trait ScriptEngine: Send + Sync {
    fn evaluate(&self, program: String, time_limit_ms: u32) -> ScriptEvaluation;
    /// Release the realm. Idempotent.
    fn dispose(&self);
}

/// Makes one realm per document (and per detached realm).
#[uniffi::export(with_foreign)]
pub trait ScriptEngineFactory: Send + Sync {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError>;
}

/// `ScriptBudget` with whole-number fields. A JavaScriptCore engine enforces
/// only `max_execution_ms`; memory and stack limits are JSC's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ActionsScriptBudget {
    pub max_execution_ms: u32,
    pub max_memory_bytes: u64,
    pub max_stack_bytes: u64,
    pub max_effects: u32,
    pub max_output_bytes: u64,
}

impl From<ActionsScriptBudget> for ScriptBudget {
    fn from(b: ActionsScriptBudget) -> Self {
        ScriptBudget {
            max_execution_ms: f64::from(b.max_execution_ms),
            max_memory_bytes: b.max_memory_bytes as f64,
            max_stack_bytes: b.max_stack_bytes as f64,
            max_effects: f64::from(b.max_effects),
            max_output_bytes: b.max_output_bytes as f64,
        }
    }
}

/// `ActionsConfig.javascript`. `enabled` with no `engine` gives the
/// document a realm, as the TypeScript's default VM does, but Rust has no
/// default engine: every script run fails with `NO_DEFAULT_SANDBOX`.
///
/// Equality and hashing are exported (`Eq`, `Hash`) because a record that
/// holds a foreign object loses Swift's synthesized `Equatable`, and
/// `ViewerConfig` (another crate, which bindgen doesn't look into) needs it.
/// The engine compares by presence: UniFFI wraps a foreign object in a new
/// `Arc` on every crossing, so identity would make a config unequal to
/// itself.
#[derive(Clone, uniffi::Record)]
#[uniffi::export(Eq, Hash)]
pub struct ActionsJavaScriptConfig {
    pub enabled: bool,
    #[uniffi(default = None)]
    pub engine: Option<Arc<dyn ScriptEngineFactory>>,
    #[uniffi(default = None)]
    pub budget: Option<ActionsScriptBudget>,
    /// The most JS nodes one dispatch may run. Default 16.
    #[uniffi(default = None)]
    pub max_script_nodes_per_dispatch: Option<u32>,
}

impl fmt::Debug for ActionsJavaScriptConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionsJavaScriptConfig")
            .field("enabled", &self.enabled)
            .field("engine", &self.engine.is_some())
            .field("budget", &self.budget)
            .field("max_script_nodes_per_dispatch", &self.max_script_nodes_per_dispatch)
            .finish()
    }
}

impl PartialEq for ActionsJavaScriptConfig {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.engine.is_some() == other.engine.is_some()
            && self.budget == other.budget
            && self.max_script_nodes_per_dispatch == other.max_script_nodes_per_dispatch
    }
}

impl Eq for ActionsJavaScriptConfig {}

impl Hash for ActionsJavaScriptConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.enabled.hash(state);
        self.engine.is_some().hash(state);
        self.budget.hash(state);
        self.max_script_nodes_per_dispatch.hash(state);
    }
}

impl From<ActionsJavaScriptConfig> for crate::JavaScriptConfig {
    fn from(js: ActionsJavaScriptConfig) -> Self {
        crate::JavaScriptConfig {
            enabled: Some(js.enabled),
            sandbox_factory: js.engine.map(sandbox_factory),
            budget: js.budget.map(Into::into),
            max_script_nodes_per_dispatch: js.max_script_nodes_per_dispatch.map(f64::from),
            ..Default::default()
        }
    }
}

/// The prelude is about 48 KB; loading it is not a script run, so it gets
/// its own limit.
const PRELUDE_LIMIT_MS: u32 = 1_000;

/// Each realm loads the prelude, then proves `__acrojsRun` is installed.
pub fn sandbox_factory(engine: Arc<dyn ScriptEngineFactory>) -> ScriptSandboxFactory {
    Rc::new(move || {
        let engine = match engine.create() {
            Ok(engine) => engine,
            Err(error) => return Promise::rejected(error.into()),
        };
        let program = format!("{PRELUDE_SOURCE}\nJSON.stringify(typeof globalThis.__acrojsRun)");
        let failed = |detail: String| {
            engine.dispose();
            Promise::rejected(Error::new(format!("Failed to install the AcroJS prelude: {detail}")))
        };
        match engine.evaluate(program, PRELUDE_LIMIT_MS) {
            ScriptEvaluation::Value { json } if json == "\"function\"" => Promise::resolved(
                Rc::new(ForeignSandbox::new(engine.clone())) as Rc<dyn ScriptSandbox>,
            ),
            ScriptEvaluation::Value { json } => failed(format!("__acrojsRun is {json}")),
            ScriptEvaluation::Threw { detail } => failed(detail),
            ScriptEvaluation::TimedOut => failed(format!("it ran past {PRELUDE_LIMIT_MS} ms")),
        }
    })
}

struct ForeignSandbox {
    engine: Arc<dyn ScriptEngine>,
    booted: Cell<bool>,
    disposed: Cell<bool>,
}

impl ForeignSandbox {
    fn new(engine: Arc<dyn ScriptEngine>) -> Self {
        ForeignSandbox { engine, booted: Cell::new(false), disposed: Cell::new(false) }
    }

    fn active(&self) -> Result<(), Error> {
        if self.disposed.get() {
            return Err(Error::new("The script sandbox has been disposed"));
        }
        Ok(())
    }

    /// `poison()`: dispose, keep the output.
    fn poisoned(&self, output: ScriptOutput) -> ScriptOutput {
        self.dispose();
        output
    }

    fn invoke(
        &self,
        method: &str,
        first: String,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        let budget = check_budget(budget.unwrap_or(&DEFAULT_SCRIPT_BUDGET))?;
        let input_json = serde_json::to_string(input)
            .map_err(|e| Error::new(format!("[acrojs] input does not serialize: {e}")))?;
        let program = format!(
            "JSON.stringify(globalThis.{method}({first},{input_json},{{\"maxEffects\":{}}}))",
            budget.max_effects
        );
        let json = match self.engine.evaluate(program, budget.max_execution_ms as u32) {
            ScriptEvaluation::TimedOut => {
                return Ok(self.poisoned(failed_output(
                    input,
                    ScriptExecutionErrorKind::Budget,
                    "Script execution exceeded its time budget".into(),
                )));
            }
            ScriptEvaluation::Threw { detail } => {
                return Ok(self.poisoned(failed_output(
                    input,
                    ScriptExecutionErrorKind::InvalidOutput,
                    detail,
                )));
            }
            ScriptEvaluation::Value { json } => json,
        };
        let Ok(output) = serde_json::from_str::<ScriptOutput>(&json) else {
            return Ok(self.poisoned(failed_output(
                input,
                ScriptExecutionErrorKind::InvalidOutput,
                "AcroJS returned an invalid output".into(),
            )));
        };
        if let Some(error) = output.error.as_ref().filter(|e| is_resource_error(e)) {
            let message = error.message.clone();
            return Ok(self.poisoned(failed_output(
                input,
                ScriptExecutionErrorKind::Budget,
                message,
            )));
        }
        if json.len() as f64 > budget.max_output_bytes {
            let message = format!(
                "Script output used {} bytes; limit is {}",
                json.len(),
                budget.max_output_bytes
            );
            return Ok(self.poisoned(failed_output(
                input,
                ScriptExecutionErrorKind::Budget,
                message,
            )));
        }
        Ok(output)
    }
}

impl ScriptSandbox for ForeignSandbox {
    fn disposed(&self) -> bool {
        self.disposed.get()
    }

    fn boot(
        &self,
        sources: &[String],
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.active()?;
        if self.booted.replace(true) {
            return Err(Error::new("The document JavaScript name tree has already booted"));
        }
        let sources = serde_json::to_string(sources).expect("strings serialize");
        self.invoke("__acrojsBoot", sources, input, budget)
    }

    fn run(
        &self,
        source: &str,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.active()?;
        let source = serde_json::to_string(source).expect("a string serializes");
        self.invoke("__acrojsRun", source, input, budget)
    }

    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.engine.dispose();
        }
    }
}

/// `assertBudget`: every field a positive safe integer.
fn check_budget(budget: &ScriptBudget) -> Result<&ScriptBudget, Error> {
    let fields = [
        ("maxExecutionMs", budget.max_execution_ms),
        ("maxMemoryBytes", budget.max_memory_bytes),
        ("maxStackBytes", budget.max_stack_bytes),
        ("maxEffects", budget.max_effects),
        ("maxOutputBytes", budget.max_output_bytes),
    ];
    for (name, value) in fields {
        if !(value > 0.0 && value.fract() == 0.0 && value <= 9_007_199_254_740_991.0) {
            return Err(Error::named(
                "RangeError",
                format!("ScriptBudget.{name} must be a positive safe integer"),
            ));
        }
    }
    Ok(budget)
}

/// `isResourceError`, plus JavaScriptCore's spelling of a stack overflow.
fn is_resource_error(error: &ScriptExecutionError) -> bool {
    let message = error.message.to_lowercase();
    ["interrupted", "out of memory", "stack overflow", "maximum call stack size exceeded"]
        .iter()
        .any(|needle| message.contains(needle))
}

/// `failedOutput(input, kind, message)`: `rc` false, the event's value (or
/// the target field's) kept.
fn failed_output(
    input: &ScriptInput,
    kind: ScriptExecutionErrorKind,
    message: String,
) -> ScriptOutput {
    let event = &input.event;
    let target_value = event.target.as_ref().and_then(|target| {
        input.fields.iter().find(|field| same_field(&field.field_ref, target))?.value.clone()
    });
    let present = |v: &Option<Value>| v.clone().filter(|v| !v.is_null());
    ScriptOutput {
        event: ScriptEventOutput {
            rc: false,
            value: present(&event.value).or_else(|| present(&target_value)).unwrap_or(Value::Null),
            change: event.change.clone().unwrap_or_default(),
            sel_start: event.sel_start.unwrap_or(0.0),
            sel_end: event.sel_end.unwrap_or(0.0),
        },
        form_effects: Vec::new(),
        annot_effects: Vec::new(),
        ui_effects: Vec::new(),
        diagnostics: Vec::new(),
        error: Some(ScriptExecutionError { kind, message, stack: None }),
    }
}

/// `QuickJsSandbox.ts` `failedOutput`'s target match: object numbers by
/// `objectNumber` (`===`, absent being `undefined`), names by `name`.
fn same_field(a: &Value, b: &Value) -> bool {
    let kind = |v: &Value| v.get("kind").and_then(Value::as_str).map(str::to_string);
    let strict = |x: Option<&Value>, y: Option<&Value>| match (x, y) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (x, y) => x == y,
    };
    match (kind(a).as_deref(), kind(b).as_deref()) {
        (Some("objectNumber"), Some("objectNumber")) => {
            strict(a.get("objectNumber"), b.get("objectNumber"))
        }
        (Some("fqn"), Some("fqn")) => strict(a.get("name"), b.get("name")),
        _ => false,
    }
}

/// What `check_script_engine` saw.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptCheck {
    /// `app.alert` messages, in order.
    pub alerts: Vec<String>,
    pub error: Option<super::ScriptExecutionError>,
    /// Whether the run poisoned the realm.
    pub disposed: bool,
}

/// Run `source` once in a fresh realm from `factory`, outside any document,
/// with the default budget: a host's check that its engine works. Runs on
/// the calling thread; call it off the main thread.
#[uniffi::export]
pub fn check_script_engine(
    factory: Arc<dyn ScriptEngineFactory>,
    source: String,
) -> Result<ScriptCheck, ViewerError> {
    let sandbox = match sandbox_factory(factory)().peek() {
        Some(Ok(sandbox)) => sandbox,
        Some(Err(error)) => return Err(error.into()),
        None => unreachable!("sandbox_factory settles at once"),
    };
    let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
    event.event_type = Some("Doc".into());
    event.name = Some("Open".into());
    let input = ScriptInput {
        document: ScriptDocumentInput {
            id: "check".into(),
            file_name: "check.pdf".into(),
            page_count: 1.0,
            page_number: 0.0,
        },
        identity: ScriptIdentity::default(),
        environment: ScriptEnvironment { now_ms: 0.0, utc_offset_minutes: 0.0, random_seed: 1.0 },
        fields: Vec::new(),
        annots: None,
        annot_pages: None,
        annots_cover_document: None,
        event,
    };
    sandbox.boot(&[], &input, None)?;
    let output = sandbox.run(&source, &input, None)?;
    let alerts = output
        .ui_effects
        .iter()
        .filter_map(|effect| match effect {
            ScriptUiEffect::Alert { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect();
    let check = ScriptCheck {
        alerts,
        error: output.error.as_ref().map(Into::into),
        disposed: sandbox.disposed(),
    };
    sandbox.dispose();
    Ok(check)
}
