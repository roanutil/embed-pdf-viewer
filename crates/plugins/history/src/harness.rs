//! The conformance adapter: how `crates/plugin-conformance` builds the
//! history from a vector and calls its capability by name, plus the
//! suite's `history-actor`, the sibling plugin that stages changes on the
//! document's queue (`tooling/plugin-conformance/src/suites/history.ts`).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::changes::{ChangeQueue, HeldChange, StagedChange};
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, define, is_undefined, undefined};
use plugin_conformance::{Answer, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::contract::*;
use crate::{HistoryCapability, history_plugin_json};

/// The adapter for `plugins/history.json`.
pub struct History;

/// The TypeScript object literal's member order: the settings calls spread
/// first, then the history's own members.
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "undo",
    "redo",
    "canUndo",
    "canRedo",
    "getUndoLabel",
    "getRedoLabel",
    "clear",
    "onUndone",
    "onUndoFailed",
];

const ACTOR_MEMBERS: &[&str] =
    &["stage", "stageUnrecorded", "stageUndoOf", "hold", "set", "send", "pending"];

/// The suite's `history-actor`: a feature plugin staging on `ctx.changes`.
#[derive(Clone)]
pub struct Actor {
    changes: ChangeQueue,
    holds: Rc<RefCell<Vec<HeldChange>>>,
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn num_arg(args: &[Value], i: usize) -> f64 {
    arg(args, i).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// The suite's `move(objectNumber, x)`.
fn move_op(object_number: f64, x: f64) -> Value {
    json!({
        "type": "annotations.update",
        "ref": {
            "kind": "objectNumber",
            "page": { "kind": "objectNumber", "objectNumber": 1 },
            "objectNumber": object_number,
        },
        "patch": { "subtype": "square", "box": { "x": x, "y": 0, "width": 10, "height": 10 } },
    })
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn returned<T>(result: Result<T, Error>, to_json: impl FnOnce(T) -> Value) -> Outcome {
    match result {
        Ok(value) => Outcome::Returned(to_json(value)),
        Err(error) => Outcome::Threw(error),
    }
}

fn call_actor(actor: &Actor, method: &str, args: &[Value]) -> Outcome {
    let label = || json!({ "key": str_arg(args, 0) });
    match method {
        "stage" => {
            let (n, x) = (num_arg(args, 1), num_arg(args, 2));
            let mut change = StagedChange::ops(label(), vec![move_op(n, x)]);
            change.undo = vec![move_op(n, 0.0)];
            returned(actor.changes.stage(change), |c| json!(c.op_id()))
        }
        "stageUnrecorded" => {
            let mut change = StagedChange::ops(label(), vec![move_op(10.0, 1.0)]);
            change.history = false;
            returned(actor.changes.stage(change), |c| json!(c.op_id()))
        }
        "stageUndoOf" => returned(
            actor.changes.stage(StagedChange::undo(label(), str_arg(args, 1), Vec::new())),
            |c| json!(c.op_id()),
        ),
        "hold" => {
            let merge = arg(args, 1).and_then(Value::as_str).map(str::to_string);
            returned(actor.changes.hold(label(), merge), |held| {
                let mut holds = actor.holds.borrow_mut();
                holds.push(held);
                json!(holds.len() - 1)
            })
        }
        "set" => {
            let held = actor.holds.borrow()[num_arg(args, 0) as usize].clone();
            let x = num_arg(args, 1);
            held.set(vec![move_op(10.0, x)], vec![move_op(10.0, x - 1.0)]);
            Outcome::undefined()
        }
        "send" => {
            let held = actor.holds.borrow()[num_arg(args, 0) as usize].clone();
            Outcome::Returned(held.send().map_or(Value::Null, |c| json!(c.op_id())))
        }
        "pending" => Outcome::Returned(Value::Array(
            actor
                .changes
                .pending()
                .iter()
                .map(|c| {
                    json!({
                        "opId": c.op_id(),
                        "label": c.label(),
                        "undoOf": c.undo_of(),
                        "merge": c.merge(),
                        "shows": c.shows(),
                        "held": c.held(),
                        "history": c.history(),
                    })
                })
                .collect(),
        )),
        other => not_a_function(other),
    }
}

/// Subscribe a scripted callback to an event; the unsubscribe is a ref.
fn listen<T: 'static>(
    h: &Harness,
    spec: Option<&Value>,
    to_json: fn(&T) -> Value,
    on: impl FnOnce(Box<dyn Fn(&T) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = on(Box::new(move |event| match callback.call(vec![to_json(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

/// A `HistoryUndoneEvent` as the TypeScript prints it.
pub fn undone_json(event: &HistoryUndoneEvent) -> Value {
    serde_json::to_value(event).unwrap()
}

/// A `HistoryUndoFailedEvent` as the TypeScript prints it.
pub fn undo_failed_json(event: &HistoryUndoFailedEvent) -> Value {
    json!({
        "label": event.label,
        "redo": event.redo,
        "reason": event.reason,
        "error": event.error.to_canonical(),
    })
}

fn unit(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

fn call_history(cap: &HistoryCapability, method: &str, args: &[Value], h: &Harness) -> Outcome {
    match method {
        "getSettings" => Outcome::Returned((*cap.settings_json()).clone()),
        "updateSettings" => {
            cap.update_settings_json(arg(args, 0).unwrap_or(&undefined()));
            Outcome::undefined()
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let callback = h.callback(arg(args, 0).unwrap_or(&Value::Null));
            let off = cap.on_settings_changed_json(move |settings, changed| {
                match callback.call(vec![json!({ "settings": settings, "changed": changed })]) {
                    Answer::Throws(error) => Err(error),
                    _ => Ok(()),
                }
            });
            Outcome::Returned(h.register_fn(Rc::new(move |_| {
                off.call();
                Outcome::undefined()
            })))
        }
        "undo" => unit(cap.undo()),
        "redo" => unit(cap.redo()),
        "canUndo" => Outcome::Returned(json!(cap.can_undo())),
        "canRedo" => Outcome::Returned(json!(cap.can_redo())),
        "getUndoLabel" => Outcome::Returned(cap.get_undo_label().unwrap_or(Value::Null)),
        "getRedoLabel" => Outcome::Returned(cap.get_redo_label().unwrap_or(Value::Null)),
        "clear" => {
            cap.clear();
            Outcome::undefined()
        }
        "onUndone" => listen(h, arg(args, 0), undone_json, |f| cap.on_undone(f)),
        "onUndoFailed" => listen(h, arg(args, 0), undo_failed_json, |f| cap.on_undo_failed(f)),
        other => not_a_function(other),
    }
}

fn actor_plugin(token: AnyToken) -> AnyPlugin {
    define::<Value, Value>("history-actor")
        .document_scoped()
        .initial_state(undefined)
        .create(&Token::<Actor>::from_any(token), |cx| {
            Ok((Actor { changes: cx.changes()?, holds: Rc::default() }, None))
        })
        .build()
}

impl HarnessPlugin for History {
    fn factories(&self) -> Vec<&'static str> {
        vec!["history", "history-actor"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "history-actor" => Ok(actor_plugin(h.token("history-actor", None))),
            _ => Ok(history_plugin_json(
                (!config.is_null() && !is_undefined(config)).then(|| config.clone()),
            )),
        }
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![HISTORY.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<HistoryCapability>() {
            return Some(call_history(cap, method, &args, h));
        }
        let actor = capability.downcast_ref::<Actor>()?;
        Some(call_actor(actor, method, &args))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<HistoryCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<Actor>()?;
        Some(ACTOR_MEMBERS.to_vec())
    }
}
