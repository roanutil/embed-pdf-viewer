//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.
//!
//! It mirrors the shim in `tooling/plugin-conformance/src/suites/attachment.ts`:
//! an option `signal: {"$abortSignal": name}` is the signal of a controller
//! named `name`, which the `attachment-signals` test plugin's
//! `abort(name)` aborts with the platform's `AbortError`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, undefined};
use plugin_conformance::js::to_value;
use plugin_conformance::{Answer, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::*;

/// The capability object literal's member order (`controller.ts`).
pub const MEMBERS: &[&str] = &[
    "list",
    "get",
    "getSelected",
    "select",
    "isPending",
    "getStatus",
    "ensureLoaded",
    "refresh",
    "canDownload",
    "canAdd",
    "canRemove",
    "add",
    "remove",
    "download",
    "reveal",
    "onChanged",
    "onResynced",
];

thread_local! {
    /// The case's abort controllers by name; an attachment build starts a
    /// case.
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// The `attachment-signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> =
    LazyLock::new(|| Token::new("attachment-test/signals"));

/// The adapter.
pub struct Attachment;

fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(undefined)
}

/// The signal an options argument names, `{"$abortSignal": name}`.
fn options(args: &[Value], index: usize) -> OperationOptions {
    let signal = arg(args, index)
        .get("signal")
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named);
    OperationOptions { signal }
}

fn list(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
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

fn call_capability(
    cap: &AttachmentCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Outcome {
    match method {
        "list" => Outcome::Returned(Value::Array((*cap.list()).clone())),
        "get" => Outcome::Returned(match arg(args, 0).as_str() {
            Some(id) => cap.get(id).unwrap_or(Value::Null),
            None => Value::Null,
        }),
        "getSelected" => Outcome::Returned(Value::Array((*cap.get_selected()).clone())),
        "select" => {
            cap.select(&list(&arg(args, 0)));
            Outcome::undefined()
        }
        "isPending" => Outcome::Returned(json!(cap.is_pending(&arg(args, 0)))),
        "getStatus" => Outcome::Returned(json!(cap.get_status().as_str())),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(options(args, 0))),
        "refresh" => Outcome::promise_unit(cap.refresh(options(args, 0))),
        "canDownload" => Outcome::Returned(json!(cap.can_download())),
        "canAdd" => Outcome::Returned(json!(cap.can_add())),
        "canRemove" => Outcome::Returned(json!(cap.can_remove(&arg(args, 0)))),
        "add" => Outcome::Promise(
            cap.add(list(&arg(args, 0)), options(args, 1)).convert(|files| Ok(json!(files))),
        ),
        "remove" => Outcome::promise_unit(cap.remove(arg(args, 0), options(args, 1))),
        "download" => Outcome::Promise(
            cap.download(arg(args, 0), options(args, 1)).convert(|content| Ok(to_value(content))),
        ),
        "reveal" => {
            cap.reveal(&arg(args, 0));
            Outcome::undefined()
        }
        "onChanged" => hook(h, args.first(), |l| cap.on_changed(l)),
        "onResynced" => hook(h, args.first(), |l| cap.on_resynced(l)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    }
}

impl HarnessPlugin for Attachment {
    fn factories(&self) -> Vec<&'static str> {
        vec!["attachment", "attachment-signals"]
    }

    fn build(&self, factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "attachment-signals" {
            return Ok(define::<Value, Value>("attachment-test/signals")
                .capability(&SIGNALS, |_| Ok(Signals))
                .build());
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        Ok(attachment_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![ATTACHMENT.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<AttachmentCapability>() {
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
        if capability.downcast_ref::<AttachmentCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<Signals>()?;
        Some(vec!["abort"])
    }
}
