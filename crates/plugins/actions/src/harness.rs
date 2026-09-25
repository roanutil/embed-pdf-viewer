//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin (and the hover-pump driver) from a vector and calls it by name.
//! `tooling/plugin-conformance/src/suites/actions.ts` is the TypeScript twin.

use std::any::Any;
use std::rc::Rc;

use epdf_acrojs::harness::{
    answer_promise, scripted_callback, scripted_sandbox_factory, to_json, typed, untyped,
};
use epdf_acrojs::{IdentityOverrides, PartialScriptIdentity, ScriptTransaction};
use epdf_kernel::engine::decode;
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, define, is_undefined};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

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

fn javascript_config(spec: &Value, h: &Harness) -> Result<JavaScriptConfig, Error> {
    let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let sandbox_factory = match spec.get("sandbox") {
        Some(sandbox) if !sandbox.is_null() => Some(scripted_sandbox_factory(sandbox, h)),
        // `null`: a host with no sandbox at all.
        _ => None,
    };
    let identity = match spec.get("identity") {
        None => None,
        Some(identity) if identity.get("$fn").is_some() => {
            let callback = scripted(identity, h);
            Some(IdentityOverrides::Function(Rc::new(move || match callback.call(Vec::new()) {
                Answer::Returns(value) => decode(value).unwrap_or_default(),
                _ => PartialScriptIdentity::default(),
            })))
        }
        Some(identity) => Some(IdentityOverrides::Value(decode(identity.clone())?)),
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
        enabled,
        sandbox_factory,
        identity,
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
    })
}

/// The vector's plugin config as the plugin's.
pub fn actions_config(config: &Value, h: &Harness) -> Result<ActionsPluginConfig, Error> {
    let log = h.clone();
    let mut out = ActionsPluginConfig {
        observer_error: Some(Rc::new(move |message: &str, error: &Error| {
            log.log("console.error", vec![json!(message), error.to_canonical()]);
        })),
        ..Default::default()
    };
    if is_undefined(config) || config.is_null() {
        return Ok(out);
    }
    if let Some(policy) = config.get("policy") {
        out.policy = Some(decode(policy.clone())?);
    }
    if let Some(triggers) = config.get("triggers") {
        out.triggers = Some(decode(triggers.clone())?);
    }
    if let Some(open_sequence) = config.get("openSequence") {
        out.open_sequence = Some(decode(open_sequence.clone())?);
    }
    if let Some(javascript) = config.get("javascript") {
        out.javascript = Some(javascript_config(javascript, h)?);
    }
    Ok(out)
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
            None => Err(Error::named("TypeError", "uiAdapter.openUri is not a function")),
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

const MEMBERS: &[&str] = &[
    "execute",
    "canExecute",
    "dispatch",
    "canDispatch",
    "setUiAdapter",
    "runDocumentVerb",
    "prepareClose",
    "setSubmitHandler",
    "onAction",
    "onDiagnostic",
    "onScriptDiagnostic",
    "onScriptError",
    "registerExecutor",
    "registerAnnotCommitSink",
    "registerFormCommitSink",
    "scriptTransaction",
    "surfaceScriptResult",
    "reportPageState",
    "registerSubmitResolver",
];

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn call_actions(
    cap: &ActionsCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let callback = |i: usize| h.callback(arg(args, i).unwrap_or(&Value::Null));
    Ok(match method {
        "execute" => Outcome::Promise(untyped(
            cap.execute(arg(args, 0).cloned().unwrap_or(Value::Null), decoded(args, 1)?),
        )),
        "canExecute" => Outcome::Returned(json!(
            cap.can_execute(arg(args, 0).unwrap_or(&Value::Null), &decoded(args, 1)?)
        )),
        "dispatch" => Outcome::Promise(untyped(cap.dispatch(decoded(args, 0)?))),
        "canDispatch" => Outcome::Returned(json!(cap.can_dispatch(&decoded(args, 0)?))),
        "setUiAdapter" => {
            disposer_ref(h, cap.set_ui_adapter(ui_adapter(arg(args, 0).unwrap_or(&Value::Null), h)))
        }
        "runDocumentVerb" => {
            let verb: DocumentVerb = decoded(args, 0)?;
            let operation = callback(1);
            Outcome::Promise(
                cap.run_document_verb(verb, move || answer_promise(operation.call(Vec::new()))),
            )
        }
        "prepareClose" => Outcome::Promise(untyped(cap.prepare_close())),
        "setSubmitHandler" => {
            let handler =
                arg(args, 0).filter(|v| !v.is_null()).map(|_| submit_handler(callback(0)));
            disposer_ref(h, cap.set_submit_handler(handler))
        }
        "onAction" => unsubscribe_ref(h, cap.on_action(listener(callback(0)))),
        "onDiagnostic" => unsubscribe_ref(h, cap.on_diagnostic(listener(callback(0)))),
        "onScriptDiagnostic" => unsubscribe_ref(h, cap.on_script_diagnostic(listener(callback(0)))),
        "onScriptError" => unsubscribe_ref(h, cap.on_script_error(listener(callback(0)))),
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
        "scriptTransaction" => {
            let body = callback(0);
            let run = move |_txn: ScriptTransaction| {
                answer_promise(
                    body.call(vec![json!({ "boot": function_marker(), "run": function_marker() })]),
                )
            };
            match cap.script_transaction(run) {
                Some(promise) => Outcome::Promise(promise),
                None => not_a_function(method),
            }
        }
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
        .capability(&token, move |ctx| {
            let pump = match &callback {
                None => {
                    let ctx = ctx.clone();
                    create_hover_pump(move |trigger| match ctx.get(&ACTIONS) {
                        Ok(actions) => actions.dispatch(trigger),
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
            Ok(HoverPumpDriver { pump })
        })
        .build())
}

impl HarnessPlugin for Actions {
    fn factories(&self) -> Vec<&'static str> {
        vec!["actions", "hover-pump"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "hover-pump" => hover_pump_plugin(config, h),
            _ => Ok(actions_plugin(actions_config(config, h)?)),
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
        let cap = capability.downcast_ref::<ActionsCapability>()?;
        Some(call_actions(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<HoverPumpDriver>().is_some() {
            return Some(vec!["hover", "reset"]);
        }
        let cap = capability.downcast_ref::<ActionsCapability>()?;
        Some(
            MEMBERS
                .iter()
                .copied()
                .filter(|m| *m != "scriptTransaction" || cap.has_script_transaction())
                .collect(),
        )
    }
}

// ── the function table ────────────────────────────────────────────────────────

fn event_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&event_of(&decoded(args, 0)?)))
}

fn origin_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&origin_of(&decoded(args, 0)?)))
}

fn url_encoded_fn(args: &[Value]) -> Result<Value, Error> {
    let entries: Vec<FormSubmissionEntry> = decoded(args, 0)?;
    Ok(json!(submit_entries_to_url_encoded(&entries)))
}

/// The functions `vectors/plugins/actions.json` names.
pub fn functions() -> FunctionTable {
    vec![
        ("eventOf", event_of_fn),
        ("originOf", origin_of_fn),
        ("submitEntriesToUrlEncoded", url_encoded_fn),
    ]
}
