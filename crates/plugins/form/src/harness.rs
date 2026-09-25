//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin and its two test drivers from a vector and calls them by name,
//! plus the function table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/form-fixtures.ts`:
//!
//! - `console.warn` and `console.error` lines starting `[form]` log as the
//!   callbacks of those names (the `FormHost` sinks);
//! - `form-scripting` builds a standalone [`FormScriptingController`] over
//!   the document with a scripted sandbox;
//! - `form-probe` answers the annotation plugin's `behaviorFor` as the id of
//!   the engaged behavior.

use std::any::Any;
use std::rc::Rc;

use epdf_acrojs::harness::{scripted_callback, scripted_sandbox_factory};
use epdf_acrojs::{IdentityOverrides, PartialScriptIdentity};
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Ctx, Error, Token, define, is_undefined, undefined};
use epdf_plugin_actions::{ActionOrigin, PdfAnnotationEventKind};
use epdf_plugin_annotation::ANNOTATION;
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::capability::FormCapability;
use crate::core::fill_items::{fill_item_for_widget, fill_items, project_widget};
use crate::core::model::{
    Model, Msg, field_by_key, field_for_widget, field_key_of, initial_model, update,
};
use crate::field_selection::{build_submit_entries, resolve_field_selection};
use crate::form_plugin;
use crate::reducer::{form_reducer, initial_form_state};
use crate::scripting::{FormScriptingController, FormScriptingControllerOptions};
use crate::tools::{FORM_TOOLS, PLACE_TAGS, form_tool_by_id};
use crate::types::{FORM, FormAction, FormHost, FormPluginConfig, FormScriptingOptions, FormState};

/// The adapter.
pub struct Form;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "snapshot",
    "refresh",
    "fillItems",
    "fillItem",
    "ensureGeom",
    "field",
    "fieldForWidget",
    "setText",
    "toggle",
    "choose",
    "reset",
    "commitValue",
    "activateWidget",
    "notifyWidgetEvent",
    "resetFormAction",
    "resolveSubmitDataset",
    "commitScriptFormEffects",
    "setValue",
    "exportData",
    "importData",
    "repair",
    "placeField",
    "pageBox",
    "updateField",
    "deleteField",
    "detachWidget",
    "canRead",
    "canFill",
    "canDesign",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn value_arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or_else(undefined)
}

fn str_arg(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn num_arg(args: &[Value], i: usize) -> f64 {
    arg(args, i).and_then(epdf_acrojs::js::number).unwrap_or(f64::NAN)
}

fn decoded<T: serde::de::DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    from_value(args.get(i).unwrap_or(&undefined()))
}

fn json_of<T: Serialize>(value: &T) -> Value {
    to_value(value)
}

/// A typed promise as the JSON a trace records, in the same moment.
fn json_promise<T: Serialize + Clone + 'static>(promise: Promise<T>) -> Outcome {
    let (json, resolver) = Promise::pending();
    promise.on_settle(move |result| resolver.settle(result.clone().map(|v| to_value(&v))));
    Outcome::Promise(json)
}

fn unit_promise(promise: Promise<()>) -> Outcome {
    let (json, resolver) = Promise::pending();
    promise.on_settle(move |result| resolver.settle(result.clone().map(|_| undefined())));
    Outcome::Promise(json)
}

fn or_null(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn call_form(
    cap: &FormCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    Ok(match method {
        "snapshot" => Outcome::Returned(or_null(cap.snapshot())),
        "refresh" => unit_promise(cap.refresh_with(crate::value::truthy(arg(args, 0)))),
        "fillItems" => Outcome::Returned(json_of(&*cap.fill_items(num_arg(args, 0)))),
        "fillItem" => {
            Outcome::Returned(cap.fill_item(num_arg(args, 0)).map_or(Value::Null, |i| json_of(&*i)))
        }
        "ensureGeom" => {
            cap.ensure_geom(num_arg(args, 0));
            Outcome::undefined()
        }
        "field" => Outcome::Returned(or_null(cap.field(&str_arg(args, 0)))),
        "fieldForWidget" => Outcome::Returned(or_null(cap.field_for_widget(num_arg(args, 0)))),
        "setText" => unit_promise(cap.set_text(str_arg(args, 0), value_arg(args, 1))),
        "toggle" => unit_promise(cap.toggle(str_arg(args, 0), value_arg(args, 1))),
        "choose" => unit_promise(cap.choose(str_arg(args, 0), value_arg(args, 1))),
        "reset" => unit_promise(cap.reset(str_arg(args, 0))),
        "commitValue" => json_promise(cap.commit_value(value_arg(args, 0), value_arg(args, 1))),
        "activateWidget" => json_promise(cap.activate_widget(str_arg(args, 0), value_arg(args, 1))),
        "notifyWidgetEvent" => {
            let event: PdfAnnotationEventKind = decoded(args, 2)?;
            match cap.notify_widget_event(&str_arg(args, 0), &value_arg(args, 1), event) {
                Ok(()) => Outcome::undefined(),
                Err(error) => Outcome::Threw(error),
            }
        }
        "resetFormAction" => {
            let targets = arg(args, 0).and_then(Value::as_array).cloned();
            let exclude = crate::value::truthy(arg(args, 1));
            let origin: Option<ActionOrigin> = match arg(args, 2) {
                Some(origin) => Some(from_value(origin)?),
                None => None,
            };
            json_promise(cap.reset_form_action(targets, exclude, origin))
        }
        "resolveSubmitDataset" => {
            let diagnose = h.callback(arg(args, 2).unwrap_or(&Value::Null));
            json_promise(cap.resolve_submit_dataset(
                decoded(args, 0)?,
                decoded(args, 1)?,
                Rc::new(move |d| {
                    let _ = diagnose.call(vec![json_of(&d)]);
                }),
            ))
        }
        "commitScriptFormEffects" => {
            let effects = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
            json_promise(cap.commit_script_form_effects(effects))
        }
        "setValue" => json_promise(cap.set_value(value_arg(args, 0), value_arg(args, 1))),
        "exportData" => json_promise(cap.export_data(arg(args, 0).cloned())),
        "importData" => json_promise(cap.import_data(value_arg(args, 0), arg(args, 1).cloned())),
        "repair" => json_promise(cap.repair(arg(args, 0).cloned())),
        "placeField" => json_promise(cap.place_field(decoded(args, 0)?)),
        "pageBox" => {
            Outcome::Returned(cap.page_box(num_arg(args, 0)).map_or(Value::Null, |b| json_of(&b)))
        }
        "updateField" => unit_promise(cap.update_field(str_arg(args, 0), value_arg(args, 1))),
        "deleteField" => unit_promise(cap.delete_field(str_arg(args, 0))),
        "detachWidget" => unit_promise(cap.detach_widget(str_arg(args, 0), num_arg(args, 1))),
        "canRead" => Outcome::Returned(json!(cap.can_read())),
        "canFill" => Outcome::Returned(json!(cap.can_fill())),
        "canDesign" => Outcome::Returned(json!(cap.can_design())),
        other => not_a_function(other),
    })
}

// ── the host shims ────────────────────────────────────────────────────────────

fn console_sink(h: &Harness, name: &'static str) -> Rc<dyn Fn(&str, &Error)> {
    let h = h.clone();
    Rc::new(move |message: &str, error: &Error| {
        h.log(name, vec![json!(message), error.to_canonical()]);
    })
}

/// The vector's `form` factory: the plugin with its console sinks logged.
pub fn form_config(h: &Harness) -> FormPluginConfig {
    FormPluginConfig {
        host: FormHost {
            console_warn: Some(console_sink(h, "console.warn")),
            console_error: Some(console_sink(h, "console.error")),
        },
    }
}

// ── the standalone scripting driver ───────────────────────────────────────────

/// The `form-scripting` driver's capability.
#[derive(Clone)]
pub struct StandaloneDriver {
    controller: FormScriptingController,
}

fn scripted(spec: &Value, h: &Harness) -> Callback {
    scripted_callback(spec.get("$fn").and_then(Value::as_str).unwrap_or_default(), spec, h)
}

fn number_fn(callback: Callback) -> Rc<dyn Fn() -> f64> {
    Rc::new(move || match callback.call(Vec::new()) {
        Answer::Returns(value) => value.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    })
}

/// `StandaloneConfig` as [`FormScriptingOptions`].
pub fn standalone_options(config: &Value, h: &Harness) -> Result<FormScriptingOptions, Error> {
    let sandbox = config.get("sandbox").cloned().unwrap_or_else(|| json!({}));
    let mut options = FormScriptingOptions::new(scripted_sandbox_factory(&sandbox, h));
    options.identity = match config.get("identity") {
        None => None,
        Some(identity) if identity.get("$fn").is_some() => {
            let callback = scripted(identity, h);
            Some(IdentityOverrides::Function(Rc::new(move || match callback.call(Vec::new()) {
                Answer::Returns(value) => from_value(&value).unwrap_or_default(),
                _ => PartialScriptIdentity::default(),
            })))
        }
        Some(identity) => Some(IdentityOverrides::Value(from_value(identity)?)),
    };
    options.file_name = config.get("fileName").map(|spec| {
        let callback = scripted(spec, h);
        Rc::new(move || match callback.call(Vec::new()) {
            Answer::Returns(value) => value.as_str().unwrap_or_default().to_string(),
            _ => String::new(),
        }) as Rc<dyn Fn() -> String>
    });
    let number = |key: &str| config.get(key).map(|spec| number_fn(scripted(spec, h)));
    options.now = number("now");
    options.utc_offset_minutes = number("utcOffsetMinutes");
    options.random_seed = number("randomSeed");
    options.budget = match config.get("budget") {
        Some(budget) => Some(from_value(budget)?),
        None => None,
    };
    Ok(options)
}

fn standalone_plugin(config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
    let token: Token<StandaloneDriver> = Token::from_any(h.token("form-scripting", None));
    let options = standalone_options(config, h)?;
    Ok(define::<Value, Value>("form-scripting")
        .document_scoped()
        .capability(&token, move |ctx: Ctx<Value, Value>| {
            let doc = ctx.doc().ok_or_else(|| Error::new("[form-scripting] no document"))?;
            let document_ctx = ctx.clone();
            let controller = FormScriptingController::new(FormScriptingControllerOptions::Config {
                doc,
                document: Rc::new(move || document_ctx.document()),
                config: options.clone(),
            });
            Ok(StandaloneDriver { controller })
        })
        .build())
}

// ── the behavior probe ────────────────────────────────────────────────────────

/// The `form-probe` driver's capability.
#[derive(Clone)]
pub struct Probe {
    ctx: Ctx<Value, Value>,
}

fn probe_plugin(h: &Harness) -> AnyPlugin {
    let token: Token<Probe> = Token::from_any(h.token("form-probe", None));
    define::<Value, Value>("form-probe")
        .document_scoped()
        .requires(ANNOTATION.any())
        .capability(&token, |ctx| Ok(Probe { ctx }))
        .build()
}

impl HarnessPlugin for Form {
    fn factories(&self) -> Vec<&'static str> {
        vec!["form", "form-scripting", "form-probe"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "form-scripting" => standalone_plugin(config, h),
            "form-probe" => Ok(probe_plugin(h)),
            _ => Ok(form_plugin(form_config(h))),
        }
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![FORM.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(driver) = capability.downcast_ref::<StandaloneDriver>() {
            let c = &driver.controller;
            return Some(match method {
                "commit" => json_promise(c.commit(value_arg(&args, 0), value_arg(&args, 1))),
                "activate" => json_promise(c.activate(value_arg(&args, 0), value_arg(&args, 1))),
                "recalculate" => json_promise(c.recalculate()),
                "dispose" => {
                    c.dispose();
                    Outcome::undefined()
                }
                other => not_a_function(other),
            });
        }
        if let Some(probe) = capability.downcast_ref::<Probe>() {
            return Some(match method {
                "behaviorFor" => {
                    let behavior = probe.ctx.get(&ANNOTATION).map(|annotation| {
                        annotation
                            .behavior_for(&str_arg(&args, 0), None)
                            .map_or(Value::Null, |b| json!(b.id()))
                    });
                    Outcome::from_result(behavior)
                }
                other => not_a_function(other),
            });
        }
        let cap = capability.downcast_ref::<FormCapability>()?;
        Some(call_form(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<StandaloneDriver>().is_some() {
            return Some(vec!["commit", "activate", "recalculate", "dispose"]);
        }
        if capability.downcast_ref::<Probe>().is_some() {
            return Some(vec!["behaviorFor"]);
        }
        capability.downcast_ref::<FormCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── the function table ────────────────────────────────────────────────────────

fn model_arg(args: &[Value], i: usize) -> Result<Rc<Model>, Error> {
    decoded(args, i).map(Rc::new)
}

fn field_key_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(field_key_of(&value_arg(args, 0))))
}

fn initial_model_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(json_of(&initial_model()))
}

fn update_fn(args: &[Value]) -> Result<Value, Error> {
    let msg: Msg = decoded(args, 1)?;
    Ok(json_of(&*update(&model_arg(args, 0)?, &msg)))
}

fn field_by_key_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(or_null(field_by_key(&*model_arg(args, 0)?, &str_arg(args, 1))))
}

fn field_for_widget_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(or_null(field_for_widget(&*model_arg(args, 0)?, num_arg(args, 1))))
}

fn project_widget_fn(args: &[Value]) -> Result<Value, Error> {
    let bx = match arg(args, 3) {
        Some(bx) => Some(from_value(bx)?),
        None => None,
    };
    let item = project_widget(&*model_arg(args, 0)?, &value_arg(args, 1), num_arg(args, 2), bx);
    Ok(item.map_or(Value::Null, |i| json_of(&i)))
}

fn fill_items_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json_of(&fill_items(&*model_arg(args, 0)?, num_arg(args, 1))))
}

fn fill_item_for_widget_fn(args: &[Value]) -> Result<Value, Error> {
    let item = fill_item_for_widget(&*model_arg(args, 0)?, num_arg(args, 1));
    Ok(item.map_or(Value::Null, |i| json_of(&i)))
}

fn initial_form_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(json_of(&initial_form_state()))
}

/// `formReducer(state, action)`: an action other than `SET_MODEL` is the
/// TypeScript's `return state`, which the Rust enum cannot carry.
fn form_reducer_fn(args: &[Value]) -> Result<Value, Error> {
    let state = value_arg(args, 0);
    let action = value_arg(args, 1);
    if action.get("type").and_then(Value::as_str) != Some("SET_MODEL") {
        return Ok(state);
    }
    let model: Model = from_value(action.get("model").unwrap_or(&Value::Null))?;
    let current: Model = from_value(state.get("model").unwrap_or(&Value::Null))?;
    let next = form_reducer(
        &FormState { model: Rc::new(current) },
        &FormAction::SetModel { model: Rc::new(model) },
    );
    Ok(json_of(&next.unwrap_or_else(initial_form_state)))
}

fn form_tools_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(json!({ "tools": json_of(&*FORM_TOOLS), "placeTags": PLACE_TAGS }))
}

fn form_tool_by_id_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(form_tool_by_id(&str_arg(args, 0)).map_or(Value::Null, json_of))
}

fn resolve_field_selection_fn(args: &[Value]) -> Result<Value, Error> {
    let all = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
    let targets = arg(args, 1).and_then(Value::as_array).cloned();
    let exclude = crate::value::truthy(arg(args, 2));
    let selection = resolve_field_selection(&all, targets.as_deref(), exclude);
    let listed: Vec<bool> = all.iter().map(|f| selection.listed(f)).collect();
    Ok(json!({ "selected": selection.selected, "listed": listed }))
}

fn build_submit_entries_fn(args: &[Value]) -> Result<Value, Error> {
    let fields = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
    let intent = decoded(args, 1)?;
    let diagnostics = std::cell::RefCell::new(Vec::new());
    let entries = build_submit_entries(&fields, &intent, &|d| diagnostics.borrow_mut().push(d));
    Ok(json!({ "entries": json_of(&entries), "diagnostics": json_of(&diagnostics.into_inner()) }))
}

/// The functions `vectors/plugins/form-core.json` names.
pub fn functions() -> FunctionTable {
    vec![
        ("fieldKeyOf", field_key_of_fn),
        ("initialModel", initial_model_fn),
        ("update", update_fn),
        ("fieldByKey", field_by_key_fn),
        ("fieldForWidget", field_for_widget_fn),
        ("projectWidget", project_widget_fn),
        ("fillItems", fill_items_fn),
        ("fillItemForWidget", fill_item_for_widget_fn),
        ("initialFormState", initial_form_state_fn),
        ("formReducer", form_reducer_fn),
        ("formTools", form_tools_fn),
        ("formToolById", form_tool_by_id_fn),
        ("resolveFieldSelection", resolve_field_selection_fn),
        ("buildSubmitEntries", build_submit_entries_fn),
    ]
}
