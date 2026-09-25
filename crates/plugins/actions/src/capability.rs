//! `capability.ts`: the dispatcher, the trigger machinery, the lifecycle
//! coordinator, document events, the submit chain, and the per-document
//! script host.
//!
//! Every TypeScript `async` function is an `async fn` here, and an `await` of
//! one is `rt::returned(...)`, which spends the turn a JavaScript caller
//! spends resuming. Two other awaits cost more than one turn and are spelled
//! out where they happen: an engine call returns an `AbortablePromise`, which
//! is not a native promise, so `await` and `Promise.resolve` adopt it as a
//! thenable ([`adopt_engine`]); and an `async` function that `return`s a
//! promise without awaiting it resolves through the same adoption.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use epdf_acrojs::js::number_to_string as num;
use epdf_acrojs::{
    ScriptAnnotInput, ScriptColorArray, ScriptDiagnostic, ScriptDiagnosticCode,
    ScriptDocumentInput, ScriptEnvironment, ScriptEventInput, ScriptEventKind,
    ScriptExecutionError, ScriptHost, ScriptHostOptions, ScriptIdentity, ScriptOutput,
    ScriptTransaction, ScriptUiEffect, ScriptWorldInput, java_script_program_from_action_tree,
    resolve_script_identity, script_fields_from_snapshot, seed_from,
};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Ctx, DOCUMENTS, Error, Unsubscribe};
use serde_json::{Value, json};

use crate::types::*;

type Diagnose = Rc<dyn Fn(ActionDiagnostic)>;
type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;

const NEVER_TYPES: &[&str] =
    &["launch", "goto-remote", "goto-embedded", "sound", "movie", "import-data"];
const UNSUPPORTED_TYPES: &[&str] =
    &["rendition", "thread", "set-ocg-state", "transition", "goto-3d-view", "unknown"];
/// Document-lifetime node types: engine mutations, committed in walk order.
const DOCUMENT_TYPES: &[&str] = &["reset-form", "javascript"];
const CASCADE_CAP: f64 = 8.0;

const ALLOW_ALL: ActionPolicyRow = ActionPolicyRow {
    user: ActionPolicyDecision::Allow,
    hover: ActionPolicyDecision::Allow,
    lifecycle: ActionPolicyDecision::Allow,
};
const ADAPTER_ON_USER_BLOCK_ELSE: ActionPolicyRow = ActionPolicyRow {
    user: ActionPolicyDecision::Adapter,
    hover: ActionPolicyDecision::Block,
    lifecycle: ActionPolicyDecision::Block,
};

/// `DEFAULT_POLICY`.
pub fn default_policy() -> ActionPolicy {
    ActionPolicy {
        goto: ALLOW_ALL,
        named: ALLOW_ALL,
        hide: ALLOW_ALL,
        reset_form: ALLOW_ALL,
        javascript: ALLOW_ALL,
        // No auto-opened tabs: only a real user activation reaches the adapter.
        uri: ActionPolicyRow {
            user: ActionPolicyDecision::Adapter,
            hover: ActionPolicyDecision::Report,
            lifecycle: ActionPolicyDecision::Report,
        },
        print: ADAPTER_ON_USER_BLOCK_ELSE,
        submit_form: ADAPTER_ON_USER_BLOCK_ELSE,
    }
}

/// `{...DEFAULT_POLICY, ...config.policy}`.
fn merged_policy(overrides: Option<&PartialActionPolicy>) -> ActionPolicy {
    let mut policy = default_policy();
    if let Some(o) = overrides {
        let pick = |row: &mut ActionPolicyRow, over: &Option<ActionPolicyRow>| {
            if let Some(over) = over {
                *row = *over;
            }
        };
        pick(&mut policy.goto, &o.goto);
        pick(&mut policy.named, &o.named);
        pick(&mut policy.hide, &o.hide);
        pick(&mut policy.reset_form, &o.reset_form);
        pick(&mut policy.javascript, &o.javascript);
        pick(&mut policy.uri, &o.uri);
        pick(&mut policy.print, &o.print);
        pick(&mut policy.submit_form, &o.submit_form);
    }
    policy
}

// ── JSON reads, as JavaScript reads them ──────────────────────────────────────

fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

fn truthy(value: Option<&Value>) -> bool {
    epdf_acrojs::actions::truthy(value)
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    get(value, key).and_then(Value::as_str).unwrap_or_default()
}

fn node_type(node: &Value) -> String {
    str_of(node, "type").to_string()
}

fn is_print_verb(node: &Value) -> bool {
    str_of(node, "type") == "named" && str_of(node, "name") == "Print"
}

/// `a === b` for two JSON numbers (absent is `undefined`).
fn same_number(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        _ => false,
    }
}

/// `sameRef`: an annotation ref names the same annotation.
fn same_ref(left: &Value, right: &Value) -> bool {
    let kind = |v: &Value| str_of(v, "kind").to_string();
    match (kind(left).as_str(), kind(right).as_str()) {
        ("objectNumber", "objectNumber") => {
            same_number(get(left, "annotObjectNumber"), get(right, "annotObjectNumber"))
        }
        ("nm", "nm") => {
            same_number(get(left, "pageObjectNumber"), get(right, "pageObjectNumber"))
                && get(left, "nm") == get(right, "nm")
        }
        ("index", "index") => {
            same_number(get(left, "pageObjectNumber"), get(right, "pageObjectNumber"))
                && same_number(get(left, "index"), get(right, "index"))
        }
        _ => false,
    }
}

fn error_message(error: &Error) -> String {
    error.message.clone()
}

/// `Promise.resolve(abortable)`: a promise adopting the engine's
/// `AbortablePromise` as a thenable. It settles two turns after an
/// already-settled call (the resolve-thenable job, then the reaction), one
/// turn after a later one; awaiting it adds the usual turn.
fn adopt_engine(call: Promise<Value>) -> Promise<Value> {
    Promise::spawn(async move {
        rt::tick().await;
        call.await
    })
}

/// `await doc.x(...)`.
async fn await_engine(call: Promise<Value>) -> Result<Value, Error> {
    adopt_engine(call).await
}

/// An `async` function's `return promise`: resolved through the same
/// adoption, two turns after an already-settled promise.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.await
}

// ── the capability ────────────────────────────────────────────────────────────

enum ExecutorKind {
    Registered(ActionExecutor),
    /// The built-in `javascript` executor over the script host.
    Script,
}

struct Registered {
    id: u64,
    kind: Rc<ExecutorKind>,
}

#[derive(Clone)]
struct LifecycleAnnot {
    annotation_ref: Value,
    actions: Value,
}

struct LastEmitted {
    current_pon: Option<f64>,
    visible: Vec<f64>,
}

pub(crate) struct Inner {
    ctx: Ctx<ActionsState, ActionsAction>,
    config: ActionsPluginConfig,
    policy: ActionPolicy,
    queue: SerialQueue,
    executors: RefCell<HashMap<String, Registered>>,
    next_id: Cell<u64>,
    annot_commit_sink: RefCell<Option<AnnotCommitSink>>,
    form_commit_sink: RefCell<Option<FormCommitSink>>,
    ui_adapter: RefCell<Option<Rc<ActionUiAdapter>>>,
    submit_handler: RefCell<Option<ActionSubmitHandler>>,
    submit_resolver: RefCell<Option<SubmitResolver>>,
    /// The document-print latch: while a print event wrapper is active, a
    /// nested print request is suppressed.
    doc_print_event_active: Cell<bool>,
    /// JS nodes run in the current dispatch.
    js_nodes_this_dispatch: Cell<f64>,
    action_hook: EventHookSource<ActionDispatchEvent>,
    diagnostic_hook: EventHookSource<ActionDiagnostic>,
    script_diagnostic_hook: EventHookSource<ScriptDiagnostic>,
    script_error_hook: EventHookSource<ScriptExecutionError>,
    /// The one memoized catalog-actions read; a rejected read is evicted.
    actions_snapshot: RefCell<Option<Promise<Option<Value>>>>,
    script_host: Option<ScriptHost>,
    lifecycle_cache: RefCell<Vec<(f64, Vec<LifecycleAnnot>)>>,
    barrier_open: Cell<bool>,
    buffered_report: RefCell<Option<PageStateReport>>,
    last_emitted: RefCell<LastEmitted>,
    cascade_rounds: Cell<f64>,
    open_fired: Cell<bool>,
    saw_user_activity: Cell<bool>,
}

/// `ActionsCapability` and `ActionsHostCapability`: one runtime object. The
/// host members (`register_*`, `script_transaction`, `surface_script_result`,
/// `report_page_state`) are for sibling plugins, not applications.
#[derive(Clone)]
pub struct ActionsCapability(Rc<Inner>);

fn observer_hook<T: 'static>(
    config: &ActionsPluginConfig,
    message: &'static str,
) -> EventHookSource<T> {
    let sink = config.observer_error.clone();
    EventHookSource::new(Some(Rc::new(move |error: &Error| {
        if let Some(sink) = &sink {
            sink(message, error);
        }
    })))
}

fn script_host_for(
    ctx: &Ctx<ActionsState, ActionsAction>,
    config: &ActionsPluginConfig,
    weak: Weak<Inner>,
) -> Option<ScriptHost> {
    let js = config.javascript.as_ref()?;
    if !js.enabled {
        return None;
    }
    // The TypeScript's default is a lazily imported QuickJS sandbox. Rust has
    // none: a host that supplies no sandbox runs with scripting disabled.
    let sandbox_factory = js.sandbox_factory.clone()?;
    ctx.doc()?;
    let document = {
        let ctx = ctx.clone();
        let file_name = js.file_name.clone();
        Rc::new(move || {
            let meta = ctx.document();
            ScriptDocumentInput {
                id: ctx.document_id().unwrap_or("document").to_string(),
                file_name: match &file_name {
                    Some(f) => f(),
                    None => meta
                        .as_ref()
                        .and_then(|m| m.name.clone())
                        .unwrap_or_else(|| "document.pdf".into()),
                },
                page_count: meta.map_or(0.0, |m| f64::from(m.page_count)),
                page_number: 0.0,
            }
        })
    };
    let identity = {
        let ctx = ctx.clone();
        let overrides = js.identity.clone();
        Rc::new(move || match ctx.doc() {
            Some(doc) => resolve_script_identity(doc.as_ref(), overrides.as_ref()),
            None => ScriptIdentity::default(),
        })
    };
    let environment = {
        let ctx = ctx.clone();
        let (now, offset, seed) =
            (js.now.clone(), js.utc_offset_minutes.clone(), js.random_seed.clone());
        let host_offset = config.host_utc_offset_minutes.clone();
        Rc::new(move |sequence: f64| {
            let now_ms = now.as_ref().map_or_else(rt::now, |f| f());
            ScriptEnvironment {
                now_ms,
                utc_offset_minutes: match &offset {
                    Some(f) => f(),
                    None => host_offset.as_ref().map_or(0.0, |f| f(now_ms)),
                },
                random_seed: match &seed {
                    Some(f) => f(),
                    None => seed_from(ctx.document_id().unwrap_or("document"), sequence),
                },
            }
        })
    };
    let boot_sources = Rc::new(move || {
        let Some(inner) = weak.upgrade() else { return Ok(Promise::resolved(Vec::new())) };
        let this = ActionsCapability(inner);
        Ok(Promise::spawn(async move {
            let snapshot = this.read_document_actions().await?;
            let scripts = snapshot
                .as_ref()
                .and_then(|s| get(s, "nameTreeScripts"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            scripts
                .iter()
                .map(|entry| {
                    java_script_program_from_action_tree(
                        get(entry, "action").unwrap_or(&Value::Null),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        }))
    });
    Some(ScriptHost::new(ScriptHostOptions {
        sandbox_factory,
        document,
        identity,
        environment,
        boot_sources,
        budget: js.budget.clone(),
    }))
}

/// `createActionsCapability(ctx, config)`.
pub fn create_actions_capability(
    ctx: Ctx<ActionsState, ActionsAction>,
    config: ActionsPluginConfig,
) -> ActionsCapability {
    let policy = merged_policy(config.policy.as_ref());
    let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
        let action_hook = observer_hook(&config, "[actions] onAction observer failed:");
        let diagnostic_hook = observer_hook(&config, "[actions] onDiagnostic observer failed:");
        let script_diagnostic_hook =
            observer_hook(&config, "[actions] onScriptDiagnostic observer failed:");
        let script_error_hook = observer_hook(&config, "[actions] onScriptError observer failed:");
        {
            let hooks = (
                action_hook.clone(),
                diagnostic_hook.clone(),
                script_diagnostic_hook.clone(),
                script_error_hook.clone(),
            );
            ctx.cleanup_fn(move || {
                hooks.0.dispose();
                hooks.1.dispose();
                hooks.2.dispose();
                hooks.3.dispose();
            });
        }
        let script_host = script_host_for(&ctx, &config, weak.clone());
        if let Some(host) = &script_host {
            let host = host.clone();
            ctx.cleanup_fn(move || host.dispose());
        }
        let mut executors = HashMap::new();
        if script_host.is_some() {
            executors.insert(
                "javascript".to_string(),
                Registered { id: 0, kind: Rc::new(ExecutorKind::Script) },
            );
        }
        Inner {
            ctx: ctx.clone(),
            config: config.clone(),
            policy,
            queue: SerialQueue::new(),
            executors: RefCell::new(executors),
            next_id: Cell::new(1),
            annot_commit_sink: RefCell::new(None),
            form_commit_sink: RefCell::new(None),
            ui_adapter: RefCell::new(None),
            submit_handler: RefCell::new(None),
            submit_resolver: RefCell::new(None),
            doc_print_event_active: Cell::new(false),
            js_nodes_this_dispatch: Cell::new(0.0),
            action_hook,
            diagnostic_hook,
            script_diagnostic_hook,
            script_error_hook,
            actions_snapshot: RefCell::new(None),
            script_host,
            lifecycle_cache: RefCell::new(Vec::new()),
            barrier_open: Cell::new(false),
            buffered_report: RefCell::new(None),
            last_emitted: RefCell::new(LastEmitted { current_pon: None, visible: Vec::new() }),
            cascade_rounds: Cell::new(0.0),
            open_fired: Cell::new(false),
            saw_user_activity: Cell::new(false),
        }
    });
    let this = ActionsCapability(inner);
    // The lifecycle cache: cleared on any annotation.* event and on
    // stream.desynced (gap events never arrive).
    if let Some(doc) = this.0.ctx.doc() {
        let weak = Rc::downgrade(&this.0);
        let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
            let kind = str_of(event, "type");
            if (kind.starts_with("annotation.") || kind == "stream.desynced")
                && let Some(inner) = weak.upgrade()
            {
                inner.lifecycle_cache.borrow_mut().clear();
            }
        }));
        this.0.ctx.cleanup_fn(move || unsubscribe.call());
    }
    // Arm the open-sequence latch: fires now for headless, releases the
    // barrier for off, waits for an adapter or user activity for auto.
    this.maybe_fire_open_sequence();
    this
}

impl ActionsCapability {
    fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.0.ctx.doc()
    }

    fn js(&self) -> Option<&JavaScriptConfig> {
        self.0.config.javascript.as_ref()
    }

    fn open_sequence(&self) -> OpenSequence {
        self.0.config.open_sequence.unwrap_or(OpenSequence::Auto)
    }

    fn emit_diagnostic(&self, diagnostic: ActionDiagnostic) {
        self.0.diagnostic_hook.emit(&diagnostic);
    }

    /// A `diagnose` that only emits.
    fn emitter(&self) -> Diagnose {
        let this = self.clone();
        Rc::new(move |d| this.emit_diagnostic(d))
    }

    /// A `diagnose` that collects into `into` and emits.
    fn collector(&self, into: &Rc<RefCell<Vec<ActionDiagnostic>>>) -> Diagnose {
        let (this, into) = (self.clone(), into.clone());
        Rc::new(move |d: ActionDiagnostic| {
            into.borrow_mut().push(d.clone());
            this.emit_diagnostic(d);
        })
    }

    /// Policy lookup: `never` and `unsupported` besides the four decisions.
    fn decision_for(&self, node: &Value, origin: ActionOrigin) -> Decision {
        let kind = node_type(node);
        if NEVER_TYPES.contains(&kind.as_str()) {
            return Decision::Never;
        }
        if UNSUPPORTED_TYPES.contains(&kind.as_str()) {
            return Decision::Unsupported;
        }
        if is_print_verb(node) {
            return Decision::Policy(self.0.policy.print.get(origin));
        }
        match self.0.policy.row(&kind) {
            Some(row) => Decision::Policy(row.get(origin)),
            None => Decision::Unsupported,
        }
    }

    fn allows_print(&self) -> bool {
        match self.0.ctx.try_get(&DOCUMENTS) {
            Ok(Some(documents)) => documents.allows("doc.print", self.0.ctx.document_id()),
            _ => true,
        }
    }

    // ── the one catalog-actions read ─────────────────────────────────────────

    fn read_document_actions(&self) -> Promise<Option<Value>> {
        if let Some(memo) = self.0.actions_snapshot.borrow().clone() {
            return memo;
        }
        let read: Promise<Option<Value>> = match self.doc() {
            Some(doc) if doc.has("actions") => {
                let call = adopt_engine(doc.call("actions.read", None, Vec::new()));
                Promise::spawn(async move { call.await.map(Some) })
            }
            _ => Promise::resolved(None),
        };
        // `read.catch(...)`: one turn after the read settles.
        let (memo, resolver) = Promise::pending();
        let (weak, own) = (Rc::downgrade(&self.0), memo.clone());
        rt::spawn(async move {
            let result = read.await;
            if result.is_err()
                && let Some(inner) = weak.upgrade()
            {
                let current =
                    inner.actions_snapshot.borrow().as_ref().is_some_and(|m| m.same(&own));
                if current {
                    *inner.actions_snapshot.borrow_mut() = None;
                }
            }
            resolver.settle(result);
        });
        *self.0.actions_snapshot.borrow_mut() = Some(memo.clone());
        memo
    }

    // ── the submit pipeline ──────────────────────────────────────────────────

    fn now_ms(&self) -> f64 {
        match self.js().and_then(|js| js.now.clone()) {
            Some(now) => now(),
            None => rt::now(),
        }
    }

    fn intent_of_payload(payload: &Value) -> SubmitIntent {
        let flags = get(payload, "flags").cloned().unwrap_or(Value::Null);
        SubmitIntent {
            url: get(payload, "url").and_then(Value::as_str).map(str::to_string),
            fields: get(payload, "fields").and_then(Value::as_array).cloned(),
            exclude: get(&flags, "exclude").and_then(Value::as_bool).unwrap_or(false),
            include_no_value_fields: get(&flags, "includeNoValueFields")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            format: str_of(&flags, "format").to_string(),
            method: str_of(&flags, "method").to_string(),
            flags_raw: get(&flags, "raw").and_then(Value::as_f64).unwrap_or(0.0),
            char_set: get(payload, "charSet").and_then(Value::as_str).map(str::to_string),
        }
    }

    fn to_form_submission_request(&self, request: &ActionSubmitRequest) -> Value {
        let mut intent = json!({
            "url": request.url,
            "format": request.format,
            "method": request.method,
            "flagsRaw": request.flags_raw,
        });
        if let Some(char_set) = &request.char_set {
            intent["charSet"] = json!(char_set);
        }
        json!({
            "entries": request.entries,
            "intent": intent,
            "origin": request.origin,
            "clientTimeMs": self.now_ms(),
        })
    }

    /// The one submit door: embedder handler, then the document's home,
    /// then blocked. Never a network call from this plugin.
    async fn perform_submit(
        self,
        intent: SubmitIntent,
        action_ctx: ActionContext,
        diagnose: Diagnose,
    ) -> (ActionNodeStatus, Option<String>) {
        let decision = self.0.policy.submit_form.get(action_ctx.origin);
        if !matches!(decision, ActionPolicyDecision::Adapter | ActionPolicyDecision::Allow) {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::Blocked,
                format!(
                    "submit-form: policy '{}' for origin '{}'",
                    decision.as_str(),
                    action_ctx.origin.as_str()
                ),
            ));
            return (ActionNodeStatus::Blocked, None);
        }
        let resolver = self.0.submit_resolver.borrow().clone();
        let Some(resolver) = resolver else {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::NoSubmitResolver,
                "submit-form: no dataset resolver registered (form plugin missing?)",
            ));
            return (ActionNodeStatus::Blocked, Some("no dataset resolver".into()));
        };
        let resolved = match resolver(&intent, &action_ctx, diagnose.clone()) {
            Ok(promise) => promise.await,
            Err(error) => Err(error),
        };
        let request = match resolved {
            Ok(request) => request,
            Err(error) => {
                let detail = error_message(&error);
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("submit-form: dataset resolution: {detail}"),
                ));
                return (ActionNodeStatus::Failed, Some(detail));
            }
        };
        let home: Option<SubmitToDocumentHome> = match self.doc() {
            Some(doc) if doc.has("forms.submit") => {
                let (this, request) = (self.clone(), request.clone());
                Some(Rc::new(move || {
                    let body = this.to_form_submission_request(&request);
                    Ok(adopt_engine(doc.call("forms.submit", None, vec![body])))
                }))
            }
            _ => None,
        };
        let handler = self.0.submit_handler.borrow().clone();
        if let Some(handler) = handler {
            return match handler(&request, SubmitHandlerContext { submit_to_document_home: home }) {
                Ok(outcome) => {
                    if let Some(outcome) = outcome {
                        // Detached by contract: `executed` means handed to the
                        // embedder; a later rejection is observability only.
                        let this = self.clone();
                        rt::spawn(async move {
                            if let Err(error) = outcome.await {
                                this.emit_diagnostic(ActionDiagnostic::new(
                                    ActionDiagnosticCode::ExecutorFailed,
                                    format!(
                                        "submit handler rejected (detached): {}",
                                        error.message
                                    ),
                                ));
                            }
                        });
                    }
                    (ActionNodeStatus::Executed, None)
                }
                Err(error) => {
                    let detail = error_message(&error);
                    diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::ExecutorFailed,
                        format!("submit handler threw: {detail}"),
                    ));
                    (ActionNodeStatus::Failed, Some(detail))
                }
            };
        }
        if let Some(home) = home {
            let submitted = match home() {
                Ok(promise) => promise.await,
                Err(error) => Err(error),
            };
            return match submitted {
                Ok(_) => (ActionNodeStatus::Executed, None),
                Err(error) => {
                    // The engine's refusal, surfaced on the node.
                    let detail = error_message(&error);
                    diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::ExecutorFailed,
                        format!("document home refused submit: {detail}"),
                    ));
                    (ActionNodeStatus::Failed, Some(detail))
                }
            };
        }
        diagnose(ActionDiagnostic::new(
            ActionDiagnosticCode::NoSubmitSink,
            "submit-form: no handler installed and the document has no submit-capable home",
        ));
        (ActionNodeStatus::Blocked, Some("no submit sink".into()))
    }

    /// Script `doc.submitForm(...)` as the one normalized intent. Script
    /// field names are include-mode.
    fn intent_of_submit_effect(effect: &ScriptUiEffect) -> Option<SubmitIntent> {
        let ScriptUiEffect::SubmitForm { url, field_names, include_empty, format, method } = effect
        else {
            return None;
        };
        Some(SubmitIntent {
            url: url.clone(),
            fields: field_names.as_ref().map(|names| {
                names.iter().map(|name| json!({ "kind": "name", "name": name })).collect()
            }),
            exclude: false,
            include_no_value_fields: *include_empty,
            format: format.clone().unwrap_or_else(|| "fdf".into()),
            method: method.clone().unwrap_or_else(|| "post".into()),
            flags_raw: 0.0,
            char_set: None,
        })
    }

    // ── the surface door ─────────────────────────────────────────────────────

    /// Surface a script transaction's UI effects, diagnostics and error
    /// through the one port (host lens). `Err` is an adapter that threw.
    pub fn surface_script_result(&self, result: &ScriptSurfaceResult) -> Result<(), Error> {
        let ui_context = ActionUiContext { origin: result.origin, phase: result.phase };
        for effect in &result.ui_effects {
            if let Some(intent) = Self::intent_of_submit_effect(effect) {
                // The form pipeline's scripted submit: the same door, detached.
                let ctx = ActionContext {
                    origin: result.origin,
                    source: ActionSource::Api,
                    event: ActionTriggerEvent::Activate,
                };
                let this = self.clone();
                let diagnose = self.emitter();
                rt::spawn(async move {
                    this.perform_submit(intent, ctx, diagnose).await;
                });
                continue;
            }
            let is_print = matches!(effect, ScriptUiEffect::Print);
            // Permission, not preference.
            if is_print && !self.allows_print() {
                self.0.script_diagnostic_hook.emit(&ScriptDiagnostic {
                    code: ScriptDiagnosticCode::UiEffectSuppressed,
                    message: "script print request withheld: doc.print is not allowed".into(),
                });
                continue;
            }
            if is_print && self.0.doc_print_event_active.get() {
                self.0.script_diagnostic_hook.emit(&ScriptDiagnostic {
                    code: ScriptDiagnosticCode::UiEffectSuppressed,
                    message: "script print request during a document print event — suppressed (reentrant)".into(),
                });
                continue;
            }
            let adapter = self.0.ui_adapter.borrow().clone();
            let Some(adapter) = adapter else {
                self.emit_diagnostic(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoAdapter,
                    format!("script {}: no UI adapter installed", effect.kind()),
                ));
                continue;
            };
            match effect {
                ScriptUiEffect::Alert { message, icon, title } => {
                    if let Some(alert) = &adapter.alert {
                        alert(
                            message,
                            &AlertOptions {
                                origin: ui_context.origin,
                                phase: ui_context.phase,
                                icon: *icon,
                                title: title.clone(),
                            },
                        )?;
                    }
                }
                ScriptUiEffect::GotoPage { page } => {
                    if let Some(goto_page) = &adapter.goto_page {
                        goto_page(*page, &ui_context)?;
                    }
                }
                _ => (adapter.print)(Some(&ui_context))?,
            }
        }
        for diagnostic in &result.diagnostics {
            self.0.script_diagnostic_hook.emit(diagnostic);
        }
        if let Some(error) = &result.error {
            self.0.script_error_hook.emit(error);
        }
        Ok(())
    }

    // ── script world building ────────────────────────────────────────────────

    fn engine_color_to_array(color: Option<&Value>) -> ScriptColorArray {
        match color.filter(|c| truthy(Some(c))) {
            Some(c) => {
                let channel =
                    |k: &str| get(c, k).and_then(Value::as_f64).unwrap_or(f64::NAN) / 255.0;
                ScriptColorArray::Rgb(channel("r"), channel("g"), channel("b"))
            }
            None => ScriptColorArray::T,
        }
    }

    fn page_index_of(&self, pon: f64) -> f64 {
        self.0.ctx.document().map_or(-1.0, |meta| {
            meta.pages
                .as_array()
                .and_then(|pages| {
                    pages.iter().position(|p| {
                        get(p, "pageObjectNumber").and_then(Value::as_f64) == Some(pon)
                    })
                })
                .map_or(-1.0, |i| i as f64)
        })
    }

    fn page_count(&self) -> f64 {
        self.0.ctx.document().map_or(0.0, |meta| f64::from(meta.page_count))
    }

    fn first_pon(&self) -> Option<f64> {
        let meta = self.0.ctx.document()?;
        meta.pages
            .as_array()?
            .first()
            .and_then(|p| get(p, "pageObjectNumber"))
            .and_then(Value::as_f64)
    }

    fn script_annot(annotation: &Value, page_index: f64) -> Result<ScriptAnnotInput, Error> {
        let text = |key: &str| {
            get(annotation, key).and_then(Value::as_str).unwrap_or_default().to_string()
        };
        let rect = get(annotation, "rect").cloned().unwrap_or(Value::Null);
        let side = |k: &str| get(&rect, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let flags = get(annotation, "flags").cloned().unwrap_or(Value::Null);
        let flag = |k: &str| get(&flags, k).and_then(Value::as_bool).unwrap_or(false);
        let number_or =
            |k: &str, fallback: f64| get(annotation, k).and_then(Value::as_f64).unwrap_or(fallback);
        let subtype = text("subtype");
        Ok(ScriptAnnotInput {
            annot_ref: get(annotation, "ref").cloned().unwrap_or(Value::Null),
            name: get(annotation, "nm").and_then(Value::as_str).unwrap_or_default().to_string(),
            page: page_index,
            rect: [side("left"), side("bottom"), side("right"), side("top")],
            contents: text("contents"),
            author: text("author"),
            subject: text("subject"),
            stroke_color: Self::engine_color_to_array(get(annotation, "color")),
            fill_color: Self::engine_color_to_array(get(annotation, "interiorColor")),
            opacity: number_or("opacity", 1.0),
            width: number_or("strokeWidth", 1.0),
            border_style: if str_of(annotation, "borderStyle") == "dashed" { "D" } else { "S" }
                .into(),
            dash: get(annotation, "dashArray")
                .and_then(Value::as_array)
                .map(|d| d.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default(),
            hidden: flag("hidden"),
            print: flag("print"),
            read_only: flag("readOnly"),
            locked: flag("locked"),
            no_view: flag("noView"),
            toggle_no_view: flag("toggleNoView"),
            opaque_body: subtype == "stamp",
            subtype,
        })
    }

    /// The page-scoped world: the form snapshot and the script-addressable
    /// annotations of one page.
    async fn script_world_for(self, pon: f64) -> Result<(Value, ScriptWorldInput), Error> {
        let doc = self.doc().ok_or_else(|| Error::new("[actions] no document bound"))?;
        let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
        let page_index = epdf_acrojs::js::max(0.0, self.page_index_of(pon));
        doc.page_index(pon as u32)?;
        let listed =
            await_engine(doc.call("page.annotations.list", Some(pon as u32), Vec::new())).await?;
        let annots = get(&listed, "annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            // Script-addressable: everything except link and widget.
            .filter(|a| {
                let subtype = str_of(a, "subtype");
                subtype != "link" && !subtype.starts_with("widget")
            })
            .map(|a| Self::script_annot(a, page_index))
            .collect::<Result<Vec<_>, _>>()?;
        let fields = script_fields_from_snapshot(&snapshot)?;
        let world = ScriptWorldInput {
            fields,
            annots: Some(annots),
            annot_pages: Some(vec![page_index]),
            annots_cover_document: Some(self.page_count() <= 1.0),
            page_number: None,
            event: ScriptEventInput::of(ScriptEventKind::WidgetActivate),
        };
        Ok((snapshot, world))
    }

    fn script_event_for(ctx: &ActionContext, snapshot: &Value) -> ScriptEventInput {
        let event_type = match (&ctx.event, &ctx.source) {
            (ActionTriggerEvent::Page { .. }, _) => "Page",
            (ActionTriggerEvent::Document { .. }, _) => "Doc",
            (_, ActionSource::Widget { .. }) => "Field",
            (_, ActionSource::Link { .. }) => "Link",
            // An EmbedPDF extension: Acrobat has no plain-annotation type.
            _ => "Annot",
        };
        let name = match &ctx.event {
            ActionTriggerEvent::Activate => "Mouse Up",
            ActionTriggerEvent::Annotation { name } => match name {
                PdfAnnotationEventKind::CursorEnter => "Mouse Enter",
                PdfAnnotationEventKind::CursorExit => "Mouse Exit",
                PdfAnnotationEventKind::MouseDown => "Mouse Down",
                PdfAnnotationEventKind::MouseUp => "Mouse Up",
                PdfAnnotationEventKind::Focus => "Focus",
                PdfAnnotationEventKind::Blur => "Blur",
            },
            ActionTriggerEvent::Page { name } => match name {
                PageEventKind::Open => "Open",
                PageEventKind::Close => "Close",
                // No Acrobat equivalent for these two: an EmbedPDF extension.
                PageEventKind::Visible => "Visible",
                PageEventKind::Invisible => "Invisible",
            },
            ActionTriggerEvent::Document { name } => match name {
                DocumentTriggerEvent::Open => "Open",
                DocumentTriggerEvent::WillSave => "WillSave",
                DocumentTriggerEvent::DidSave => "DidSave",
                DocumentTriggerEvent::WillPrint => "WillPrint",
                DocumentTriggerEvent::DidPrint => "DidPrint",
                DocumentTriggerEvent::WillClose => "WillClose",
            },
        };
        let target = match &ctx.source {
            ActionSource::Widget { field, .. } => Some(field.clone()),
            _ => None,
        };
        let target_field = target.as_ref().and_then(|target| {
            get(snapshot, "fields").and_then(Value::as_array)?.iter().find(|field| {
                let field_ref = get(field, "ref").cloned().unwrap_or(Value::Null);
                if str_of(target, "kind") == "objectNumber"
                    && str_of(&field_ref, "kind") == "objectNumber"
                {
                    same_number(
                        get(&field_ref, "fieldObjectNumber"),
                        get(target, "fieldObjectNumber"),
                    )
                } else {
                    str_of(target, "kind") == "fqn" && get(field, "name") == get(target, "name")
                }
            })
        });
        let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
        event.event_type = Some(event_type.into());
        event.name = Some(name.into());
        event.target = target.clone();
        event.source = target;
        if let Some(entry) = target_field.and_then(|f| get(f, "valueEntry"))
            && str_of(entry, "kind") == "scalar"
        {
            event.value = Some(get(entry, "value").cloned().unwrap_or(Value::Null));
        }
        event
    }

    /// Commit one run's document effects through the owner sinks, form
    /// first; the first failure skips the rest. The failure summary, if any.
    async fn commit_script_output(
        self,
        output: ScriptOutput,
        diagnose: Diagnose,
    ) -> Result<Option<String>, Error> {
        let mut failure: Option<String> = None;
        if !output.form_effects.is_empty() {
            let sink = self.0.form_commit_sink.borrow().clone();
            match sink {
                None => diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    format!(
                        "script form effects dropped: no form commit sink registered ({})",
                        output.form_effects.len()
                    ),
                )),
                Some(sink) => {
                    let result = sink(output.form_effects.clone())?.await?;
                    if let Some(bad) = result
                        .results
                        .iter()
                        .find(|e| e.status == "failed" || e.status == "rejected")
                    {
                        let why = bad
                            .error
                            .as_ref()
                            .and_then(|e| e.message.clone())
                            .unwrap_or_else(|| bad.status.clone());
                        failure = Some(format!("form effect {}: {why}", num(bad.index)));
                    }
                }
            }
        }
        if !output.annot_effects.is_empty() {
            let sink = self.0.annot_commit_sink.borrow().clone();
            if failure.is_some() {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!(
                        "script annot effects skipped after form failure ({})",
                        output.annot_effects.len()
                    ),
                ));
            } else if let Some(sink) = sink {
                let entries = output
                    .annot_effects
                    .iter()
                    .map(|effect| {
                        let by_number = str_of(&effect.annot_ref, "kind") == "objectNumber";
                        AnnotCommitEntry {
                            annot_object_number: if by_number {
                                get(&effect.annot_ref, "annotObjectNumber")
                                    .and_then(Value::as_f64)
                                    .unwrap_or(f64::NAN)
                            } else {
                                -1.0
                            },
                            page_object_number: if by_number {
                                get(&effect.annot_ref, "pageObjectNumber").and_then(Value::as_f64)
                            } else {
                                None
                            },
                            patch: effect.patch.clone(),
                        }
                    })
                    .collect();
                let result = sink(entries)?.await?;
                if let Some(bad) =
                    result.results.iter().find(|e| e.status == AnnotCommitStatus::Failed)
                {
                    failure = Some(format!(
                        "annotation {}: {}",
                        num(bad.annot_object_number),
                        bad.error.clone().unwrap_or_else(|| "failed".into())
                    ));
                }
            } else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    format!(
                        "script annot effects dropped: no annotation commit sink registered ({})",
                        output.annot_effects.len()
                    ),
                ));
            }
        }
        if let Some(failure) = &failure {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::ExecutorFailed,
                format!("script commit: {failure}"),
            ));
        }
        Ok(failure)
    }

    /// The real `javascript` executor: one node is one host transaction, with
    /// prefetch, boot, run, commit and surface inside the boundary.
    async fn script_executor(
        self,
        node: Value,
        action_ctx: ActionContext,
    ) -> Result<ActionExecutorResult, Error> {
        if str_of(&node, "type") != "javascript" {
            return Ok(ActionExecutorResult::Inert { reason: "not a JS node".into() });
        }
        if self.doc().is_none() {
            return Ok(ActionExecutorResult::Inert { reason: "no document".into() });
        }
        let Some(host) = self.0.script_host.clone() else {
            return Ok(ActionExecutorResult::Inert { reason: "no document".into() });
        };
        let node_cap = self.js().and_then(|js| js.max_script_nodes_per_dispatch).unwrap_or(16.0);
        if self.0.js_nodes_this_dispatch.get() >= node_cap {
            return Ok(ActionExecutorResult::Inert {
                reason: format!("dispatch script budget exhausted ({} JS nodes)", num(node_cap)),
            });
        }
        self.0.js_nodes_this_dispatch.set(self.0.js_nodes_this_dispatch.get() + 1.0);
        let pon = match &action_ctx.source {
            ActionSource::Widget { pon, .. } | ActionSource::Page { pon } => Some(*pon),
            ActionSource::Link { pon, .. } => pon.or_else(|| self.first_pon()),
            ActionSource::Annotation { pon, .. } => Some(*pon),
            _ => self.first_pon(),
        };
        let Some(pon) = pon else {
            return Ok(ActionExecutorResult::Inert {
                reason: "no page to anchor the world on".into(),
            });
        };
        let diagnose = self.emitter();
        // Print and submit effects are external: they run after the
        // transaction releases, still inside this queued dispatch.
        let pending_external: Rc<RefCell<Vec<(ScriptUiEffect, ScriptPhase)>>> = Rc::default();
        let split_external = {
            let pending = pending_external.clone();
            move |output: &ScriptOutput, phase: ScriptPhase| -> Vec<ScriptUiEffect> {
                output
                    .ui_effects
                    .iter()
                    .filter(|effect| {
                        if matches!(
                            effect,
                            ScriptUiEffect::Print | ScriptUiEffect::SubmitForm { .. }
                        ) {
                            pending.borrow_mut().push(((*effect).clone(), phase));
                            false
                        } else {
                            true
                        }
                    })
                    .cloned()
                    .collect()
            }
        };
        let script = str_of(&node, "script").to_string();
        let (this, ctx2, diagnose2) = (self.clone(), action_ctx.clone(), diagnose.clone());
        let outcome = host
            .transaction(move |txn: ScriptTransaction| {
                Ok(Promise::spawn(async move {
                    let this = this;
                    let mut built = rt::returned(this.clone().script_world_for(pon)).await?;
                    let mut boot_event = ScriptEventInput::of(ScriptEventKind::NameTreeBoot);
                    boot_event.event_type = Some("Doc".into());
                    boot_event.name = Some("Open".into());
                    let boot = txn
                        .boot(ScriptWorldInput { event: boot_event, ..built.1.clone() }, None)
                        .await?;
                    if let Some(boot) = boot {
                        // Boot effects belong to this first transaction; a boot
                        // fault only degrades. Refetch so the run sees them.
                        rt::returned(
                            this.clone().commit_script_output(boot.clone(), diagnose2.clone()),
                        )
                        .await?;
                        this.surface_script_result(&ScriptSurfaceResult {
                            ui_effects: split_external(&boot, ScriptPhase::Boot),
                            diagnostics: boot.diagnostics.clone(),
                            error: boot.error.clone(),
                            origin: ctx2.origin,
                            phase: ScriptPhase::Boot,
                        })?;
                        if !boot.form_effects.is_empty() || !boot.annot_effects.is_empty() {
                            built = rt::returned(this.clone().script_world_for(pon)).await?;
                        }
                    }
                    let event = Self::script_event_for(&ctx2, &built.0);
                    let output = txn
                        .run(&script, ScriptWorldInput { event, ..built.1.clone() }, None)
                        .await?;
                    this.surface_script_result(&ScriptSurfaceResult {
                        ui_effects: split_external(&output, ScriptPhase::User),
                        diagnostics: output.diagnostics.clone(),
                        error: output.error.clone(),
                        origin: ctx2.origin,
                        phase: ScriptPhase::User,
                    })?;
                    if let Some(error) = &output.error {
                        return Ok(ActionExecutorResult::Failed { error: error.message.clone() });
                    }
                    let failure =
                        rt::returned(this.clone().commit_script_output(output, diagnose2)).await?;
                    Ok(match failure {
                        Some(error) => ActionExecutorResult::Failed { error },
                        None => ActionExecutorResult::Executed,
                    })
                }))
            })
            .await?;
        let external = std::mem::take(&mut *pending_external.borrow_mut());
        for (effect, phase) in external {
            match Self::intent_of_submit_effect(&effect) {
                None => {
                    let ui = ActionUiContext { origin: action_ctx.origin, phase };
                    rt::returned(
                        self.clone().fire_print_through_adapter(Some(ui), diagnose.clone()),
                    )
                    .await?;
                }
                Some(intent) => {
                    rt::returned(self.clone().perform_submit(
                        intent,
                        action_ctx.clone(),
                        diagnose.clone(),
                    ))
                    .await;
                }
            }
        }
        Ok(outcome)
    }

    // ── the tree walk ────────────────────────────────────────────────────────

    /// A Hide action sets or clears the document Hidden state through the
    /// owning plane's commit sink: widget targets by their field's display,
    /// plain annotations by a flag patch.
    async fn interpret_hide(
        self,
        node: Value,
        diagnose: Diagnose,
    ) -> Result<(ActionNodeStatus, Option<String>), Error> {
        let doc = self.doc();
        let hide = get(&node, "hide").and_then(Value::as_bool).unwrap_or(false);
        let display = if truthy(get(&node, "hide")) { "hidden" } else { "visible" };
        let mut field_refs: Vec<Value> = Vec::new();
        let mut annot_entries: Vec<AnnotCommitEntry> = Vec::new();
        let mut names: Vec<Value> = Vec::new();
        let mut bare_object_numbers: Vec<Value> = Vec::new();
        for target in get(&node, "targets").and_then(Value::as_array).cloned().unwrap_or_default() {
            if str_of(&target, "kind") == "objectNumber" {
                bare_object_numbers
                    .push(get(&target, "objectNumber").cloned().unwrap_or(Value::Null));
            } else {
                names.push(get(&target, "name").cloned().unwrap_or(Value::Null));
            }
        }
        if (!names.is_empty() || !bare_object_numbers.is_empty())
            && let Some(doc) = doc
        {
            let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
            let fields =
                get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let field_ref = |field: &Value| json!({ "kind": "objectNumber", "fieldObjectNumber": get(field, "fieldObjectNumber").cloned().unwrap_or(Value::Null) });
            for name in &names {
                match fields.iter().find(|candidate| get(candidate, "name") == Some(name)) {
                    None => diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::UnresolvedTarget,
                        format!("hide: no field named '{}'", epdf_acrojs::js::string_of(name)),
                    )),
                    Some(field) => field_refs.push(field_ref(field)),
                }
            }
            for object_number in &bare_object_numbers {
                // A bare object number may be a widget (its field's display)
                // or a plain annotation (its own flags).
                let owner = fields.iter().find(|field| {
                    get(field, "widgets").and_then(Value::as_array).is_some_and(|widgets| {
                        widgets
                            .iter()
                            .any(|w| same_number(get(w, "annotObjectNumber"), Some(object_number)))
                    })
                });
                match owner {
                    Some(owner) => field_refs.push(field_ref(owner)),
                    None => annot_entries.push(AnnotCommitEntry {
                        annot_object_number: object_number.as_f64().unwrap_or(f64::NAN),
                        page_object_number: None,
                        patch: json!({ "flags": { "hidden": hide } }),
                    }),
                }
            }
        }
        let mut failed_detail: Option<String> = None;
        if !field_refs.is_empty() {
            let sink = self.0.form_commit_sink.borrow().clone();
            let Some(sink) = sink else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    "hide: no form commit sink registered (form plugin absent)",
                ));
                return Ok((ActionNodeStatus::NoExecutor, None));
            };
            let effects = field_refs
                .into_iter()
                .map(|r| json!({ "kind": "setDisplay", "ref": r, "display": display }))
                .collect();
            let result = sink(effects)?.await?;
            if let Some(bad) =
                result.results.iter().find(|e| e.status == "failed" || e.status == "rejected")
            {
                let detail = bad
                    .error
                    .as_ref()
                    .and_then(|e| e.message.clone())
                    .unwrap_or_else(|| bad.status.clone());
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("hide: {detail}"),
                ));
                failed_detail = Some(detail);
            }
        }
        let failed_so_far = failed_detail.as_deref().is_some_and(|d| !d.is_empty());
        if !annot_entries.is_empty() && !failed_so_far {
            let sink = self.0.annot_commit_sink.borrow().clone();
            let Some(sink) = sink else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    "hide: no annotation commit sink registered (annotation plugin absent)",
                ));
                return Ok((ActionNodeStatus::NoExecutor, None));
            };
            let committed = sink(annot_entries)?.await?;
            let failed: Vec<&AnnotCommitResultEntry> = committed
                .results
                .iter()
                .filter(|e| e.status == AnnotCommitStatus::Failed)
                .collect();
            for failure in &failed {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!(
                        "hide: annotation {}: {}",
                        num(failure.annot_object_number),
                        failure.error.clone().unwrap_or_else(|| "failed".into())
                    ),
                ));
            }
            if failed.len() == committed.results.len() && !failed.is_empty() {
                failed_detail = failed[0].error.clone();
            }
        }
        match failed_detail.filter(|d| !d.is_empty()) {
            Some(detail) => Ok((ActionNodeStatus::Failed, Some(detail))),
            None => Ok((ActionNodeStatus::Executed, None)),
        }
    }

    fn executor(&self, kind: &str) -> Option<Rc<ExecutorKind>> {
        self.0.executors.borrow().get(kind).map(|r| r.kind.clone())
    }

    /// `executor(node, ctx)` as the promise `await` sees; `Err` is a
    /// synchronous throw.
    fn invoke_executor(
        &self,
        executor: &ExecutorKind,
        node: &Value,
        ctx: &ActionContext,
    ) -> Result<Promise<ActionExecutorResult>, Error> {
        match executor {
            ExecutorKind::Registered(run) => run(node, ctx),
            ExecutorKind::Script => {
                Ok(Promise::spawn(self.clone().script_executor(node.clone(), ctx.clone())))
            }
        }
    }

    async fn run(
        self,
        tree: Value,
        action_ctx: ActionContext,
    ) -> Result<ActionDispatchResult, Error> {
        let diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let diagnose = self.collector(&diagnostics);

        // The law: never execute an incomplete tree, not even its root.
        if truthy(get(&tree, "incomplete")) {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::IncompleteTree,
                "refused: the action tree is incomplete",
            ));
            let diagnostics = diagnostics.borrow().clone();
            return Ok(ActionDispatchResult {
                status: ActionResultStatus::Refused,
                nodes: Vec::new(),
                diagnostics,
            });
        }
        let Some(root) = get(&tree, "root").filter(|r| truthy(Some(r))).cloned() else {
            let diagnostics = diagnostics.borrow().clone();
            return Ok(ActionDispatchResult {
                status: ActionResultStatus::Inert,
                nodes: Vec::new(),
                diagnostics,
            });
        };

        let walk = Rc::new(Walk {
            this: self.clone(),
            ctx: action_ctx,
            diagnose,
            nodes: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            document_failed: Cell::new(false),
        });
        rt::returned(walk.clone().walk(root, Vec::new())).await?;
        if !walk.document_failed.get() {
            let deferred = std::mem::take(&mut *walk.deferred.borrow_mut());
            for entry in deferred {
                rt::returned(walk.clone().fire(entry)).await?;
            }
        }
        let nodes = walk.nodes.borrow().clone();
        let any_executed = nodes.iter().any(|n| n.status == ActionNodeStatus::Executed);
        let any_failed_or_skipped = nodes
            .iter()
            .any(|n| matches!(n.status, ActionNodeStatus::Failed | ActionNodeStatus::Skipped));
        let status = if any_executed {
            if any_failed_or_skipped {
                ActionResultStatus::Partial
            } else {
                ActionResultStatus::Executed
            }
        } else if any_failed_or_skipped {
            ActionResultStatus::Partial
        } else {
            ActionResultStatus::Inert
        };
        let diagnostics = diagnostics.borrow().clone();
        Ok(ActionDispatchResult { status, nodes, diagnostics })
    }

    /// run, then the counter, then the event: the shared per-tree unit.
    ///
    /// Boxed: a tree can print, and printing runs the will-print tree.
    fn run_and_emit(
        self,
        tree: Value,
        action_ctx: ActionContext,
    ) -> LocalFuture<Result<ActionDispatchResult, Error>> {
        Box::pin(async move {
            let result = rt::returned(self.clone().run(tree.clone(), action_ctx.clone())).await?;
            self.0.ctx.dispatch(ActionsAction::ActionsDispatched);
            self.0.action_hook.emit(&ActionDispatchEvent {
                ctx: action_ctx,
                tree,
                result: result.clone(),
            });
            Ok(result)
        })
    }

    /// Execute one tree as one queued dispatch.
    pub fn execute(
        &self,
        tree: PdfActionTree,
        action_ctx: ActionContext,
    ) -> Promise<ActionDispatchResult> {
        // Before enqueueing: a real user gesture arms the open-sequence latch
        // (its op then lands ahead of this one) and resets the cascade count.
        if action_ctx.origin == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        self.0.queue.run(move || {
            this.0.js_nodes_this_dispatch.set(0.0);
            Ok(Promise::spawn(this.clone().run_and_emit(tree, action_ctx)))
        })
    }

    /// Would the dispatcher accept this tree's root and attempt it.
    pub fn can_execute(&self, tree: &PdfActionTree, action_ctx: &ActionContext) -> bool {
        if truthy(get(tree, "incomplete")) || !truthy(get(tree, "root")) {
            return false;
        }
        let root = get(tree, "root").cloned().unwrap_or(Value::Null);
        matches!(
            self.decision_for(&root, action_ctx.origin),
            Decision::Policy(ActionPolicyDecision::Allow | ActionPolicyDecision::Adapter)
        )
    }

    // ── trigger machinery ────────────────────────────────────────────────────

    fn trigger_enabled(&self, trigger: &ActionTrigger) -> bool {
        let gates = self.0.config.triggers.as_ref();
        match trigger {
            ActionTrigger::Activate { .. } => true,
            ActionTrigger::Annotation { .. } => gates.and_then(|g| g.annotation) != Some(false),
            ActionTrigger::Page { .. } => gates.and_then(|g| g.page) != Some(false),
            ActionTrigger::Document { .. } => gates.and_then(|g| g.document) != Some(false),
        }
    }

    /// Per-page cache of annotations bearing page-lifecycle trees.
    async fn lifecycle_trees_for(self, pon: f64) -> Result<Vec<LifecycleAnnot>, Error> {
        let hit =
            self.0.lifecycle_cache.borrow().iter().find(|(p, _)| *p == pon).map(|(_, v)| v.clone());
        if let Some(hit) = hit {
            return Ok(hit);
        }
        let Some(doc) = self.doc() else { return Ok(Vec::new()) };
        doc.page_index(pon as u32)?;
        let listed =
            await_engine(doc.call("page.annotations.list", Some(pon as u32), Vec::new())).await?;
        let bearing: Vec<LifecycleAnnot> = get(&listed, "annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| {
                let actions = get(a, "actions");
                truthy(actions)
                    && ["pageOpen", "pageClose", "pageVisible", "pageInvisible"].iter().any(|key| {
                        truthy(actions.and_then(|x| get(x, key)).and_then(|t| get(t, "root")))
                    })
            })
            .map(|a| LifecycleAnnot {
                annotation_ref: get(&a, "ref").cloned().unwrap_or(Value::Null),
                actions: get(&a, "actions").cloned().unwrap_or(Value::Null),
            })
            .collect();
        let mut cache = self.0.lifecycle_cache.borrow_mut();
        cache.retain(|(p, _)| *p != pon);
        cache.push((pon, bearing.clone()));
        Ok(bearing)
    }

    /// ISO order: PO after the page O, PC before the page C. Rootless trees
    /// are skipped.
    fn plan_page_steps(
        event: PageEventKind,
        pon: f64,
        page_actions: Option<&Value>,
        lifecycle: &[LifecycleAnnot],
    ) -> Vec<(ActionSource, Value)> {
        let page_source = ActionSource::Page { pon };
        let annot_steps = |key: &str| -> Vec<(ActionSource, Value)> {
            lifecycle
                .iter()
                .filter(|a| truthy(get(&a.actions, key).and_then(|t| get(t, "root"))))
                .map(|a| {
                    (
                        ActionSource::Annotation { annotation: a.annotation_ref.clone(), pon },
                        get(&a.actions, key).cloned().unwrap_or(Value::Null),
                    )
                })
                .collect()
        };
        let page_tree = |key: &str| -> Vec<(ActionSource, Value)> {
            match page_actions.and_then(|p| get(p, key)) {
                Some(tree) if truthy(get(tree, "root")) => {
                    vec![(page_source.clone(), tree.clone())]
                }
                _ => Vec::new(),
            }
        };
        match event {
            PageEventKind::Open => [page_tree("open"), annot_steps("pageOpen")].concat(),
            PageEventKind::Close => [annot_steps("pageClose"), page_tree("close")].concat(),
            PageEventKind::Visible => annot_steps("pageVisible"),
            PageEventKind::Invisible => annot_steps("pageInvisible"),
        }
    }

    /// Fold step statuses. A step failure never skips its siblings.
    fn fold_steps(
        steps: Vec<ActionStepResult>,
        diagnostics: Vec<ActionDiagnostic>,
    ) -> ActionTriggerResult {
        let all = |s: ActionResultStatus| steps.iter().all(|step| step.result.status == s);
        let status = if steps.is_empty() {
            ActionResultStatus::Inert
        } else if all(ActionResultStatus::Refused) {
            ActionResultStatus::Refused
        } else if all(ActionResultStatus::Inert) {
            ActionResultStatus::Inert
        } else if all(ActionResultStatus::Executed) {
            ActionResultStatus::Executed
        } else {
            ActionResultStatus::Partial
        };
        ActionTriggerResult { status, steps, diagnostics }
    }

    async fn run_steps(
        self,
        steps: Vec<(ActionSource, Value)>,
        origin: ActionOrigin,
        event: ActionTriggerEvent,
        diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>>,
    ) -> Result<ActionTriggerResult, Error> {
        let mut results = Vec::new();
        for (source, tree) in steps {
            let ctx = ActionContext { origin, source: source.clone(), event: event.clone() };
            let result = rt::returned(self.clone().run_and_emit(tree.clone(), ctx)).await?;
            results.push(ActionStepResult { source, tree, result });
        }
        Ok(Self::fold_steps(results, diagnostics.borrow().clone()))
    }

    /// Everything a trigger needs, reads included, inside the queued op.
    /// Never fails.
    async fn resolve_and_run(self, trigger: ActionTrigger) -> ActionTriggerResult {
        let diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let diagnose = self.collector(&diagnostics);
        let inert = |diagnostics: &Rc<RefCell<Vec<ActionDiagnostic>>>| ActionTriggerResult {
            status: ActionResultStatus::Inert,
            steps: Vec::new(),
            diagnostics: diagnostics.borrow().clone(),
        };
        let attempt: Result<ActionTriggerResult, Error> = async {
            let Some(doc) = self.doc() else { return Ok(inert(&diagnostics)) };
            if !self.trigger_enabled(&trigger) {
                let scope = match &trigger {
                    ActionTrigger::Activate { .. } => "activate",
                    ActionTrigger::Annotation { .. } => "annotation",
                    ActionTrigger::Page { .. } => "page",
                    ActionTrigger::Document { .. } => "document",
                };
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerDisabled,
                    format!("{scope}: trigger family disabled by config"),
                ));
                return Ok(inert(&diagnostics));
            }
            let origin = origin_of(&trigger);
            match &trigger {
                ActionTrigger::Activate { annotation_ref, pon, source }
                | ActionTrigger::Annotation { annotation_ref, pon, source, .. } => {
                    let event = match &trigger {
                        ActionTrigger::Annotation { event, .. } => event.as_str(),
                        _ => "activate",
                    };
                    doc.page_index(*pon as u32)?;
                    let listed = await_engine(doc.call(
                        "page.annotations.list",
                        Some(*pon as u32),
                        Vec::new(),
                    ))
                    .await?;
                    let annotations = get(&listed, "annotations")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let annotation = annotations.iter().find(|candidate| {
                        same_ref(get(candidate, "ref").unwrap_or(&Value::Null), annotation_ref)
                    });
                    let actions = annotation.and_then(|a| get(a, "actions"));
                    // /A takes precedence over /AA U: a shadowed U tree is
                    // silently inert.
                    if event == "mouseUp" && truthy(actions.and_then(|a| get(a, "activate"))) {
                        return Ok(inert(&diagnostics));
                    }
                    let tree = actions.and_then(|a| get(a, event)).cloned();
                    let runnable = tree
                        .as_ref()
                        .is_some_and(|t| truthy(get(t, "root")) || truthy(get(t, "incomplete")));
                    let Some(tree) = tree.filter(|_| runnable) else {
                        return Ok(inert(&diagnostics));
                    };
                    let source = source.clone().unwrap_or(ActionSource::Annotation {
                        annotation: annotation_ref.clone(),
                        pon: *pon,
                    });
                    rt::returned(self.clone().run_steps(
                        vec![(source, tree)],
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                    ))
                    .await
                }
                ActionTrigger::Page { event, pon } => {
                    let layout = self.0.ctx.document().and_then(|meta| {
                        meta.pages.as_array().and_then(|pages| {
                            pages
                                .iter()
                                .find(|p| {
                                    get(p, "pageObjectNumber").and_then(Value::as_f64) == Some(*pon)
                                })
                                .cloned()
                        })
                    });
                    let lifecycle = rt::returned(self.clone().lifecycle_trees_for(*pon)).await?;
                    let steps = Self::plan_page_steps(
                        *event,
                        *pon,
                        layout.as_ref().and_then(|l| get(l, "actions")),
                        &lifecycle,
                    );
                    rt::returned(self.clone().run_steps(
                        steps,
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                    ))
                    .await
                }
                ActionTrigger::Document { event } => {
                    if *event != DocumentTriggerEvent::Open {
                        return Ok(rt::returned(self.clone().run_document_event_op(
                            *event,
                            diagnostics.clone(),
                            diagnose.clone(),
                        ))
                        .await);
                    }
                    if self.0.open_fired.get() {
                        diagnose(ActionDiagnostic::new(
                            ActionDiagnosticCode::OpenSequenceReplayed,
                            "document open sequence already fired for this document",
                        ));
                        return Ok(inert(&diagnostics));
                    }
                    self.0.open_fired.set(true);
                    Ok(rt::returned(self.clone().run_open_sequence_op()).await)
                }
            }
        }
        .await;
        match attempt {
            Ok(result) => result,
            Err(error) => {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerFailed,
                    format!("trigger resolution failed: {}", error.message),
                ));
                ActionTriggerResult {
                    status: ActionResultStatus::Refused,
                    steps: Vec::new(),
                    diagnostics: diagnostics.borrow().clone(),
                }
            }
        }
    }

    /// Report a trigger. The queue slot is taken before this returns, so two
    /// dispatches execute in call order; the promise never rejects.
    pub fn dispatch(&self, trigger: ActionTrigger) -> Promise<ActionTriggerResult> {
        if origin_of(&trigger) == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        self.0.queue.run(move || {
            this.0.js_nodes_this_dispatch.set(0.0);
            Ok(Promise::spawn(async move { Ok(this.resolve_and_run(trigger).await) }))
        })
    }

    /// Family enabled and a document present.
    pub fn can_dispatch(&self, trigger: &ActionTrigger) -> bool {
        self.trigger_enabled(trigger) && self.doc().is_some()
    }

    // ── the lifecycle coordinator ────────────────────────────────────────────

    fn emit_for_report(&self, report: &PageStateReport) {
        if report.cause == PageStateCause::User {
            self.0.cascade_rounds.set(0.0);
        }
        let mut next_visible: Vec<f64> = Vec::new();
        for pon in &report.visible_pons {
            if !next_visible.contains(pon) {
                next_visible.push(*pon);
            }
        }
        let (changed_current, leaving, entering, previous_current) = {
            let last = self.0.last_emitted.borrow();
            let leaving: Vec<f64> =
                last.visible.iter().copied().filter(|p| !next_visible.contains(p)).collect();
            let entering: Vec<f64> =
                next_visible.iter().copied().filter(|p| !last.visible.contains(p)).collect();
            (report.current_pon != last.current_pon, leaving, entering, last.current_pon)
        };
        if !changed_current && leaving.is_empty() && entering.is_empty() {
            return;
        }
        // Track truth even when suppressed: the budget bounds emission.
        *self.0.last_emitted.borrow_mut() =
            LastEmitted { current_pon: report.current_pon, visible: next_visible };
        if report.cause == PageStateCause::Programmatic {
            self.0.cascade_rounds.set(self.0.cascade_rounds.get() + 1.0);
            if self.0.cascade_rounds.get() > CASCADE_CAP {
                self.emit_diagnostic(ActionDiagnostic::new(
                    ActionDiagnosticCode::CascadeBudget,
                    format!(
                        "page-lifecycle emission suppressed: {} consecutive programmatic rounds (cap {})",
                        num(self.0.cascade_rounds.get()),
                        num(CASCADE_CAP)
                    ),
                ));
                return;
            }
        }
        // close(old), invisible set, visible set, open(new).
        if changed_current && let Some(previous) = previous_current {
            let _ =
                self.dispatch(ActionTrigger::Page { event: PageEventKind::Close, pon: previous });
        }
        for pon in leaving {
            let _ = self.dispatch(ActionTrigger::Page { event: PageEventKind::Invisible, pon });
        }
        for pon in entering {
            let _ = self.dispatch(ActionTrigger::Page { event: PageEventKind::Visible, pon });
        }
        if changed_current && let Some(current) = report.current_pon {
            let _ = self.dispatch(ActionTrigger::Page { event: PageEventKind::Open, pon: current });
        }
    }

    /// Stage's page-truth push door (host lens).
    pub fn report_page_state(&self, report: PageStateReport) {
        if !report.placed {
            return;
        }
        if report.cause == PageStateCause::User {
            self.0.cascade_rounds.set(0.0);
        }
        if !self.0.barrier_open.get() {
            // Coalesce: only the latest pre-open state matters.
            *self.0.buffered_report.borrow_mut() = Some(report);
            return;
        }
        self.emit_for_report(&report);
    }

    fn release_barrier(&self, fire_fallback: bool) {
        self.0.barrier_open.set(true);
        let report = self.0.buffered_report.borrow_mut().take();
        if let Some(report) = report {
            self.emit_for_report(&report);
        } else if fire_fallback
            && self.open_sequence() == OpenSequence::Headless
            && let Some(first) = self.first_pon()
        {
            // The first-page fallback only in declared-headless mode.
            self.0.last_emitted.borrow_mut().current_pon = Some(first);
            let _ = self.dispatch(ActionTrigger::Page { event: PageEventKind::Open, pon: first });
        }
    }

    /// The open sequence body, inside the queue.
    async fn run_open_sequence_op(self) -> ActionTriggerResult {
        let mut diagnostics = Vec::new();
        let steps: Rc<RefCell<Vec<ActionStepResult>>> = Rc::default();
        let lifecycle_ctx = ActionContext {
            origin: ActionOrigin::Lifecycle,
            source: ActionSource::Document,
            event: ActionTriggerEvent::Document { name: DocumentTriggerEvent::Open },
        };
        let attempt: Result<(), Error> = async {
            let snapshot = self.read_document_actions().await?;
            let snapshot = snapshot.unwrap_or(Value::Null);
            if let Some(destination) = get(&snapshot, "openDestination").filter(|d| truthy(Some(d))) {
                // The initial view reuses the whole spine as a synthesized
                // lifecycle goto.
                let tree = json!({
                    "root": { "type": "goto", "subtype": "GoTo", "destination": destination, "next": [] },
                    "incomplete": false,
                    "warningFlags": 0,
                    "warnings": [],
                });
                let result = rt::returned(self.clone().run_and_emit(tree.clone(), lifecycle_ctx.clone())).await?;
                steps.borrow_mut().push(ActionStepResult { source: ActionSource::Document, tree, result });
            }
            if let Some(open_action) = get(&snapshot, "openAction")
                && (truthy(get(open_action, "root")) || truthy(get(open_action, "incomplete")))
            {
                let tree = open_action.clone();
                let result = rt::returned(self.clone().run_and_emit(tree.clone(), lifecycle_ctx.clone())).await?;
                steps.borrow_mut().push(ActionStepResult { source: ActionSource::Document, tree, result });
            }
            Ok(())
        }
        .await;
        if let Err(error) = attempt {
            let diagnostic = ActionDiagnostic::new(
                ActionDiagnosticCode::TriggerFailed,
                format!("open sequence failed: {}", error.message),
            );
            diagnostics.push(diagnostic.clone());
            self.emit_diagnostic(diagnostic);
        }
        // Release after the document steps, on every path. Page-open emission
        // enqueues behind this op.
        self.release_barrier(true);
        let steps = std::mem::take(&mut *steps.borrow_mut());
        Self::fold_steps(steps, diagnostics)
    }

    // ── document lifecycle events ────────────────────────────────────────────

    /// OpenAction never runs after a WillSave, WillPrint or WillClose script:
    /// an armed-but-unfired sequence runs inline first.
    async fn ensure_open_sequence_before_doc_event(self) {
        if self.0.open_fired.get() || self.open_sequence() == OpenSequence::Off {
            return;
        }
        self.0.open_fired.set(true);
        rt::returned(self.clone().run_open_sequence_op()).await;
    }

    /// One catalog lifecycle tree inside the current queue operation. Never
    /// fails: a read failure degrades to a diagnostic.
    async fn run_doc_event_tree_safe(
        self,
        event: DocumentTriggerEvent,
        diagnose: Diagnose,
    ) -> Option<ActionStepResult> {
        if self.0.config.triggers.as_ref().and_then(|g| g.document) == Some(false) {
            return None;
        }
        let key = match event {
            DocumentTriggerEvent::WillSave => "willSave",
            DocumentTriggerEvent::DidSave => "didSave",
            DocumentTriggerEvent::WillPrint => "willPrint",
            DocumentTriggerEvent::DidPrint => "didPrint",
            DocumentTriggerEvent::WillClose => "willClose",
            DocumentTriggerEvent::Open => "open",
        };
        let attempt: Result<Option<ActionStepResult>, Error> = async {
            rt::returned(self.clone().ensure_open_sequence_before_doc_event()).await;
            let snapshot = self.read_document_actions().await?;
            let tree = snapshot.as_ref().and_then(|s| get(s, key)).cloned();
            let runnable = tree
                .as_ref()
                .is_some_and(|t| truthy(get(t, "root")) || truthy(get(t, "incomplete")));
            let Some(tree) = tree.filter(|_| runnable) else { return Ok(None) };
            let ctx = ActionContext {
                origin: ActionOrigin::Lifecycle,
                source: ActionSource::Document,
                event: ActionTriggerEvent::Document { name: event },
            };
            let result = rt::returned(self.clone().run_and_emit(tree.clone(), ctx)).await?;
            Ok(Some(ActionStepResult { source: ActionSource::Document, tree, result }))
        }
        .await;
        match attempt {
            Ok(step) => step,
            Err(error) => {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerFailed,
                    format!("{}: {}", event.as_str(), error.message),
                ));
                None
            }
        }
    }

    async fn run_document_event_op(
        self,
        event: DocumentTriggerEvent,
        diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>>,
        diagnose: Diagnose,
    ) -> ActionTriggerResult {
        let step = rt::returned(self.clone().run_doc_event_tree_safe(event, diagnose)).await;
        Self::fold_steps(step.into_iter().collect(), diagnostics.borrow().clone())
    }

    /// Both adapter print invocations go through here: will-print, the
    /// adapter exactly once, did-print, with the latch held throughout. An
    /// adapter throw skips did-print; the latch still resets.
    async fn fire_print_through_adapter(
        self,
        ui_context: Option<ActionUiContext>,
        diagnose: Diagnose,
    ) -> Result<(ActionNodeStatus, Option<String>), Error> {
        if self.0.doc_print_event_active.get() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::ReentrantPrint,
                "print request during a document print event — suppressed (one dialog per request)",
            ));
            return Ok((ActionNodeStatus::Blocked, Some("reentrant print suppressed".into())));
        }
        if !self.allows_print() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::Blocked,
                "print: doc.print is not allowed",
            ));
            return Ok((ActionNodeStatus::Blocked, Some("doc.print is not allowed".into())));
        }
        if self.0.ui_adapter.borrow().is_none() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::NoAdapter,
                "print: no UI adapter installed",
            ));
            return Ok((ActionNodeStatus::NoExecutor, Some("no UI adapter installed".into())));
        }
        self.0.doc_print_event_active.set(true);
        let outcome: Result<(ActionNodeStatus, Option<String>), Error> = async {
            rt::returned(
                self.clone()
                    .run_doc_event_tree_safe(DocumentTriggerEvent::WillPrint, diagnose.clone()),
            )
            .await;
            let adapter = self.0.ui_adapter.borrow().clone();
            let Some(adapter) = adapter else {
                return Ok((
                    ActionNodeStatus::NoExecutor,
                    Some("UI adapter uninstalled mid-print".into()),
                ));
            };
            (adapter.print)(ui_context.as_ref())?;
            rt::returned(
                self.clone()
                    .run_doc_event_tree_safe(DocumentTriggerEvent::DidPrint, diagnose.clone()),
            )
            .await;
            Ok((ActionNodeStatus::Executed, None))
        }
        .await;
        self.0.doc_print_event_active.set(false);
        outcome
    }

    fn maybe_fire_open_sequence(&self) {
        if self.0.open_fired.get() {
            return;
        }
        if self.open_sequence() == OpenSequence::Off {
            // The sequence never runs, but feeds must not buffer forever.
            self.0.open_fired.set(true);
            self.release_barrier(false);
            return;
        }
        if self.0.ui_adapter.borrow().is_none()
            && self.open_sequence() != OpenSequence::Headless
            && !self.0.saw_user_activity.get()
        {
            return;
        }
        self.0.open_fired.set(true);
        let this = self.clone();
        let _ = self
            .0
            .queue
            .run(move || Ok(Promise::spawn(async move { Ok(this.run_open_sequence_op().await) })));
    }

    fn note_user_activity(&self) {
        self.0.cascade_rounds.set(0.0);
        if !self.0.saw_user_activity.get() {
            self.0.saw_user_activity.set(true);
            self.maybe_fire_open_sequence();
        }
    }

    // ── the rest of the surface ──────────────────────────────────────────────

    /// Identity-safe port install: the disposer clears the slot only while
    /// this adapter is still current; `None` force-clears.
    pub fn set_ui_adapter(&self, adapter: Option<Rc<ActionUiAdapter>>) -> Disposer {
        *self.0.ui_adapter.borrow_mut() = adapter.clone();
        // An adapter arriving is the open latch's usual release.
        if adapter.is_some() {
            self.maybe_fire_open_sequence();
        }
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current = match (&*inner.ui_adapter.borrow(), &adapter) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if current {
                *inner.ui_adapter.borrow_mut() = None;
            }
        })
    }

    /// Run one embedder-owned document verb as one serialized operation:
    /// before-event tree, `operation()`, after-event tree. `operation`
    /// returns `Err` for a synchronous throw.
    pub fn run_document_verb<T: Clone + 'static>(
        &self,
        verb: DocumentVerb,
        operation: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                this.0.js_nodes_this_dispatch.set(0.0);
                let diagnose = this.emitter();
                let (before, after) = match verb {
                    DocumentVerb::Save => {
                        (DocumentTriggerEvent::WillSave, DocumentTriggerEvent::DidSave)
                    }
                    DocumentVerb::Print => {
                        (DocumentTriggerEvent::WillPrint, DocumentTriggerEvent::DidPrint)
                    }
                };
                let body = {
                    let this = this.clone();
                    async move {
                        // A before-event failure never cancels the verb.
                        rt::returned(
                            this.clone().run_doc_event_tree_safe(before, diagnose.clone()),
                        )
                        .await;
                        // A throwing operation skips the after-event.
                        let value = operation()?.await?;
                        rt::returned(this.clone().run_doc_event_tree_safe(after, diagnose)).await;
                        Ok(value)
                    }
                };
                if verb != DocumentVerb::Print {
                    // `return body()`: the async function adopts it.
                    return adopt(Promise::spawn(body)).await;
                }
                // The print verb holds the latch for its whole body.
                this.0.doc_print_event_active.set(true);
                let value = rt::returned(body).await;
                this.0.doc_print_event_active.set(false);
                value
            }))
        })
    }

    /// The cooperative will-close door. Call `documents.close()` after it
    /// resolves.
    pub fn prepare_close(&self) -> Promise<ActionTriggerResult> {
        self.dispatch(ActionTrigger::Document { event: DocumentTriggerEvent::WillClose })
    }

    /// Sink 1 of the submit chain. Installing it does not release the open
    /// latch.
    pub fn set_submit_handler(&self, handler: Option<ActionSubmitHandler>) -> Disposer {
        *self.0.submit_handler.borrow_mut() = handler.clone();
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current = same_option(&inner.submit_handler.borrow(), &handler);
            if current {
                *inner.submit_handler.borrow_mut() = None;
            }
        })
    }

    pub fn on_action(
        &self,
        listener: impl Fn(&ActionDispatchEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.action_hook.on(listener)
    }

    pub fn on_diagnostic(
        &self,
        listener: impl Fn(&ActionDiagnostic) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.diagnostic_hook.on(listener)
    }

    pub fn on_script_diagnostic(
        &self,
        listener: impl Fn(&ScriptDiagnostic) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.script_diagnostic_hook.on(listener)
    }

    pub fn on_script_error(
        &self,
        listener: impl Fn(&ScriptExecutionError) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.script_error_hook.on(listener)
    }

    /// Host lens. Last-wins on duplicates, with a `duplicate-executor`
    /// diagnostic; the disposer removes the entry only while it is current.
    pub fn register_executor(&self, action_type: &str, executor: ActionExecutor) -> Disposer {
        if self.0.executors.borrow().contains_key(action_type) {
            self.emit_diagnostic(ActionDiagnostic::new(
                ActionDiagnosticCode::DuplicateExecutor,
                format!("executor for '{action_type}' replaced (last-wins)"),
            ));
        }
        let id = self.0.next_id.get();
        self.0.next_id.set(id + 1);
        self.0.executors.borrow_mut().insert(
            action_type.to_string(),
            Registered { id, kind: Rc::new(ExecutorKind::Registered(executor)) },
        );
        let (weak, action_type) = (Rc::downgrade(&self.0), action_type.to_string());
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current = inner.executors.borrow().get(&action_type).is_some_and(|r| r.id == id);
            if current {
                inner.executors.borrow_mut().remove(&action_type);
            }
        })
    }

    /// Host lens: the annotation plugin's commit sink.
    pub fn register_annot_commit_sink(&self, sink: AnnotCommitSink) -> Disposer {
        *self.0.annot_commit_sink.borrow_mut() = Some(sink.clone());
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current =
                inner.annot_commit_sink.borrow().as_ref().is_some_and(|s| Rc::ptr_eq(s, &sink));
            if current {
                *inner.annot_commit_sink.borrow_mut() = None;
            }
        })
    }

    /// Host lens: the form plugin's commit sink.
    pub fn register_form_commit_sink(&self, sink: FormCommitSink) -> Disposer {
        *self.0.form_commit_sink.borrow_mut() = Some(sink.clone());
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current =
                inner.form_commit_sink.borrow().as_ref().is_some_and(|s| Rc::ptr_eq(s, &sink));
            if current {
                *inner.form_commit_sink.borrow_mut() = None;
            }
        })
    }

    /// Host lens: whether `script_transaction` exists (scripting on and a
    /// sandbox supplied). Its presence is the form plugin's "scripting on".
    pub fn has_script_transaction(&self) -> bool {
        self.0.script_host.is_some()
    }

    /// Host lens: the realm transaction port, `None` without a script host.
    /// The body does prefetch, runs, sink commits and reconciliation before
    /// its promise settles.
    pub fn script_transaction<T: Clone + 'static>(
        &self,
        body: impl FnOnce(ScriptTransaction) -> Result<Promise<T>, Error> + 'static,
    ) -> Option<Promise<T>> {
        self.0.script_host.as_ref().map(|host| host.transaction(body))
    }

    /// Host lens: the form plugin's dataset resolver.
    pub fn register_submit_resolver(&self, resolver: SubmitResolver) -> Disposer {
        *self.0.submit_resolver.borrow_mut() = Some(resolver.clone());
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current =
                inner.submit_resolver.borrow().as_ref().is_some_and(|r| Rc::ptr_eq(r, &resolver));
            if current {
                *inner.submit_resolver.borrow_mut() = None;
            }
        })
    }
}

fn same_option(a: &Option<ActionSubmitHandler>, b: &Option<ActionSubmitHandler>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `runDocumentVerb`'s verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentVerb {
    Save,
    Print,
}

enum Decision {
    Policy(ActionPolicyDecision),
    Never,
    Unsupported,
}

enum Deferred {
    Submit(Value),
    External(Value),
    Navigation(Value),
}

/// One `run`'s walk state: the node results, the deferred thunks, and the
/// document-failure latch.
struct Walk {
    this: ActionsCapability,
    ctx: ActionContext,
    diagnose: Diagnose,
    nodes: RefCell<Vec<ActionNodeResult>>,
    deferred: RefCell<Vec<(usize, Deferred)>>,
    document_failed: Cell<bool>,
}

impl Walk {
    fn set(&self, index: usize, status: ActionNodeStatus, detail: Option<String>) {
        let mut nodes = self.nodes.borrow_mut();
        nodes[index].status = status;
        if let Some(detail) = detail.filter(|d| !d.is_empty()) {
            nodes[index].detail = Some(detail);
        }
    }

    fn settle(&self, index: usize, outcome: ActionExecutorResult) {
        let node_type = self.nodes.borrow()[index].node_type.clone();
        match outcome {
            ActionExecutorResult::Executed => {
                self.nodes.borrow_mut()[index].status = ActionNodeStatus::Executed
            }
            ActionExecutorResult::Inert { reason } => {
                {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes[index].status = ActionNodeStatus::Inert;
                    nodes[index].detail = Some(reason.clone());
                }
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorInert,
                    format!("{node_type}: {reason}"),
                ));
            }
            ActionExecutorResult::Failed { error } => {
                {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes[index].status = ActionNodeStatus::Failed;
                    nodes[index].detail = Some(error.clone());
                }
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("{node_type}: {error}"),
                ));
            }
        }
    }

    /// `try { settle(result, await executor(node, ctx)) } catch ...`.
    async fn execute_with(&self, index: usize, executor: Rc<ExecutorKind>, node: &Value) {
        let outcome = match self.this.invoke_executor(&executor, node, &self.ctx) {
            Ok(promise) => promise.await,
            Err(error) => Err(error),
        };
        self.settle(
            index,
            outcome.unwrap_or_else(|error| ActionExecutorResult::Failed { error: error.message }),
        );
    }

    fn status(&self, index: usize) -> ActionNodeStatus {
        self.nodes.borrow()[index].status
    }

    async fn interpret(self: Rc<Self>, node: Value, path: Vec<usize>) -> Result<(), Error> {
        let kind = node_type(&node);
        let index = {
            let mut nodes = self.nodes.borrow_mut();
            nodes.push(ActionNodeResult {
                path,
                node_type: kind.clone(),
                status: ActionNodeStatus::Blocked,
                detail: None,
            });
            nodes.len() - 1
        };
        let origin = self.ctx.origin;
        match self.this.decision_for(&node, origin) {
            Decision::Never => {
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!("{kind}: never executable"),
                ));
                return Ok(());
            }
            Decision::Policy(
                decision @ (ActionPolicyDecision::Block | ActionPolicyDecision::Report),
            ) => {
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!(
                        "{kind}: policy '{}' for origin '{}'",
                        decision.as_str(),
                        origin.as_str()
                    ),
                ));
                return Ok(());
            }
            Decision::Unsupported => {
                self.set(index, ActionNodeStatus::NoExecutor, None);
                return Ok(());
            }
            Decision::Policy(_) => {}
        }

        if kind == "hide" {
            // A document mutation now: an earlier failure skips it, and its
            // failure stops later document work.
            if self.document_failed.get() {
                self.set(index, ActionNodeStatus::Skipped, None);
                return Ok(());
            }
            let (status, detail) =
                rt::returned(self.this.clone().interpret_hide(node, self.diagnose.clone())).await?;
            self.set(index, status, detail);
            if status == ActionNodeStatus::Failed {
                self.document_failed.set(true);
            }
            return Ok(());
        }

        if DOCUMENT_TYPES.contains(&kind.as_str()) {
            if self.document_failed.get() {
                self.set(index, ActionNodeStatus::Skipped, None);
                return Ok(());
            }
            let Some(executor) = self.this.executor(&kind) else {
                let javascript = kind == "javascript";
                self.set(
                    index,
                    if javascript { ActionNodeStatus::Inert } else { ActionNodeStatus::NoExecutor },
                    None,
                );
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoExecutor,
                    format!(
                        "{kind}: no executor registered{}",
                        if javascript { " (scripting unavailable)" } else { "" }
                    ),
                ));
                return Ok(());
            };
            self.execute_with(index, executor, &node).await;
            if self.status(index) == ActionNodeStatus::Failed {
                self.document_failed.set(true);
            }
            return Ok(());
        }

        if kind == "submit-form" {
            let Some(payload) = get(&node, "payload").filter(|p| truthy(Some(p))).cloned() else {
                // Older-runtime extraction: recognized-inert, honestly diagnosed.
                self.set(
                    index,
                    ActionNodeStatus::Inert,
                    Some("submit payload unavailable (older runtime extraction)".into()),
                );
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::SubmitPayloadUnavailable,
                    "submit-form: payload unavailable (older runtime extraction) — node inert",
                ));
                return Ok(());
            };
            // External: deferred until every document node succeeded.
            self.set(index, ActionNodeStatus::Skipped, None);
            self.deferred.borrow_mut().push((index, Deferred::Submit(payload)));
            return Ok(());
        }

        if is_print_verb(&node) || kind == "uri" {
            self.set(index, ActionNodeStatus::Skipped, None);
            self.deferred.borrow_mut().push((index, Deferred::External(node)));
            return Ok(());
        }

        // goto and the named page verbs: navigation, deferred.
        self.set(index, ActionNodeStatus::Skipped, None);
        self.deferred.borrow_mut().push((index, Deferred::Navigation(node)));
        Ok(())
    }

    fn walk(self: Rc<Self>, node: Value, path: Vec<usize>) -> LocalFuture<Result<(), Error>> {
        Box::pin(async move {
            rt::returned(self.clone().interpret(node.clone(), path.clone())).await?;
            let children =
                get(&node, "next").and_then(Value::as_array).cloned().unwrap_or_default();
            for (index, child) in children.into_iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(index);
                rt::returned(self.clone().walk(child, child_path)).await?;
            }
            Ok(())
        })
    }

    async fn fire(self: Rc<Self>, (index, entry): (usize, Deferred)) -> Result<(), Error> {
        match entry {
            Deferred::Submit(payload) => {
                let intent = ActionsCapability::intent_of_payload(&payload);
                let (status, detail) = rt::returned(self.this.clone().perform_submit(
                    intent,
                    self.ctx.clone(),
                    self.diagnose.clone(),
                ))
                .await;
                self.set(index, status, detail);
            }
            Deferred::External(node) if node_type(&node) == "uri" => {
                let adapter = self.this.0.ui_adapter.borrow().clone();
                let Some(adapter) = adapter else {
                    self.set(index, ActionNodeStatus::NoExecutor, None);
                    (self.diagnose)(ActionDiagnostic::new(
                        ActionDiagnosticCode::NoAdapter,
                        "uri: no UI adapter installed",
                    ));
                    return Ok(());
                };
                (adapter.open_uri)(
                    str_of(&node, "uri"),
                    &UriOptions {
                        is_map: get(&node, "isMap").and_then(Value::as_bool).unwrap_or(false),
                        origin: self.ctx.origin,
                    },
                )?;
                self.set(index, ActionNodeStatus::Executed, None);
            }
            Deferred::External(_) => {
                // The Print verb: will-print, the adapter once, did-print.
                let (status, detail) = rt::returned(
                    self.this.clone().fire_print_through_adapter(None, self.diagnose.clone()),
                )
                .await?;
                self.set(index, status, detail);
            }
            Deferred::Navigation(node) => {
                let kind = node_type(&node);
                let Some(executor) = self.this.executor(&kind) else {
                    self.set(index, ActionNodeStatus::NoExecutor, None);
                    (self.diagnose)(ActionDiagnostic::new(
                        ActionDiagnosticCode::NoExecutor,
                        format!("{kind}: no executor registered"),
                    ));
                    return Ok(());
                };
                self.execute_with(index, executor, &node).await;
            }
        }
        Ok(())
    }
}
