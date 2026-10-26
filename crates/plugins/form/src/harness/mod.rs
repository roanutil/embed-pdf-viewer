//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin and its test drivers from a vector and calls them by name
//! (`tooling/plugin-conformance/src/suites/form-current-fixtures.ts`,
//! `form-live-fixtures.ts` and `form-script-fixtures.ts`):
//!
//! - `form-current` is `formPlugin(config)`;
//! - `form-live-diagnostics` logs the plugin's `console.error` lines that
//!   start `[form]` as the callback `console.error`, and
//!   `form-script-diagnostics` its `console.warn` lines as `console.warn`;
//! - `form-actions-current` is the actions plugin with a scripted sandbox,
//!   as `actions-current` builds it;
//! - `form-scripts-current` drives a [`FormScriptingController`] over a
//!   realm the actions plugin mints for the document, on first use.

pub mod functions;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::java_script_program_from_action_tree;
use epdf_kernel::rt::Promise;
use epdf_kernel::{
    AnyPlugin, AnyToken, Ctx, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_actions::{
    ACTIONS, ActionOrigin, DetachedScriptRealm, PdfAnnotationEventKind, ScriptDocument,
    ScriptRealmTarget,
};
use plugin_conformance::{Answer, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::FORM;
use crate::controller::FormCapability;
use crate::form_plugin;
use crate::scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
};

pub use functions::functions;

/// The adapter.
pub struct Form;

/// The composed api's member order (`composeApi` over the controller's slices).
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "getSnapshot",
    "getStatus",
    "getFormKind",
    "refresh",
    "get",
    "getFieldForWidget",
    "getWidget",
    "getWidgets",
    "list",
    "getValue",
    "getSelectedField",
    "validate",
    "exportValues",
    "listWidgets",
    "listShownWidgets",
    "getAppearanceEpoch",
    "renderAppearances",
    "getWidgetPainting",
    "getTextEditorLayout",
    "ensureLoaded",
    "getWidgetAt",
    "getPageBox",
    "getFieldSettings",
    "setValue",
    "setValues",
    "importValues",
    "draftText",
    "commitDraftText",
    "discardDraftText",
    "reset",
    "resetFormAction",
    "create",
    "addWidget",
    "update",
    "reorderCalculations",
    "delete",
    "removeWidget",
    "updateWidget",
    "deleteWidget",
    "repair",
    "export",
    "import",
    "activateWidget",
    "notifyWidgetEvent",
    "resolveSubmitDataset",
    "commitScriptFormEffects",
    "canRead",
    "canFill",
    "canDesign",
    "canRestoreAttribution",
    "canWriteScripts",
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

fn value(args: &[Value], i: usize) -> Value {
    arg(args, i).cloned().unwrap_or(Value::Null)
}

fn or_null(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

fn number(args: &[Value], i: usize) -> f64 {
    arg(args, i).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// `OperationOptions` with its `signal` taken out: what the engine call takes.
fn without_signal(options: &Value) -> Value {
    let mut out = options.as_object().cloned().unwrap_or_default();
    out.shift_remove("signal");
    Value::Object(out)
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
fn hook(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&Value) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &Value| match callback.call(vec![event.clone()]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    unsubscribe_ref(h, off)
}

/// A typed promise as the JSON a trace records, in the same turn.
fn json_promise<T: serde::Serialize + Clone + 'static>(promise: Promise<T>) -> Outcome {
    Outcome::Promise(promise.convert(|v| Ok(epdf_kernel::js::to_value(v))))
}

fn call_form(
    cap: &FormCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    Ok(match method {
        "getSettings" => Outcome::Returned((*cap.get_settings()).clone()),
        "updateSettings" => {
            cap.update_settings(&value(args, 0));
            Outcome::undefined()
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let cap = cap.clone();
            hook(h, arg(args, 0), move |f| cap.on_settings_changed(f))
        }
        "getSnapshot" => Outcome::Returned(or_null(cap.get_snapshot().map(|s| (*s).clone()))),
        "getStatus" => Outcome::Returned(json!(cap.get_status().as_str())),
        "getFormKind" => Outcome::Returned(cap.get_form_kind()),
        "refresh" => Outcome::promise_unit(cap.refresh(None)),
        "get" => Outcome::Returned(or_null(cap.get(&value(args, 0)))),
        "getFieldForWidget" => {
            Outcome::Returned(or_null(cap.get_field_for_widget(&value(args, 0))))
        }
        "getWidget" => Outcome::Returned(or_null(cap.get_widget(&value(args, 0)))),
        "getWidgets" => Outcome::Returned(Value::Array(cap.get_widgets(&value(args, 0)))),
        "list" => Outcome::Returned(Value::Array(cap.list(arg(args, 0)))),
        "getValue" => Outcome::Returned(or_null(cap.get_value(&value(args, 0)))),
        "getSelectedField" => Outcome::Returned(or_null(cap.get_selected_field())),
        "validate" => Outcome::Returned(cap.validate()),
        "exportValues" => Outcome::Returned(cap.export_values()),
        "listWidgets" => Outcome::Returned(Value::Array(cap.list_widgets(&value(args, 0)))),
        "listShownWidgets" => Outcome::Returned(Value::Array(
            cap.list_shown_widgets(&value(args, 0)).iter().map(|w| w.to_json()).collect(),
        )),
        "getAppearanceEpoch" => Outcome::Returned(json!(cap.get_appearance_epoch(&value(args, 0)))),
        "renderAppearances" => Outcome::Promise(
            cap.render_appearances(&value(args, 0), number(args, 1), None)
                .convert(|list| Ok(epdf_kernel::js::to_value(&**list))),
        ),
        "getWidgetPainting" => Outcome::Returned(
            cap.get_widget_painting(&value(args, 0))
                .map_or(Value::Null, |p| json!({ "scene": p.scene.len() })),
        ),
        "getTextEditorLayout" => Outcome::Returned(
            cap.get_text_editor_layout(&value(args, 0), &crate::value::string_of(arg(args, 1)))
                .map_or(Value::Null, |l| epdf_kernel::js::to_value(&l)),
        ),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(&value(args, 0))),
        "getWidgetAt" => {
            let point = value(args, 1);
            let x = point.get("x").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let y = point.get("y").and_then(Value::as_f64).unwrap_or(f64::NAN);
            Outcome::Returned(
                cap.get_widget_at(&value(args, 0), x, y).map_or(Value::Null, |w| w.to_json()),
            )
        }
        "getPageBox" => Outcome::Returned(cap.get_page_box_json(&value(args, 0))),
        "getFieldSettings" => Outcome::Returned(or_null(cap.get_field_settings(&value(args, 0)))),
        "setValue" => Outcome::Promise(cap.set_value(value(args, 0), value(args, 1), None)),
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
            Outcome::Promise(cap.set_values(entries, None))
        }
        "importValues" => Outcome::Promise(cap.import_values(value(args, 0), None)),
        "draftText" => {
            cap.draft_text(&value(args, 0), &crate::value::string_of(arg(args, 1)));
            Outcome::undefined()
        }
        "commitDraftText" => Outcome::Promise(cap.commit_draft_text(&value(args, 0))),
        "discardDraftText" => {
            cap.discard_draft_text(&value(args, 0));
            Outcome::undefined()
        }
        "reset" => {
            let refs = arg(args, 0).and_then(Value::as_array).cloned();
            Outcome::Promise(cap.reset(refs, None))
        }
        "resetFormAction" => {
            let targets = arg(args, 0).and_then(Value::as_array).cloned();
            let exclude = crate::value::truthy(arg(args, 1));
            let origin: Option<ActionOrigin> = match arg(args, 2) {
                Some(origin) => Some(epdf_kernel::js::from_value(origin)?),
                None => None,
            };
            json_promise(cap.reset_form_action(targets, exclude, origin))
        }
        "create" => Outcome::Promise(cap.create(value(args, 0), None)),
        "addWidget" => {
            let placement = arg(args, 1).cloned().unwrap_or_else(|| json!({}));
            Outcome::Promise(cap.add_widget(value(args, 0), placement, None))
        }
        "update" => Outcome::Promise(cap.update(value(args, 0), value(args, 1), None)),
        "reorderCalculations" => {
            Outcome::Promise(cap.reorder_calculations(value(args, 0), value(args, 1), None))
        }
        "delete" => Outcome::Promise(cap.delete(value(args, 0), None)),
        "removeWidget" => Outcome::Promise(cap.remove_widget(value(args, 0), value(args, 1), None)),
        "updateWidget" => Outcome::Promise(cap.update_widget(value(args, 0), value(args, 1), None)),
        "deleteWidget" => Outcome::Promise(cap.delete_widget(value(args, 0), None)),
        "repair" => Outcome::Promise(cap.repair(arg(args, 0).map(without_signal), None)),
        "export" => Outcome::Promise(cap.export(arg(args, 0).cloned(), None)),
        "import" => {
            let options = arg(args, 1).map(without_signal).unwrap_or_else(|| json!({}));
            Outcome::Promise(cap.import(value(args, 0), options, None))
        }
        "activateWidget" => {
            Outcome::Promise(cap.activate_widget(value(args, 0), None).convert(|r| Ok(r.to_json())))
        }
        "notifyWidgetEvent" => {
            let event: PdfAnnotationEventKind = epdf_kernel::js::from_value(&value(args, 2))?;
            match cap.notify_widget_event(&value(args, 0), &value(args, 1), event) {
                Ok(()) => Outcome::undefined(),
                Err(error) => Outcome::Threw(error),
            }
        }
        "resolveSubmitDataset" => {
            let diagnose = h.callback(arg(args, 2).unwrap_or(&Value::Null));
            json_promise(cap.resolve_submit_dataset(
                epdf_kernel::js::from_value(&value(args, 0))?,
                epdf_kernel::js::from_value(&value(args, 1))?,
                Rc::new(move |d| {
                    let _ = diagnose.call(vec![epdf_kernel::js::to_value(&d)]);
                }),
            ))
        }
        "commitScriptFormEffects" => {
            let effects = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
            json_promise(cap.commit_script_form_effects(effects))
        }
        "canRead" => Outcome::Returned(json!(cap.can_read())),
        "canFill" => Outcome::Returned(json!(cap.can_fill(&value(args, 0)))),
        "canDesign" => Outcome::Returned(json!(cap.can_design())),
        "canRestoreAttribution" => Outcome::Returned(json!(cap.can_restore_attribution())),
        "canWriteScripts" => Outcome::Returned(json!(cap.can_write_scripts())),
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

// ── the console diagnostics ───────────────────────────────────────────────────

thread_local! {
    /// The console levels this case's diagnostics plugins asked to log, and
    /// the harness they log into; `form-current` installs them.
    static DIAGNOSTICS: RefCell<(Option<Harness>, bool, bool)> = const { RefCell::new((None, false, false)) };
}

/// A diagnostics plugin: an empty workspace capability.
#[derive(Clone)]
pub struct Diagnostics;

fn diagnostics_plugin(name: &str, h: &Harness) -> AnyPlugin {
    let token: Token<Diagnostics> = Token::from_any(h.token(name, None));
    define::<Value, Value>(name).capability(&token, |_| Ok(Diagnostics)).build()
}

/// Install the console sink this case asked for, then forget the request.
fn install_console() {
    let (harness, errors, warns) = DIAGNOSTICS.with(|d| std::mem::take(&mut *d.borrow_mut()));
    let Some(log) = harness.filter(|_| errors || warns) else {
        crate::console::set_sink(None);
        return;
    };
    crate::console::set_sink(Some(Rc::new(move |level, args| {
        let first = args.first().and_then(Value::as_str).unwrap_or("");
        if !first.starts_with("[form]") {
            return;
        }
        if level == "error" && errors {
            log.log("console.error", args);
        } else if level == "warn" && warns {
            log.log("console.warn", args);
        }
    })));
}

// ── the standalone scripting driver ───────────────────────────────────────────

struct DriverState {
    realm: Option<DetachedScriptRealm>,
    controller: Option<FormScriptingController>,
}

/// The `form-scripts-current` driver's capability.
#[derive(Clone)]
pub struct StandaloneDriver {
    ctx: Ctx<Value, Value>,
    state: Rc<RefCell<DriverState>>,
}

/// The name-tree boot programs: the catalog's scripts, read lazily.
pub fn name_tree_boot_sources(
    doc: Rc<dyn epdf_kernel::engine::DocumentHandle>,
) -> Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>> {
    Rc::new(move || {
        // `doc.actions` is a required service of the engine contract, so
        // `ctx.doc.actions ? ... : null` always reads it.
        let read = doc.call("actions.get", None, Vec::new());
        Ok(Promise::spawn(async move {
            let catalog = read.await?;
            if catalog.is_null() {
                return Ok(Vec::new());
            }
            crate::value::items(&catalog, "nameTreeScripts")
                .iter()
                .map(|entry| {
                    java_script_program_from_action_tree(
                        entry.get("action").unwrap_or(&Value::Null),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        }))
    })
}

impl StandaloneDriver {
    /// `ready()`: mint the realm and build the controller on first use.
    fn ready(&self) -> Result<FormScriptingController, Error> {
        if let Some(controller) = &self.state.borrow().controller {
            return Ok(controller.clone());
        }
        let doc = self.ctx.doc().ok_or_else(|| Error::new("[form-scripts-current] no document"))?;
        let actions = self.ctx.get(&ACTIONS)?;
        let (meta_ctx, script_ctx) = (self.ctx.clone(), self.ctx.clone());
        let realm = actions
            .create_detached_script_realm(ScriptRealmTarget {
                doc: doc.clone(),
                document: Rc::new(move || meta_ctx.document().map(ScriptDocument::from)),
                boot_sources: name_tree_boot_sources(doc),
            })
            .ok_or_else(|| Error::new("scripting is off"))?;
        let port = realm.clone();
        let transaction: TransactionPort =
            Rc::new(move |body: TransactionBody| port.transaction::<Rc<dyn Any>>(body));
        let controller = FormScriptingController::new(FormScriptingControllerOptions {
            document: Rc::new(move || script_ctx.document()),
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
    let token: Token<StandaloneDriver> = Token::from_any(h.token("form-scripts-current", None));
    define::<Value, Value>("form-scripts-current")
        .document_scoped()
        .requires(ACTIONS.any())
        .capability(&token, move |ctx: Ctx<Value, Value>| {
            let state = Rc::new(RefCell::new(DriverState { realm: None, controller: None }));
            let cleanup = state.clone();
            ctx.cleanup_fn(move || {
                let state = cleanup.borrow();
                if let Some(controller) = &state.controller {
                    controller.dispose();
                }
                if let Some(realm) = &state.realm {
                    realm.dispose();
                }
            });
            Ok(StandaloneDriver { ctx, state })
        })
        .build()
}

fn call_driver(driver: &StandaloneDriver, method: &str, args: &[Value]) -> Outcome {
    let controller = match method {
        "commit" | "activate" | "recalculate" | "dispose" => match driver.ready() {
            Ok(controller) => controller,
            Err(error) => return Outcome::Threw(error),
        },
        other => return not_a_function(other),
    };
    let form = || Rc::new(value(args, 0));
    match method {
        "commit" => json_promise(controller.commit(form(), value(args, 1), value(args, 2))),
        "activate" => json_promise(controller.activate(form(), value(args, 1), value(args, 2))),
        "recalculate" => json_promise(controller.recalculate(form())),
        _ => {
            controller.dispose();
            if let Some(realm) = &driver.state.borrow().realm {
                realm.dispose();
            }
            Outcome::undefined()
        }
    }
}

impl HarnessPlugin for Form {
    fn factories(&self) -> Vec<&'static str> {
        vec![
            "form-current",
            "form-live-diagnostics",
            "form-script-diagnostics",
            "form-actions-current",
            "form-scripts-current",
        ]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "form-live-diagnostics" => {
                DIAGNOSTICS.with(|d| {
                    let mut d = d.borrow_mut();
                    d.0 = Some(h.clone());
                    d.1 = true;
                });
                Ok(diagnostics_plugin("form-live-diagnostics", h))
            }
            "form-script-diagnostics" => {
                DIAGNOSTICS.with(|d| {
                    let mut d = d.borrow_mut();
                    d.0 = Some(h.clone());
                    d.2 = true;
                });
                Ok(diagnostics_plugin("form-script-diagnostics", h))
            }
            "form-actions-current" => Ok(epdf_plugin_actions::actions_plugin(
                epdf_plugin_actions::harness::actions_config(config, h, true)?,
            )),
            "form-scripts-current" => Ok(standalone_plugin(h)),
            _ => {
                install_console();
                Ok(form_plugin(Some(config.clone()).filter(|c| !is_undefined(c))))
            }
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
        if capability.downcast_ref::<Diagnostics>().is_some() {
            return Some(not_a_function(method));
        }
        let cap = capability.downcast_ref::<FormCapability>()?;
        Some(call_form(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<StandaloneDriver>().is_some() {
            return Some(vec!["commit", "activate", "recalculate", "dispose"]);
        }
        if capability.downcast_ref::<Diagnostics>().is_some() {
            return Some(Vec::new());
        }
        capability.downcast_ref::<FormCapability>()?;
        Some(MEMBERS.to_vec())
    }
}
