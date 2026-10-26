//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the table of
//! named model transitions the reducer suite calls.
//!
//! Two factories, `view-manager` and `view-manager-as-shipped`, both the
//! plugin as it ships: the generator registers the same `viewManagerPlugin()`
//! under each name.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::model::{self, ViewManagerState};
use crate::{VIEW_MANAGER, ViewManagerCapability, view_manager_plugin};

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

/// A canonical number: plain, or `{"$number": ...}`; `None` when absent
/// or `null` (`index == null`).
fn number(value: Option<&Value>) -> Option<f64> {
    let value = value.filter(|v| !v.is_null())?;
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

impl HarnessPlugin for ViewManager {
    fn factories(&self) -> Vec<&'static str> {
        vec!["view-manager", "view-manager-as-shipped"]
    }

    /// `view-manager` and `view-manager-as-shipped` build the same plugin.
    fn build(&self, _factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(view_manager_plugin())
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
            "listPanes" => returned(json_of(&*cap.list_panes())),
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
            "splitPane" => {
                let from = arg(&args, 1).and_then(|o| o.get("from")).and_then(Value::as_str);
                returned(json!(cap.split_pane(text(0), from)))
            }
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

// ── the transition table ─────────────────────────────────────────────────────

fn state(args: &[Value]) -> Result<Rc<ViewManagerState>, Error> {
    serde_json::from_value(scrub(arg(args, 0).unwrap_or(&Value::Null)))
        .map(Rc::new)
        .map_err(|e| bad("state", e))
}

fn text(args: &[Value], i: usize) -> &str {
    arg(args, i).and_then(Value::as_str).unwrap_or_default()
}

fn shown(state: &ViewManagerState) -> Result<Value, Error> {
    serde_json::to_value(state).map_err(|e| bad("state", e))
}

fn f_initial(_: &[Value]) -> Result<Value, Error> {
    shown(&model::initial_view_manager_state())
}

fn f_next_pane_id(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(model::next_pane_id(&*state(args)?)))
}

fn f_pane_of_document(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(model::pane_of_document(&*state(args)?, text(args, 1))))
}

fn f_create_pane(args: &[Value]) -> Result<Value, Error> {
    shown(&model::create_pane(&state(args)?, number(arg(args, 1))))
}

fn f_remove_pane(args: &[Value]) -> Result<Value, Error> {
    shown(&model::remove_pane(&state(args)?, text(args, 1)))
}

fn f_move_pane(args: &[Value]) -> Result<Value, Error> {
    let to_index = number(arg(args, 2)).unwrap_or(f64::NAN);
    shown(&model::move_pane(&state(args)?, text(args, 1), to_index))
}

fn f_set_focused_pane(args: &[Value]) -> Result<Value, Error> {
    shown(&model::set_focused_pane(&state(args)?, arg(args, 1).and_then(Value::as_str)))
}

fn f_set_active_document(args: &[Value]) -> Result<Value, Error> {
    let document_id = arg(args, 2).and_then(Value::as_str);
    shown(&model::set_active_document(&state(args)?, text(args, 1), document_id))
}

fn f_add_document(args: &[Value]) -> Result<Value, Error> {
    let index = number(arg(args, 3));
    shown(&model::add_document(&state(args)?, text(args, 1), text(args, 2), index))
}

fn f_remove_document(args: &[Value]) -> Result<Value, Error> {
    shown(&model::remove_document(&state(args)?, text(args, 1), text(args, 2)))
}

fn f_move_document_within(args: &[Value]) -> Result<Value, Error> {
    let to_index = number(arg(args, 3)).unwrap_or(f64::NAN);
    shown(&model::move_document_within(&state(args)?, text(args, 1), text(args, 2), to_index))
}

fn f_move_document_between(args: &[Value]) -> Result<Value, Error> {
    let (from, to, document_id) = (text(args, 1), text(args, 2), text(args, 3));
    let to_index = number(arg(args, 4));
    shown(&model::move_document_between(&state(args)?, from, to, document_id, to_index))
}

fn f_rename_document(args: &[Value]) -> Result<Value, Error> {
    shown(&model::rename_document(&state(args)?, text(args, 1), text(args, 2)))
}

fn f_reconcile(args: &[Value]) -> Result<Value, Error> {
    let open: Vec<String> = arg(args, 1)
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let prefer = arg(args, 2).and_then(Value::as_str);
    shown(&model::reconcile(&state(args)?, &open, prefer))
}

pub fn functions() -> FunctionTable {
    vec![
        ("initialViewManagerState", f_initial),
        ("nextPaneId", f_next_pane_id),
        ("paneOfDocument", f_pane_of_document),
        ("createPane", f_create_pane),
        ("removePane", f_remove_pane),
        ("movePane", f_move_pane),
        ("setFocusedPane", f_set_focused_pane),
        ("setActiveDocument", f_set_active_document),
        ("addDocument", f_add_document),
        ("removeDocument", f_remove_document),
        ("moveDocumentWithin", f_move_document_within),
        ("moveDocumentBetween", f_move_document_between),
        ("renameDocument", f_rename_document),
        ("reconcile", f_reconcile),
    ]
}
