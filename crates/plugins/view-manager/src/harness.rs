//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the reducer suite calls.
//!
//! Two factories. `view-manager` is the plugin as this crate builds it; the
//! generator installs the TypeScript's through a wrapper that hides `doc`
//! from the controller's context spread (see the crate README).
//! `view-manager-as-shipped` is the TypeScript plugin unmodified, whose
//! spread reads the workspace `doc` getter and throws while the kernel is
//! constructed. The Rust port has no spread, so this factory reproduces the
//! read: it reads `cx.doc()` before building the controller, which refuses
//! the same way.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::plugin::{Created, PluginDef};
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::model::{ViewManagerAction, ViewManagerState};
use crate::{
    VIEW_MANAGER, ViewManagerCapability, initial_view_manager_state, view_manager_plugin,
    view_manager_reducer,
};

pub struct ViewManager;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "listPanes",
    "getPane",
    "getPaneOrder",
    "getFocusedPaneId",
    "getPaneOfDocument",
    "createPane",
    "removePane",
    "movePane",
    "setFocusedPane",
    "splitPane",
    "setActiveDocument",
    "addDocument",
    "removeDocument",
    "moveDocumentWithin",
    "moveDocumentBetween",
    "onPaneCreated",
    "onPaneRemoved",
    "onFocusChanged",
    "onDocumentMoved",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

/// A canonical number: plain, or `{"$number": ...}`; `None` when absent.
fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value.as_f64().or_else(|| match value.get("$number").and_then(Value::as_str) {
        Some("Infinity") => Some(f64::INFINITY),
        Some("-Infinity") => Some(f64::NEG_INFINITY),
        _ => Some(f64::NAN),
    })
}

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Subscribe a scripted `$fn` listener; the outcome is the unsubscribe.
fn listen<E: serde::Serialize>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![json_of(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

/// The TypeScript plugin as it ships: building its controller reads the
/// workspace `doc` getter, which throws.
fn as_shipped() -> AnyPlugin {
    let plugin = view_manager_plugin();
    let create = plugin.create.clone().expect("a v3 plugin");
    Rc::new(PluginDef {
        id: plugin.id.clone(),
        token: plugin.token.clone(),
        scope: plugin.scope,
        requires: plugin.requires.clone(),
        optional: plugin.optional.clone(),
        initial_state: plugin.initial_state.clone(),
        reducer: plugin.reducer.clone(),
        to_json: plugin.to_json.clone(),
        to_text: plugin.to_text.clone(),
        capability: None,
        init: None,
        effects: None,
        create: Some(Rc::new(move |cx| -> Result<Created, Error> {
            cx.doc()?;
            create(cx)
        })),
    })
}

impl HarnessPlugin for ViewManager {
    fn factories(&self) -> Vec<&'static str> {
        vec!["view-manager", "view-manager-as-shipped"]
    }

    fn build(&self, factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(match factory {
            "view-manager-as-shipped" => as_shipped(),
            _ => view_manager_plugin(),
        })
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![VIEW_MANAGER.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<ViewManagerCapability>()?;
        let text = |i: usize| arg(&args, i).and_then(Value::as_str).unwrap_or_default();
        let returned = |value: Value| Outcome::Returned(value);
        let done = || Outcome::undefined();
        Some(match method {
            "listPanes" => returned(json_of(&cap.list_panes())),
            "getPane" => returned(json_of(&cap.get_pane(text(0)))),
            "getPaneOrder" => returned(json!(cap.get_pane_order())),
            "getFocusedPaneId" => returned(json!(cap.get_focused_pane_id())),
            "getPaneOfDocument" => returned(json!(cap.get_pane_of_document(text(0)))),
            "createPane" => {
                let ids: Vec<String> = arg(&args, 0)
                    .map(scrub)
                    .and_then(|o| o.get("documentIds").and_then(Value::as_array).cloned())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                returned(json!(cap.create_pane(&ids)))
            }
            "removePane" => {
                let to =
                    arg(&args, 1).and_then(|o| o.get("moveDocumentsTo")).and_then(Value::as_str);
                cap.remove_pane(text(0), to);
                done()
            }
            "movePane" => {
                cap.move_pane(text(0), number(arg(&args, 1)).unwrap_or(f64::NAN));
                done()
            }
            "setFocusedPane" => {
                cap.set_focused_pane(arg(&args, 0).and_then(Value::as_str));
                done()
            }
            "splitPane" => returned(json!(cap.split_pane(text(0)))),
            "setActiveDocument" => {
                cap.set_active_document(text(0), arg(&args, 1).and_then(Value::as_str));
                done()
            }
            "addDocument" => {
                cap.add_document(text(0), text(1), number(arg(&args, 2)));
                done()
            }
            "removeDocument" => {
                cap.remove_document(text(0), text(1));
                done()
            }
            "moveDocumentWithin" => {
                cap.move_document_within(
                    text(0),
                    text(1),
                    number(arg(&args, 2)).unwrap_or(f64::NAN),
                );
                done()
            }
            "moveDocumentBetween" => {
                cap.move_document_between(text(0), text(1), text(2), number(arg(&args, 3)));
                done()
            }
            "onPaneCreated" => listen(h, arg(&args, 0), |f| cap.on_pane_created(f)),
            "onPaneRemoved" => listen(h, arg(&args, 0), |f| cap.on_pane_removed(f)),
            "onFocusChanged" => listen(h, arg(&args, 0), |f| cap.on_focus_changed(f)),
            "onDocumentMoved" => listen(h, arg(&args, 0), |f| cap.on_document_moved(f)),
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<ViewManagerCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[view-manager harness] {what}: {e}"))
}

/// The reducer with its reference identity made visible. An action no
/// variant decodes is the TypeScript's `default: return state`.
fn f_reducer(args: &[Value]) -> Result<Value, Error> {
    let state: ViewManagerState =
        serde_json::from_value(scrub(arg(args, 0).unwrap_or(&Value::Null)))
            .map_err(|e| bad("state", e))?;
    let action: Option<ViewManagerAction> =
        arg(args, 1).and_then(|a| serde_json::from_value(scrub(a)).ok());
    let next = action.and_then(|action| view_manager_reducer(&state, &action));
    let shown =
        serde_json::to_value(next.as_ref().unwrap_or(&state)).map_err(|e| bad("state", e))?;
    Ok(json!({ "same": next.is_none(), "next": shown }))
}

fn f_initial(_: &[Value]) -> Result<Value, Error> {
    serde_json::to_value(initial_view_manager_state()).map_err(|e| bad("state", e))
}

pub fn functions() -> FunctionTable {
    vec![("viewManagerReducer", f_reducer), ("initialViewManagerState", f_initial)]
}
