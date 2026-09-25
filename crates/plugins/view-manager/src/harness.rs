//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the reducer suite calls.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::types::{ViewManagerAction, ViewManagerState};
use crate::{
    VIEW_MANAGER, ViewManagerCapability, initial_view_manager_state, view_manager_plugin,
    view_manager_reducer,
};

pub struct ViewManager;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "list",
    "order",
    "get",
    "focusedViewId",
    "documentView",
    "createView",
    "removeView",
    "moveView",
    "setFocused",
    "setActiveDocument",
    "addDocument",
    "removeDocument",
    "moveDocumentWithin",
    "moveDocumentBetween",
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

impl HarnessPlugin for ViewManager {
    fn factories(&self) -> Vec<&'static str> {
        vec!["view-manager"]
    }

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
        _h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<ViewManagerCapability>()?;
        let text = |i: usize| arg(&args, i).and_then(Value::as_str).unwrap_or_default();
        let returned = |value: Value| Outcome::Returned(value);
        Some(match method {
            "list" => returned(json!(cap.list())),
            "order" => returned(json!(cap.order())),
            "get" => returned(json!(cap.get(text(0)))),
            "focusedViewId" => returned(json!(cap.focused_view_id())),
            "documentView" => returned(json!(cap.document_view(text(0)))),
            "createView" => returned(json!(cap.create_view())),
            "removeView"
            | "moveView"
            | "setFocused"
            | "setActiveDocument"
            | "addDocument"
            | "removeDocument"
            | "moveDocumentWithin"
            | "moveDocumentBetween" => {
                match method {
                    "removeView" => cap.remove_view(text(0)),
                    "moveView" => cap.move_view(text(0), number(arg(&args, 1)).unwrap_or(f64::NAN)),
                    "setFocused" => cap.set_focused(text(0)),
                    "setActiveDocument" => {
                        cap.set_active_document(text(0), arg(&args, 1).and_then(Value::as_str))
                    }
                    "addDocument" => cap.add_document(text(0), text(1), number(arg(&args, 2))),
                    "removeDocument" => cap.remove_document(text(0), text(1)),
                    "moveDocumentWithin" => cap.move_document_within(
                        text(0),
                        text(1),
                        number(arg(&args, 2)).unwrap_or(f64::NAN),
                    ),
                    _ => {
                        cap.move_document_between(text(0), text(1), text(2), number(arg(&args, 3)))
                    }
                }
                Outcome::undefined()
            }
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
