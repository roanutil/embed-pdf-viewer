//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name. `custom` is the
//! nested capability, reached by dotted calls (`custom.update`).

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe};
use plugin_conformance::{Answer, Callback, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    CustomMetadataCapability, METADATA, MetadataCapability, OperationOptions, metadata_plugin,
};

pub struct Metadata;

/// The TypeScript object literals' member order (`controller.ts`).
const MEMBERS: &[&str] = &[
    "getSnapshot",
    "getStatus",
    "ensureLoaded",
    "canUpdate",
    "update",
    "refresh",
    "onUpdated",
    "onResynced",
    "custom",
];
const CUSTOM_MEMBERS: &[&str] =
    &["getSnapshot", "getStatus", "update", "refresh", "onUpdated", "onResynced"];

/// `OperationOptions` carry only a signal, which no vector passes.
fn options() -> OperationOptions {
    OperationOptions::default()
}

fn snapshot(value: Option<Rc<Value>>) -> Outcome {
    Outcome::Returned(value.map_or(Value::Null, |value| (*value).clone()))
}

/// A scripted listener: the event as its JSON, a scripted throw reported.
fn listener<T: Serialize>(callback: Callback) -> impl Fn(&T) -> Result<(), Error> {
    move |event| match callback.call(vec![serde_json::to_value(event).unwrap_or(Value::Null)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }
}

/// The unsubscribe a listener verb returns, as a callable ref.
fn unsubscribe(off: Unsubscribe, h: &Harness) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn first(args: Vec<Value>) -> Value {
    args.into_iter().next().unwrap_or_else(epdf_kernel::undefined)
}

fn not_a_function(name: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{name}\" is not a function")))
}

impl HarnessPlugin for Metadata {
    fn factories(&self) -> Vec<&'static str> {
        vec!["metadata"]
    }

    fn build(&self, _factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(metadata_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![METADATA.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(custom) = capability.downcast_ref::<CustomMetadataCapability>() {
            return Some(match method {
                "getSnapshot" => snapshot(custom.get_snapshot()),
                "getStatus" => Outcome::Returned(json!(custom.get_status().as_str())),
                "update" => Outcome::Promise(custom.update(first(args), options())),
                "refresh" => Outcome::promise_unit(custom.refresh(options())),
                "onUpdated" => {
                    let callback = h.callback(args.first().unwrap_or(&Value::Null));
                    unsubscribe(custom.on_updated(listener(callback)), h)
                }
                "onResynced" => {
                    let callback = h.callback(args.first().unwrap_or(&Value::Null));
                    unsubscribe(custom.on_resynced(listener(callback)), h)
                }
                other => not_a_function(other),
            });
        }
        let cap = capability.downcast_ref::<MetadataCapability>()?;
        Some(match method {
            "getSnapshot" => snapshot(cap.get_snapshot()),
            "getStatus" => Outcome::Returned(json!(cap.get_status().as_str())),
            "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(options())),
            "canUpdate" => Outcome::Returned(Value::Bool(cap.can_update())),
            "update" => Outcome::Promise(cap.update(first(args), options())),
            "refresh" => Outcome::promise_unit(cap.refresh(options())),
            "onUpdated" => {
                let callback = h.callback(args.first().unwrap_or(&Value::Null));
                unsubscribe(cap.on_updated(listener(callback)), h)
            }
            "onResynced" => {
                let callback = h.callback(args.first().unwrap_or(&Value::Null));
                unsubscribe(cap.on_resynced(listener(callback)), h)
            }
            other => not_a_function(other),
        })
    }

    fn resolve_member(
        &self,
        capability: &Rc<dyn Any>,
        member: &str,
        _h: &Harness,
    ) -> Option<Result<Rc<dyn Any>, Error>> {
        let cap = capability.downcast_ref::<MetadataCapability>()?;
        (member == "custom").then(|| Ok(Rc::new(cap.custom()) as Rc<dyn Any>))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<CustomMetadataCapability>().is_some() {
            return Some(CUSTOM_MEMBERS.to_vec());
        }
        capability.downcast_ref::<MetadataCapability>()?;
        Some(MEMBERS.to_vec())
    }
}
