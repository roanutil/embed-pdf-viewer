//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin (and the hover-pump driver) from a vector and calls it by name.
//! `tooling/plugin-conformance/src/suites/actions.ts` is the TypeScript twin.

use std::any::Any;
use std::rc::Rc;

use epdf_acrojs::harness::{
    answer_promise, scripted_callback, scripted_sandbox_factory, to_json, typed, untyped,
};
use epdf_acrojs::{
    PartialScriptIdentity, ScriptBudget, ScriptEventInput, ScriptEventKind, ScriptInput,
    ScriptOutput, ScriptSandbox, ScriptSandboxFactory, ScriptTransaction, ScriptWorldInput,
};
use epdf_kernel::engine::decode;
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Controller, Error, Token, define, is_undefined, undefined};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::cell::RefCell;

use crate::hover_pump::{HoverPump, HoverTarget, create_hover_pump};
use crate::*;

pub struct Actions;

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn decoded<T: DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    decode(arg(args, i).cloned().unwrap_or(Value::Null))
}

/// A callback's answer as a typed promise; a value is already resolved.
fn answer_to<T: DeserializeOwned + Clone + 'static>(answer: Answer) -> Result<Promise<T>, Error> {
    match answer {
        Answer::Returns(value) => decode(value).map(Promise::resolved),
        Answer::Throws(error) => Err(error),
        Answer::Promise(promise) => Ok(typed(promise)),
    }
}

/// A callback whose return value the caller ignores: only a throw counts.
fn ignored(answer: Answer) -> Result<(), Error> {
    match answer {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }
}

fn function_marker() -> Value {
    json!({ "$function": true })
}

fn disposer_ref(h: &Harness, disposer: Disposer) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        disposer();
        Outcome::undefined()
    })))
}

fn unsubscribe_ref(h: &Harness, unsubscribe: epdf_kernel::Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        unsubscribe.call();
        Outcome::undefined()
    })))
}

fn listener<T: Serialize + 'static>(
    callback: Callback,
) -> impl Fn(&T) -> Result<(), Error> + 'static {
    move |event: &T| ignored(callback.call(vec![to_json(event)]))
}

fn number_fn(callback: Callback) -> Rc<dyn Fn() -> f64> {
    Rc::new(move || match callback.call(Vec::new()) {
        Answer::Returns(value) => value.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    })
}

fn scripted(spec: &Value, h: &Harness) -> Callback {
    scripted_callback(spec.get("$fn").and_then(Value::as_str).unwrap_or_default(), spec, h)
}

// ── a live sandbox ────────────────────────────────────────────────────────────

/// A live host's sandbox: `sandbox: {"$fn": ...}` in the `javascript`
/// config, a factory whose promise resolves to `{disposed, boot, run,
/// dispose}`, each a `$fn` the host answers synchronously (the eval build's
/// QuickJS sandbox, `packages/core-wasm/src/sandbox.ts`). Only a live
/// harness reads it; a vector's sandbox is never a `$fn`.
fn live_sandbox_factory(spec: &Value, h: &Harness) -> ScriptSandboxFactory {
    let factory = h.callback(spec);
    let h = h.clone();
    Rc::new(move || {
        let sandbox = match factory.call(Vec::new()) {
            Answer::Returns(value) => Promise::resolved(value),
            Answer::Throws(error) => Promise::rejected(error),
            Answer::Promise(promise) => promise,
        };
        let h = h.clone();
        sandbox.map(move |value| LiveSandbox::from_host(&value, &h))
    })
}

struct LiveSandbox {
    disposed: Callback,
    boot: Callback,
    run: Callback,
    dispose: Callback,
}

impl LiveSandbox {
    fn from_host(value: &Value, h: &Harness) -> Result<Rc<dyn ScriptSandbox>, Error> {
        let member = |name: &str| match value.get(name).filter(|f| f.get("$fn").is_some()) {
            Some(spec) => Ok(h.callback(spec)),
            None => Err(Error::named("TypeError", format!("sandbox.{name} is not a function"))),
        };
        Ok(Rc::new(LiveSandbox {
            disposed: member("disposed")?,
            boot: member("boot")?,
            run: member("run")?,
            dispose: member("dispose")?,
        }))
    }

    /// `boot` or `run` (`member`), with the budget as the last argument.
    /// Both hosts always pass one (`crates/plugins/acrojs/src/host.rs` and
    /// `packages/core/acrojs/src/host.ts`); a `None` would leave it off.
    fn output(
        member: &str,
        callback: &Callback,
        mut args: Vec<Value>,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        args.extend(budget.map(to_json));
        match callback.call(args) {
            Answer::Returns(value) => decode(value),
            Answer::Throws(error) => Err(error),
            Answer::Promise(_) => {
                Err(Error::named("TypeError", format!("sandbox.{member} answered a promise")))
            }
        }
    }
}

impl ScriptSandbox for LiveSandbox {
    fn disposed(&self) -> bool {
        matches!(self.disposed.call(Vec::new()), Answer::Returns(Value::Bool(true)))
    }

    fn boot(
        &self,
        sources: &[String],
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        LiveSandbox::output("boot", &self.boot, vec![json!(sources), to_json(input)], budget)
    }

    fn run(
        &self,
        source: &str,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        LiveSandbox::output("run", &self.run, vec![json!(source), to_json(input)], budget)
    }

    fn dispose(&self) {
        let _ = self.dispose.call(Vec::new());
    }
}

/// A vector identity: fields, or a scripted function answering them.
fn identity_of(identity: &Value, h: &Harness) -> Result<Option<ActionsScriptIdentity>, Error> {
    if identity.is_null() {
        return Ok(None);
    }
    if identity.get("$fn").is_some() {
        let callback = scripted(identity, h);
        return Ok(Some(ActionsScriptIdentity::Function(Rc::new(move || {
            match callback.call(Vec::new()) {
                Answer::Returns(value) => decode(value).unwrap_or_default(),
                _ => PartialScriptIdentity::default(),
            }
        }))));
    }
    Ok(Some(ActionsScriptIdentity::Value(decode(identity.clone())?)))
}

/// The vector's `javascript` config. `current` is the current fixture
/// (`actions-current-fixtures.ts`): `enabled` as given, and a `null` or
/// absent sandbox leaves the default factory. The legacy `actions` factory
/// (`actions.ts`) refuses scripting without a sandbox spec and turns it off
/// for a `null` one.
fn javascript_config(spec: &Value, h: &Harness, current: bool) -> Result<JavaScriptConfig, Error> {
    let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let sandbox = spec.get("sandbox").filter(|s| !is_undefined(s));
    if !current && enabled && sandbox.is_none() {
        return Err(Error::new(
            "[actions suite] javascript.enabled needs a scripted sandbox: the QuickJS default does not port",
        ));
    }
    let sandbox_factory = match sandbox {
        Some(sandbox) if h.is_live() && sandbox.get("$fn").is_some() => {
            Some(live_sandbox_factory(sandbox, h))
        }
        Some(sandbox) if !sandbox.is_null() => Some(scripted_sandbox_factory(sandbox, h)),
        _ => None,
    };
    let identity = match spec.get("identity").filter(|v| !is_undefined(v)) {
        None => None,
        Some(identity) => Some(identity_of(identity, h)?),
    };
    let text_fn = |key: &str| -> Option<Rc<dyn Fn() -> String>> {
        let callback = scripted(spec.get(key)?, h);
        Some(Rc::new(move || match callback.call(Vec::new()) {
            Answer::Returns(value) => value.as_str().unwrap_or_default().to_string(),
            _ => String::new(),
        }))
    };
    let number = |key: &str| spec.get(key).map(|s| number_fn(scripted(s, h)));
    Ok(JavaScriptConfig {
        enabled: Some(if current {
            enabled
        } else {
            enabled && sandbox.is_some_and(|s| !s.is_null())
        }),
        identity,
        sandbox_factory,
        file_name: text_fn("fileName"),
        now: number("now"),
        utc_offset_minutes: number("utcOffsetMinutes"),
        random_seed: number("randomSeed"),
        budget: match spec.get("budget") {
            Some(budget) => Some(decode(budget.clone())?),
            None => None,
        },
        max_script_nodes_per_dispatch: spec
            .get("maxScriptNodesPerDispatch")
            .and_then(Value::as_f64),
        host_utc_offset_minutes: None,
    })
}

/// The vector's plugin config as the plugin's.
pub fn actions_config(config: &Value, h: &Harness, current: bool) -> Result<ActionsConfig, Error> {
    let mut out = ActionsConfig::default();
    if is_undefined(config) || config.is_null() {
        return Ok(out);
    }
    if let Some(policy) = config.get("policy").filter(|v| !is_undefined(v)) {
        out.policy = Some(decode(policy.clone())?);
    }
    if let Some(triggers) = config.get("triggers").filter(|v| !is_undefined(v)) {
        out.triggers = Some(decode(triggers.clone())?);
    }
    if let Some(open_sequence) = config.get("openSequence").filter(|v| !is_undefined(v)) {
        out.open_sequence = Some(decode(open_sequence.clone())?);
    }
    if let Some(javascript) = config.get("javascript").filter(|v| !is_undefined(v)) {
        out.javascript = Some(javascript_config(javascript, h, current)?);
    }
    Ok(out)
}

/// A `DeepPartial<ActionsSettings>` from the vector.
fn settings_patch(value: &Value, h: &Harness) -> Result<ActionsSettingsPatch, Error> {
    let member = |key: &str| value.get(key).filter(|v| !is_undefined(v));
    let javascript = match member("javascript") {
        None => None,
        Some(js) => Some(JavaScriptSettingsPatch {
            enabled: js.get("enabled").and_then(Value::as_bool),
            identity: match js.get("identity").filter(|v| !is_undefined(v)) {
                None => None,
                Some(identity) => Some(identity_of(identity, h)?),
            },
        }),
    };
    Ok(ActionsSettingsPatch {
        policy: member("policy").map(|p| decode(p.clone())).transpose()?,
        triggers: member("triggers").map(|t| decode(t.clone())).transpose()?,
        open_sequence: member("openSequence").map(|o| decode(o.clone())).transpose()?,
        javascript,
    })
}

/// The settings as the TypeScript prints them.
fn settings_json(settings: &ActionsSettings) -> Value {
    let identity = match &settings.javascript.identity {
        None => Value::Null,
        Some(ActionsScriptIdentity::Value(fields)) => to_json(fields),
        Some(ActionsScriptIdentity::Function(_)) => function_marker(),
    };
    json!({
        "policy": to_json(&settings.policy),
        "triggers": to_json(&settings.triggers),
        "openSequence": settings.open_sequence.as_str(),
        "javascript": { "enabled": settings.javascript.enabled, "identity": identity },
    })
}

fn ui_adapter(spec: &Value, h: &Harness) -> Option<Rc<ActionUiAdapter>> {
    if spec.is_null() {
        return None;
    }
    let member = |key: &str| spec.get(key).map(|s| h.callback(s));
    let open_uri = member("openUri");
    let print = member("print");
    let alert = member("alert");
    let goto_page = member("gotoPage");
    Some(Rc::new(ActionUiAdapter {
        open_uri: Rc::new(move |uri: &str, options: &UriOptions| match &open_uri {
            Some(callback) => ignored(callback.call(vec![json!(uri), to_json(options)])),
            None => Err(Error::named("TypeError", "ports.uiAdapter.openUri is not a function")),
        }),
        print: Rc::new(move |ctx: Option<&ActionUiContext>| match &print {
            Some(callback) => ignored(callback.call(ctx.map(to_json).into_iter().collect())),
            None => Err(Error::named("TypeError", "adapter.print is not a function")),
        }),
        alert: alert.map(|callback| {
            Rc::new(move |message: &str, options: &AlertOptions| {
                ignored(callback.call(vec![json!(message), to_json(options)]))
            }) as Rc<dyn Fn(&str, &AlertOptions) -> Result<(), Error>>
        }),
        goto_page: goto_page.map(|callback| {
            Rc::new(move |page: f64, ctx: &ActionUiContext| {
                ignored(callback.call(vec![json!(page), to_json(ctx)]))
            }) as Rc<dyn Fn(f64, &ActionUiContext) -> Result<(), Error>>
        }),
    }))
}

fn executor(callback: Callback) -> ActionExecutor {
    Rc::new(move |node: &Value, ctx: &ActionContext| {
        answer_to(callback.call(vec![node.clone(), to_json(ctx)]))
    })
}

fn submit_handler(callback: Callback) -> ActionSubmitHandler {
    Rc::new(move |request: &ActionSubmitRequest, ctx: SubmitHandlerContext| {
        let home =
            if ctx.submit_to_document_home.is_some() { function_marker() } else { Value::Null };
        match callback.call(vec![to_json(request), json!({ "submitToDocumentHome": home })]) {
            Answer::Returns(_) => Ok(None),
            Answer::Throws(error) => Err(error),
            Answer::Promise(promise) => Ok(Some(promise)),
        }
    })
}

fn submit_resolver(callback: Callback) -> SubmitResolver {
    Rc::new(
        move |intent: &SubmitIntent,
              ctx: &ActionContext,
              _diagnose: Rc<dyn Fn(ActionDiagnostic)>| {
            answer_to(callback.call(vec![to_json(intent), to_json(ctx), function_marker()]))
        },
    )
}

fn form_reader(callback: Callback) -> FormReader {
    Rc::new(move || match callback.call(Vec::new()) {
        Answer::Returns(value) if !value.is_null() && !is_undefined(&value) => Some(Rc::new(value)),
        _ => None,
    })
}

/// The capability's members in the TypeScript `composeApi` order. The two
/// realm members exist only with scripting on.
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "registerExecutor",
    "registerAnnotCommitSink",
    "registerFormCommitSink",
    "registerFormReader",
    "setSubmitHandler",
    "registerSubmitResolver",
    "isScriptingEnabled",
    "scriptRealm",
    "createDetachedScriptRealm",
    "surfaceScriptResult",
    "surfaceScriptCommit",
    "reportPageState",
    "setUiAdapter",
    "runDocumentVerb",
    "execute",
    "canExecute",
    "executeNamed",
    "canExecuteNamed",
    "getActionTree",
    "dispatch",
    "canDispatch",
    "prepareClose",
    "onExecuted",
    "onDiagnosticReported",
    "onScriptDiagnosticReported",
    "onScriptFailed",
    "onOpenSequenceCompleted",
];

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn partial_context(args: &[Value], i: usize) -> Result<Option<PartialActionContext>, Error> {
    match arg(args, i) {
        Some(context) => Ok(Some(decode(context.clone())?)),
        None => Ok(None),
    }
}

fn call_actions(
    cap: &ActionsCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let callback = |i: usize| h.callback(arg(args, i).unwrap_or(&Value::Null));
    Ok(match method {
        "getSettings" => Outcome::Returned(settings_json(&cap.get_settings())),
        "updateSettings" => {
            cap.update_settings(&settings_patch(arg(args, 0).unwrap_or(&json!({})), h)?);
            Outcome::undefined()
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let listener = callback(0);
            unsubscribe_ref(
                h,
                cap.on_settings_changed(move |event| {
                    ignored(listener.call(vec![json!({
                        "settings": settings_json(&event.settings),
                        "changed": event.changed,
                    })]))
                }),
            )
        }
        "isScriptingEnabled" => Outcome::Returned(json!(cap.is_scripting_enabled())),
        "executeNamed" => {
            let name = arg(args, 0).and_then(Value::as_str).unwrap_or_default().to_string();
            Outcome::Promise(untyped(cap.execute_named(&name, partial_context(args, 1)?, None)))
        }
        "canExecuteNamed" => {
            let name = arg(args, 0).and_then(Value::as_str).unwrap_or_default();
            Outcome::Returned(json!(cap.can_execute_named(name, partial_context(args, 1)?)))
        }
        "getActionTree" => Outcome::Promise(cap.get_action_tree(decoded(args, 0)?, None)),
        "surfaceScriptCommit" => {
            match cap.surface_script_commit(&decoded(args, 0)?, &decoded(args, 1)?) {
                Ok(()) => Outcome::undefined(),
                Err(error) => Outcome::Threw(error),
            }
        }
        "execute" => Outcome::Promise(untyped(cap.execute(
            arg(args, 0).cloned().unwrap_or(Value::Null),
            decoded(args, 1)?,
            None,
        ))),
        "canExecute" => Outcome::Returned(json!(
            cap.can_execute(arg(args, 0).unwrap_or(&Value::Null), &decoded(args, 1)?)
        )),
        "dispatch" => Outcome::Promise(untyped(cap.dispatch(decoded(args, 0)?, None))),
        "canDispatch" => Outcome::Returned(json!(cap.can_dispatch(&decoded(args, 0)?))),
        "setUiAdapter" => {
            disposer_ref(h, cap.set_ui_adapter(ui_adapter(arg(args, 0).unwrap_or(&Value::Null), h)))
        }
        "runDocumentVerb" => {
            let verb: DocumentVerb = decoded(args, 0)?;
            let operation = callback(1);
            Outcome::Promise(cap.run_document_verb(
                verb,
                move || answer_promise(operation.call(Vec::new())),
                None,
            ))
        }
        "prepareClose" => Outcome::Promise(untyped(cap.prepare_close(None))),
        "setSubmitHandler" => {
            let handler =
                arg(args, 0).filter(|v| !v.is_null()).map(|_| submit_handler(callback(0)));
            disposer_ref(h, cap.set_submit_handler(handler))
        }
        "onExecuted" => unsubscribe_ref(h, cap.on_executed(listener(callback(0)))),
        "onOpenSequenceCompleted" => {
            unsubscribe_ref(h, cap.on_open_sequence_completed(listener(callback(0))))
        }
        "onDiagnosticReported" => {
            unsubscribe_ref(h, cap.on_diagnostic_reported(listener(callback(0))))
        }
        "onScriptDiagnosticReported" => {
            unsubscribe_ref(h, cap.on_script_diagnostic_reported(listener(callback(0))))
        }
        "onScriptFailed" => unsubscribe_ref(h, cap.on_script_failed(listener(callback(0)))),
        "registerExecutor" => {
            let action_type = arg(args, 0).and_then(Value::as_str).unwrap_or_default().to_string();
            disposer_ref(h, cap.register_executor(&action_type, executor(callback(1))))
        }
        "registerAnnotCommitSink" => {
            let sink = callback(0);
            disposer_ref(
                h,
                cap.register_annot_commit_sink(Rc::new(move |entries: Vec<AnnotCommitEntry>| {
                    answer_to(sink.call(vec![to_json(&entries)]))
                })),
            )
        }
        "registerFormCommitSink" => {
            let sink = callback(0);
            disposer_ref(
                h,
                cap.register_form_commit_sink(Rc::new(move |effects: Vec<Value>| {
                    answer_to(sink.call(vec![Value::Array(effects)]))
                })),
            )
        }
        "registerFormReader" => disposer_ref(h, cap.register_form_reader(form_reader(callback(0)))),
        "surfaceScriptResult" => match cap.surface_script_result(&decoded(args, 0)?) {
            Ok(()) => Outcome::undefined(),
            Err(error) => Outcome::Threw(error),
        },
        "reportPageState" => {
            cap.report_page_state(decoded(args, 0)?);
            Outcome::undefined()
        }
        "registerSubmitResolver" => {
            disposer_ref(h, cap.register_submit_resolver(submit_resolver(callback(0))))
        }
        other => not_a_function(other),
    })
}

// ── the hover-pump driver ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct HoverPumpDriver {
    pump: HoverPump,
}

fn hover_pump_plugin(config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
    let token: Token<HoverPumpDriver> = Token::from_any(h.token("hover-pump", None));
    let dispatch_spec = config.get("dispatch").cloned().unwrap_or(Value::Null);
    let over_actions = dispatch_spec.as_str() == Some("actions");
    let callback = if over_actions { None } else { Some(scripted(&dispatch_spec, h)) };
    let mut builder = define::<Value, Value>("hover-pump").document_scoped();
    if over_actions {
        builder = builder.requires(ACTIONS.any());
    }
    Ok(builder
        .initial_state(undefined)
        .create(&token, move |cx| {
            let pump = match &callback {
                None => {
                    let cx = cx.clone();
                    create_hover_pump(move |trigger| match cx.get(&ACTIONS) {
                        Ok(actions) => actions.dispatch(trigger, None),
                        Err(error) => Promise::rejected(error),
                    })
                }
                Some(callback) => {
                    let callback = callback.clone();
                    create_hover_pump(move |trigger| {
                        answer_to(callback.call(vec![to_json(&trigger)]))
                            .unwrap_or_else(Promise::rejected)
                    })
                }
            };
            let reset = pump.clone();
            cx.cleanup_fn(move || reset.reset());
            Ok((HoverPumpDriver { pump }, None))
        })
        .build())
}

impl HarnessPlugin for Actions {
    fn factories(&self) -> Vec<&'static str> {
        vec!["actions", "actions-current", "hover-pump", "detached-realm"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "hover-pump" => hover_pump_plugin(config, h),
            "detached-realm" => detached_realm_plugin(config, h),
            "actions-current" => Ok(actions_plugin(actions_config(config, h, true)?)),
            _ => Ok(actions_plugin(actions_config(config, h, false)?)),
        }
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![ACTIONS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(driver) = capability.downcast_ref::<HoverPumpDriver>() {
            return Some(match method {
                "hover" => match arg(&args, 0).filter(|v| !v.is_null()) {
                    None => {
                        driver.pump.hover(None);
                        Outcome::undefined()
                    }
                    Some(target) => match decode::<HoverTarget>(target.clone()) {
                        Ok(target) => {
                            driver.pump.hover(Some(target));
                            Outcome::undefined()
                        }
                        Err(error) => Outcome::Threw(error),
                    },
                },
                "reset" => {
                    driver.pump.reset();
                    Outcome::undefined()
                }
                other => not_a_function(other),
            });
        }
        if let Some(driver) = capability.downcast_ref::<DetachedRealmDriver>() {
            return Some(driver.call(method, &args, h));
        }
        let cap = capability.downcast_ref::<ActionsCapability>()?;
        Some(call_actions(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<HoverPumpDriver>().is_some() {
            return Some(vec!["hover", "reset"]);
        }
        if capability.downcast_ref::<DetachedRealmDriver>().is_some() {
            return Some(vec![
                "transaction",
                "transactionThrows",
                "detachedTransaction",
                "dispose",
                "mint",
                "run",
            ]);
        }
        let cap = capability.downcast_ref::<ActionsCapability>()?;
        let scripting = cap.is_scripting_enabled();
        Some(
            MEMBERS
                .iter()
                .copied()
                .filter(|m| scripting || !matches!(*m, "scriptRealm" | "createDetachedScriptRealm"))
                .collect(),
        )
    }

    fn describe_member(
        &self,
        capability: &Rc<dyn Any>,
        member: &'static str,
        h: &Harness,
    ) -> Option<Value> {
        let cap = capability.downcast_ref::<ActionsCapability>()?;
        if member != "scriptRealm" {
            return None;
        }
        let port = cap.script_realm()?;
        Some(realm_json(h, port.clone(), port.budget.clone(), None))
    }
}

/// A realm port as the TypeScript prints it: `transaction` (and `dispose`
/// for a detached realm) are refs, `budget` a value. A transaction body is a
/// scripted callback, handed the transaction's two functions.
fn realm_json(
    h: &Harness,
    port: ScriptRealmPort,
    budget: epdf_acrojs::ScriptBudget,
    dispose: Option<DetachedScriptRealm>,
) -> Value {
    let harness = h.clone();
    let transaction = h.register_fn(Rc::new(move |args: Vec<Value>| {
        let body = harness.callback(args.first().unwrap_or(&Value::Null));
        Outcome::Promise(port.transaction(move |_txn: ScriptTransaction| {
            answer_promise(
                body.call(vec![json!({ "boot": function_marker(), "run": function_marker() })]),
            )
        }))
    }));
    let mut out = serde_json::Map::new();
    out.insert("transaction".into(), transaction);
    out.insert("budget".into(), to_json(&budget));
    if let Some(realm) = dispose {
        out.insert(
            "dispose".into(),
            h.register_fn(Rc::new(move |_| {
                realm.dispose();
                Outcome::undefined()
            })),
        );
    }
    Value::Object(out)
}

// ── the detached-realm driver ─────────────────────────────────────────────────

/// A sibling that mints a detached realm for its own document from the
/// actions plugin of `owner` (default `a`), as the stamp plugin does for an
/// asset, and drives its own document's realm port. `meta` replaces the
/// realm's document metadata (`null` is none); `bootSources` is a scripted
/// callback (default: no programs). `run` boots and runs one program in a
/// single transaction over an empty world.
#[derive(Clone)]
pub struct DetachedRealmDriver {
    cx: Controller<Value, Value>,
    owner: String,
    /// `undefined`: the document's own metadata; otherwise this (`null` is none).
    meta: Option<Option<ScriptDocument>>,
    boot_sources: Option<Callback>,
    /// `ctx.tryGet(ActionsToken)?.scriptRealm`, read at create.
    document_realm: Option<ScriptRealmPort>,
    realm: Rc<RefCell<Option<DetachedScriptRealm>>>,
}

fn empty_world() -> ScriptWorldInput {
    ScriptWorldInput {
        fields: Vec::new(),
        annots: None,
        annot_pages: None,
        annots_cover_document: None,
        page_number: None,
        event: ScriptEventInput::of(ScriptEventKind::WidgetActivate),
    }
}

/// `realm?.transaction(...)` on an absent realm: `undefined`.
fn transaction_on(
    realm: Option<&ScriptRealmPort>,
    body: impl FnOnce() -> Result<Promise<Value>, Error> + 'static,
) -> Outcome {
    match realm {
        Some(realm) => Outcome::Promise(realm.transaction(move |_| body())),
        None => Outcome::undefined(),
    }
}

impl DetachedRealmDriver {
    fn call(&self, method: &str, args: &[Value], h: &Harness) -> Outcome {
        let detached = || self.realm.borrow().clone();
        match method {
            "transaction" => transaction_on(self.document_realm.as_ref(), || {
                Ok(Promise::resolved(json!("body-result")))
            }),
            "transactionThrows" => transaction_on(self.document_realm.as_ref(), || {
                Ok(Promise::rejected(Error::new("body threw")))
            }),
            "detachedTransaction" => transaction_on(detached().as_ref().map(|r| &r.port), || {
                Ok(Promise::resolved(json!("body-result")))
            }),
            "dispose" => {
                if let Some(realm) = detached() {
                    realm.dispose();
                }
                Outcome::undefined()
            }
            "mint" => Outcome::from_result(self.mint(h)),
            "run" => {
                let program = arg(args, 0).and_then(Value::as_str).unwrap_or_default();
                match self.run(program.to_string()) {
                    Ok(promise) => Outcome::Promise(promise),
                    Err(error) => Outcome::Threw(error),
                }
            }
            other => not_a_function(other),
        }
    }

    fn mint(&self, h: &Harness) -> Result<Value, Error> {
        let actions = self.cx.try_for_document(&ACTIONS, &self.owner)?;
        let Some(actions) = actions.filter(|a| a.is_scripting_enabled()) else {
            return Ok(Value::Null);
        };
        if let Some(previous) = self.realm.borrow_mut().take() {
            previous.dispose();
        }
        let doc = self.cx.doc()?;
        let document: Rc<dyn Fn() -> Option<ScriptDocument>> = match &self.meta {
            None => {
                let cx = self.cx.clone();
                Rc::new(move || cx.document().map(ScriptDocument::from))
            }
            Some(meta) => {
                let meta = meta.clone();
                Rc::new(move || meta.clone())
            }
        };
        let boot_sources: Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>> =
            match &self.boot_sources {
                Some(callback) => {
                    let callback = callback.clone();
                    Rc::new(move || answer_to(callback.call(Vec::new())))
                }
                // `async () => []`.
                None => Rc::new(|| Ok(Promise::resolved(Vec::new()))),
            };
        let Some(realm) =
            actions.create_detached_script_realm(ScriptRealmTarget { doc, document, boot_sources })
        else {
            return Ok(Value::Null);
        };
        *self.realm.borrow_mut() = Some(realm.clone());
        Ok(realm_json(h, realm.port.clone(), realm.budget().clone(), Some(realm)))
    }

    fn run(&self, program: String) -> Result<Promise<Value>, Error> {
        let realm = self.realm.borrow().clone().ok_or_else(|| {
            Error::named("TypeError", "Cannot read properties of null (reading 'transaction')")
        })?;
        Ok(realm.transaction(move |txn: ScriptTransaction| {
            Ok(Promise::spawn(async move {
                let boot = txn.boot(empty_world(), None).await?;
                let run = txn.run(&program, empty_world(), None).await?;
                Ok(json!({ "boot": to_json(&boot), "run": to_json(&run) }))
            }))
        }))
    }
}

fn detached_realm_plugin(config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
    let token: Token<DetachedRealmDriver> = Token::from_any(h.token("detached-realm", None));
    let owner = config.get("owner").and_then(Value::as_str).unwrap_or("a").to_string();
    let meta: Option<Option<ScriptDocument>> = match config.get("meta") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(meta) => Some(Some(ScriptDocument {
            name: meta.get("name").and_then(Value::as_str).map(str::to_string),
            page_count: meta.get("pageCount").and_then(Value::as_u64).unwrap_or(0) as u32,
            pages: Rc::new(meta.get("pages").cloned().unwrap_or(json!([]))),
        })),
    };
    let boot_sources = config.get("bootSources").map(|spec| scripted(spec, h));
    Ok(define::<Value, Value>("detached-realm")
        .document_scoped()
        .optional(ACTIONS.any())
        .initial_state(undefined)
        .create(&token, move |cx| {
            let document_realm = cx.try_get(&ACTIONS)?.and_then(|a| a.script_realm());
            let driver = DetachedRealmDriver {
                cx: cx.clone(),
                owner: owner.clone(),
                meta: meta.clone(),
                boot_sources: boot_sources.clone(),
                document_realm,
                realm: Rc::default(),
            };
            let realm = driver.realm.clone();
            cx.cleanup_fn(move || {
                if let Some(realm) = realm.borrow().clone() {
                    realm.dispose();
                }
            });
            Ok((driver, None))
        })
        .build())
}

// ── the function table ────────────────────────────────────────────────────────

fn event_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&event_of(&decoded(args, 0)?)))
}

fn trigger_origin_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&trigger_origin_of(&decoded(args, 0)?)))
}

fn url_encoded_fn(args: &[Value]) -> Result<Value, Error> {
    let entries: Vec<FormSubmissionEntry> = decoded(args, 0)?;
    Ok(json!(submit_entries_to_url_encoded(&entries)))
}

/// The functions `vectors/plugins/actions.json` names.
pub fn functions() -> FunctionTable {
    vec![
        ("eventOf", event_of_fn),
        ("triggerOriginOf", trigger_origin_of_fn),
        ("submitEntriesToUrlEncoded", url_encoded_fn),
    ]
}
