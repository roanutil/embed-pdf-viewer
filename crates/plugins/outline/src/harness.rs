//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.
//!
//! It mirrors the shim in `tooling/plugin-conformance/src/suites/outline.ts`:
//! an option `signal: {"$abortSignal": name}` is the signal of a controller
//! named `name`, which the `outline-signals` test plugin's `abort(name)`
//! aborts with the platform's `AbortError`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use plugin_conformance::js::to_value;
use plugin_conformance::{Answer, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::model::truthy;
use crate::*;

/// The capability object literal's member order (`controller.ts`).
pub const MEMBERS: &[&str] = &[
    "list",
    "get",
    "getChildren",
    "listRows",
    "getCurrentItem",
    "isIncomplete",
    "getStatus",
    "ensureLoaded",
    "refresh",
    "isExpanded",
    "expand",
    "collapse",
    "toggle",
    "expandAll",
    "collapseAll",
    "reveal",
    "getSelected",
    "select",
    "activate",
    "canEdit",
    "getCurrentViewTarget",
    "create",
    "update",
    "move",
    "delete",
    "onChanged",
    "onResynced",
];

thread_local! {
    /// The case's abort controllers by name; an outline build starts a case.
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// The `outline-signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> = LazyLock::new(|| Token::new("outline-test/signals"));

/// The adapter.
pub struct Outline;

fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(undefined)
}

/// The signal an options argument names, `{"$abortSignal": name}`.
fn signal_of(options: &Value) -> Option<AbortSignal> {
    options
        .get("signal")
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named)
}

fn options(args: &[Value], index: usize) -> OperationOptions {
    OperationOptions { signal: signal_of(&arg(args, index)) }
}

fn refs(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn or_null(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

/// The activation as the TypeScript returns it, its dispatch promise
/// tracked.
fn activation_json(activation: &LinkActivation, h: &Harness) -> Value {
    activation.to_json_with(|dispatch| h.track(dispatch.map(|result| Ok(to_value(&result)))))
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn call_capability(cap: &OutlineCapability, method: &str, args: &[Value], h: &Harness) -> Outcome {
    match method {
        "list" => Outcome::Returned(Value::Array((*cap.list()).clone())),
        "get" => Outcome::Returned(or_null(cap.get(&arg(args, 0)))),
        "getChildren" => {
            let parent = arg(args, 0);
            let parent = (!is_undefined(&parent)).then_some(&parent);
            Outcome::Returned(Value::Array((*cap.get_children(parent)).clone()))
        }
        "listRows" => Outcome::Returned(to_value(&*cap.list_rows())),
        "getCurrentItem" => Outcome::Returned(or_null(cap.get_current_item())),
        "isIncomplete" => Outcome::Returned(json!(cap.is_incomplete())),
        "getStatus" => Outcome::Returned(json!(cap.get_status().as_str())),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(options(args, 0))),
        "refresh" => Outcome::promise_unit(cap.refresh(options(args, 0))),
        "isExpanded" => Outcome::Returned(json!(cap.is_expanded(&arg(args, 0)))),
        "expand" => {
            cap.expand(&arg(args, 0));
            Outcome::undefined()
        }
        "collapse" => {
            cap.collapse(&arg(args, 0));
            Outcome::undefined()
        }
        "toggle" => {
            cap.toggle(&arg(args, 0));
            Outcome::undefined()
        }
        "expandAll" => {
            cap.expand_all();
            Outcome::undefined()
        }
        "collapseAll" => {
            cap.collapse_all();
            Outcome::undefined()
        }
        "reveal" => {
            cap.reveal(&arg(args, 0));
            Outcome::undefined()
        }
        "getSelected" => Outcome::Returned(Value::Array((*cap.get_selected()).clone())),
        "select" => {
            cap.select(&refs(&arg(args, 0)));
            Outcome::undefined()
        }
        "activate" => match cap.activate(&arg(args, 0)) {
            Ok(activation) => Outcome::Returned(activation_json(&activation, h)),
            Err(error) => Outcome::Threw(error),
        },
        "canEdit" => Outcome::Returned(json!(cap.can_edit())),
        "getCurrentViewTarget" => Outcome::Returned(or_null(cap.get_current_view_target())),
        "create" => {
            let input = arg(args, 0);
            let input = if is_undefined(&input) { json!({}) } else { input };
            let raw = arg(args, 1);
            let options =
                OutlineCreateOptions { signal: signal_of(&raw), select: truthy(raw.get("select")) };
            Outcome::Promise(cap.create(input, options))
        }
        "update" => Outcome::Promise(cap.update(arg(args, 0), arg(args, 1), options(args, 2))),
        "move" => Outcome::promise_unit(cap.move_items(
            refs(&arg(args, 0)),
            arg(args, 1),
            arg(args, 2),
            options(args, 3),
        )),
        "delete" => Outcome::promise_unit(cap.delete(refs(&arg(args, 0)), options(args, 1))),
        "onChanged" => hook(h, args.first(), |l| cap.on_changed(l)),
        "onResynced" => hook(h, args.first(), |l| cap.on_resynced(l)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    }
}

impl HarnessPlugin for Outline {
    fn factories(&self) -> Vec<&'static str> {
        vec!["outline", "outline-signals"]
    }

    fn build(&self, factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "outline-signals" {
            return Ok(define::<Value, Value>("outline-test/signals")
                .capability(&SIGNALS, |_| Ok(Signals))
                .build());
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        Ok(outline_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![OUTLINE.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<OutlineCapability>() {
            return Some(call_capability(cap, method, &args, h));
        }
        capability.downcast_ref::<Signals>()?;
        Some(match method {
            "abort" => {
                let name = args.first().and_then(Value::as_str).unwrap_or_default();
                controller_named(name)
                    .abort(Error::named("AbortError", "This operation was aborted"));
                Outcome::undefined()
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<OutlineCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<Signals>()?;
        Some(vec!["abort"])
    }
}
