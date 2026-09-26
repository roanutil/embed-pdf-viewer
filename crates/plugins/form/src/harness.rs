//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin and its two test drivers from a vector and calls them by name,
//! plus the function table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/form-fixtures.ts`:
//!
//! - `console.warn` and `console.error` lines starting `[form]` log as the
//!   callbacks of those names (the `FormHost` sinks);
//! - `form-scripting` builds a [`FormScriptingController`] over a realm the
//!   actions plugin mints with `create_detached_script_realm` for the
//!   document, on first use; the driver owns the realm;
//! - `form-probe` answers the annotation plugin's `getBehaviorFor` as the id
//!   of the engaged behavior.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::java_script_program_from_action_tree;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::rt::Promise;
use epdf_kernel::{
    AnyPlugin, AnyToken, Ctx, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_actions::{
    ACTIONS, ActionOrigin, DetachedScriptRealm, PdfAnnotationEventKind, ScriptRealmTarget,
};
use epdf_plugin_annotation::ANNOTATION;
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::FormCapability;
use crate::core::fill_items::{fill_item_for_widget, fill_items, project_widget};
use crate::core::model::{
    Model, Msg, field_by_key, field_for_widget, field_key_of, initial_model, update, widget_at,
};
use crate::field_selection::{build_submit_entries, resolve_field_selection};
use crate::form_plugin;
use crate::model::{FormAction, FormState, form_reducer, initial_form_state};
use crate::promise::await_engine;
use crate::scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
};
use crate::tools::definitions::{FORM_TOOLS, PLACE_TAGS, form_tool_by_id};
use crate::value::get;

/// The adapter.
pub struct Form;

/// The composed api's member order (`composeApi` over the controller's
/// slices).
const MEMBERS: &[&str] = &[
    "getSnapshot",
    "getField",
    "getFieldForWidget",
    "listFields",
    "getValue",
    "exportValues",
    "listFillItems",
    "getFillItem",
    "ensureLoaded",
    "getWidgetAt",
    "getPageBox",
    "listWidgets",
    "refresh",
    "getStatus",
    "setValue",
    "setText",
    "setChecked",
    "setChoice",
    "setValueRaw",
    "setValues",
    "importValues",
    "reset",
    "resetAll",
    "resetFormAction",
    "createField",
    "updateField",
    "deleteField",
    "detachWidget",
    "attachWidget",
    "exportData",
    "importData",
    "repair",
    "activateWidget",
    "notifyWidgetEvent",
    "resolveSubmitDataset",
    "commitScriptFormEffects",
    "canRead",
    "canFill",
    "canDesign",
    "onValueChanged",
    "onFieldCreated",
    "onFieldUpdated",
    "onFieldDeleted",
    "onValidationRejected",
    "onResynced",
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

fn page_arg(args: &[Value], i: usize) -> Result<PageRef, Error> {
    arg(args, i)
        .and_then(PageRef::from_json)
        .ok_or_else(|| Error::named("TypeError", "not a PageRef"))
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

fn unsubscribe_ref(h: &Harness, un: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        un.call();
        Outcome::undefined()
    })))
}

/// An event hook: subscribe a scripted callback, return its unsubscribe.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(std::boxed::Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(std::boxed::Box::new(move |event: &E| {
        match callback.call(vec![to_value(event)]) {
            Answer::Throws(error) => Err(error),
            _ => Ok(()),
        }
    }));
    unsubscribe_ref(h, off)
}

fn call_form(
    cap: &FormCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    Ok(match method {
        "getSnapshot" => Outcome::Returned(or_null(cap.get_snapshot())),
        "getField" => Outcome::Returned(or_null(cap.get_field(&value_arg(args, 0)))),
        "getFieldForWidget" => {
            Outcome::Returned(or_null(cap.get_field_for_widget(&value_arg(args, 0))))
        }
        "listFields" => {
            let filter: Option<FormFilter> = match arg(args, 0) {
                Some(filter) => Some(from_value(filter)?),
                None => None,
            };
            Outcome::Returned(Value::Array(cap.list_fields(filter.as_ref())))
        }
        "getValue" => Outcome::Returned(or_null(cap.get_value(&value_arg(args, 0)))),
        "exportValues" => Outcome::Returned(Value::Object(cap.export_values())),
        "listFillItems" => Outcome::Returned(json_of(&*cap.list_fill_items(page_arg(args, 0)?))),
        "listWidgets" => Outcome::Returned(json_of(&*cap.list_widgets(page_arg(args, 0)?))),
        "getFillItem" => Outcome::Returned(
            cap.get_fill_item(num_arg(args, 0)).map_or(Value::Null, |i| json_of(&*i)),
        ),
        "ensureLoaded" => {
            cap.ensure_loaded(page_arg(args, 0)?);
            Outcome::undefined()
        }
        "getWidgetAt" => {
            let point = value_arg(args, 1);
            let x = point.get("x").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let y = point.get("y").and_then(Value::as_f64).unwrap_or(f64::NAN);
            Outcome::Returned(
                cap.get_widget_at(page_arg(args, 0)?, x, y).map_or(Value::Null, |w| json_of(&w)),
            )
        }
        "getPageBox" => Outcome::Returned(
            cap.get_page_box(page_arg(args, 0)?).map_or(Value::Null, |b| json_of(&b)),
        ),
        "refresh" => unit_promise(cap.refresh()),
        "getStatus" => Outcome::Returned(json_of(&cap.get_status())),
        "setValue" => json_promise(cap.set_value(value_arg(args, 0), value_arg(args, 1))),
        "setText" => json_promise(cap.set_text(value_arg(args, 0), value_arg(args, 1))),
        "setChecked" => json_promise(cap.set_checked(value_arg(args, 0), value_arg(args, 1))),
        "setChoice" => json_promise(cap.set_choice(
            value_arg(args, 0),
            arg(args, 1).and_then(Value::as_array).cloned().unwrap_or_default(),
        )),
        "setValueRaw" => json_promise(cap.set_value_raw(value_arg(args, 0), value_arg(args, 1))),
        "setValues" => {
            let entries = arg(args, 0)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|e| {
                    (
                        e.get("ref").cloned().unwrap_or_else(undefined),
                        e.get("value").cloned().unwrap_or_else(undefined),
                    )
                })
                .collect();
            json_promise(cap.set_values(entries))
        }
        "importValues" => {
            let values = arg(args, 0).and_then(Value::as_object).cloned().unwrap_or_default();
            json_promise(cap.import_values(values))
        }
        "reset" => unit_promise(cap.reset(value_arg(args, 0))),
        "resetAll" => {
            let options: ResetAllOptions = match arg(args, 0) {
                Some(options) => from_value(options)?,
                None => ResetAllOptions::default(),
            };
            json_promise(cap.reset_all(options))
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
        "createField" => json_promise(cap.create_field(decoded(args, 0)?)),
        "updateField" => unit_promise(cap.update_field(value_arg(args, 0), value_arg(args, 1))),
        "deleteField" => unit_promise(cap.delete_field(value_arg(args, 0))),
        "detachWidget" => unit_promise(cap.detach_widget(value_arg(args, 0), value_arg(args, 1))),
        "attachWidget" => unit_promise(cap.attach_widget(value_arg(args, 0), value_arg(args, 1))),
        "exportData" => json_promise(cap.export_data(arg(args, 0).cloned())),
        "importData" => json_promise(cap.import_data(value_arg(args, 0), arg(args, 1).cloned())),
        "repair" => json_promise(cap.repair(arg(args, 0).cloned())),
        "activateWidget" => json_promise(cap.activate_widget(value_arg(args, 0))),
        "notifyWidgetEvent" => {
            let event: PdfAnnotationEventKind = decoded(args, 2)?;
            match cap.notify_widget_event(&value_arg(args, 0), &value_arg(args, 1), event) {
                Ok(()) => Outcome::undefined(),
                Err(error) => Outcome::Threw(error),
            }
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
        "canRead" => Outcome::Returned(json!(cap.can_read())),
        "canFill" => Outcome::Returned(json!(cap.can_fill())),
        "canDesign" => Outcome::Returned(json!(cap.can_design())),
        "onValueChanged" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_value_changed(f))
        }
        "onFieldCreated" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_field_created(f))
        }
        "onFieldUpdated" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_field_updated(f))
        }
        "onFieldDeleted" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_field_deleted(f))
        }
        "onValidationRejected" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_validation_rejected(f))
        }
        "onResynced" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_resynced(f))
        }
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
pub fn form_config(h: &Harness) -> FormConfig {
    FormConfig {
        validation: None,
        host: FormHost {
            console_warn: Some(console_sink(h, "console.warn")),
            console_error: Some(console_sink(h, "console.error")),
        },
    }
}

// ── the standalone scripting driver ───────────────────────────────────────────

/// The name-tree boot programs: the catalog's scripts, read lazily.
pub fn name_tree_boot_sources(
    doc: Rc<dyn epdf_kernel::engine::DocumentHandle>,
) -> Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>> {
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
}

struct DriverState {
    realm: Option<DetachedScriptRealm>,
    controller: Option<FormScriptingController>,
}

/// The `form-scripting` driver's capability.
#[derive(Clone)]
pub struct StandaloneDriver {
    ctx: Ctx<Value, Value>,
    state: Rc<RefCell<DriverState>>,
}

impl StandaloneDriver {
    /// `ready()`: mint the realm and build the controller on first use.
    fn ready(&self) -> Result<FormScriptingController, Error> {
        if let Some(controller) = &self.state.borrow().controller {
            return Ok(controller.clone());
        }
        let doc = self.ctx.doc().ok_or_else(|| Error::new("[form-scripting] no document"))?;
        let actions = self.ctx.get(&ACTIONS)?;
        let document_ctx = self.ctx.clone();
        let document: Rc<dyn Fn() -> Option<epdf_kernel::types::DocumentMeta>> =
            Rc::new(move || document_ctx.document());
        let realm = actions
            .create_detached_script_realm(ScriptRealmTarget {
                doc: doc.clone(),
                document: document.clone(),
                boot_sources: name_tree_boot_sources(doc.clone()),
            })
            .ok_or_else(|| Error::new("[form-scripting] scripting is off"))?;
        let port = realm.clone();
        let transaction: TransactionPort =
            Rc::new(move |body: TransactionBody| port.transaction(body));
        let controller = FormScriptingController::new(FormScriptingControllerOptions {
            doc,
            document,
            transaction,
            budget: Some(realm.budget().clone()),
        });
        let mut state = self.state.borrow_mut();
        state.realm = Some(realm);
        state.controller = Some(controller.clone());
        Ok(controller)
    }
}

fn standalone_plugin(h: &Harness) -> AnyPlugin {
    let token: Token<StandaloneDriver> = Token::from_any(h.token("form-scripting", None));
    define::<Value, Value>("form-scripting")
        .document_scoped()
        .requires(ACTIONS.any())
        .capability(&token, move |ctx: Ctx<Value, Value>| {
            Ok(StandaloneDriver {
                ctx,
                state: Rc::new(RefCell::new(DriverState { realm: None, controller: None })),
            })
        })
        .build()
}

fn call_driver(driver: &StandaloneDriver, method: &str, args: &[Value]) -> Outcome {
    let run = |f: &dyn Fn(&FormScriptingController) -> Outcome| match driver.ready() {
        Ok(controller) => f(&controller),
        Err(error) => Outcome::Threw(error),
    };
    match method {
        "commit" => run(&|c| json_promise(c.commit(value_arg(args, 0), value_arg(args, 1)))),
        "activate" => run(&|c| json_promise(c.activate(value_arg(args, 0), value_arg(args, 1)))),
        "recalculate" => run(&|c| json_promise(c.recalculate())),
        "dispose" => run(&|c| {
            c.dispose();
            if let Some(realm) = &driver.state.borrow().realm {
                realm.dispose();
            }
            Outcome::undefined()
        }),
        other => not_a_function(other),
    }
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

    fn build(&self, factory: &str, _config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "form-scripting" => Ok(standalone_plugin(h)),
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
            return Some(call_driver(driver, method, &args));
        }
        if let Some(probe) = capability.downcast_ref::<Probe>() {
            return Some(match method {
                "behaviorFor" => {
                    let behavior = probe.ctx.get(&ANNOTATION).map(|annotation| {
                        annotation
                            .get_behavior_for(&str_arg(&args, 0), None)
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

fn widget_at_fn(args: &[Value]) -> Result<Value, Error> {
    let point = value_arg(args, 2);
    let x = point.get("x").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let y = point.get("y").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let hit = widget_at(&*model_arg(args, 0)?, num_arg(args, 1), x, y);
    Ok(hit.map_or(Value::Null, |h| json_of(&h)))
}

fn field_ref_by_name_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(field_ref::by_name(&str_arg(args, 0)))
}

fn field_ref_by_object_number_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(field_ref::by_object_number(num_arg(args, 0)))
}

fn initial_form_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(json_of(&initial_form_state()))
}

/// `formReducer(state, action)`: an action of another type is the
/// TypeScript's `return state`, which the Rust enum cannot carry.
fn form_reducer_fn(args: &[Value]) -> Result<Value, Error> {
    let state = value_arg(args, 0);
    let action = value_arg(args, 1);
    let current = FormState {
        model: Rc::new(from_value(state.get("model").unwrap_or(&Value::Null))?),
        status: from_value(state.get("status").unwrap_or(&Value::Null))?,
    };
    let action = match action.get("type").and_then(Value::as_str) {
        Some("SET_MODEL") => FormAction::SetModel {
            model: Rc::new(from_value(action.get("model").unwrap_or(&Value::Null))?),
        },
        Some("SET_STATUS") => FormAction::SetStatus {
            status: from_value(action.get("status").unwrap_or(&Value::Null))?,
        },
        _ => return Ok(state),
    };
    Ok(form_reducer(&current, &action).map_or(state, |next| json_of(&next)))
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
    let diagnostics = RefCell::new(Vec::new());
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
        ("widgetAt", widget_at_fn),
        ("fieldRefByName", field_ref_by_name_fn),
        ("fieldRefByObjectNumber", field_ref_by_object_number_fn),
        ("initialFormState", initial_form_state_fn),
        ("formReducer", form_reducer_fn),
        ("formTools", form_tools_fn),
        ("formToolById", form_tool_by_id_fn),
        ("resolveFieldSelection", resolve_field_selection_fn),
        ("buildSubmitEntries", build_submit_entries_fn),
    ]
}
