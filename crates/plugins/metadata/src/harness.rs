//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error};
use plugin_conformance::{Answer, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::{METADATA, MetadataCapability, metadata_plugin};

pub struct Metadata;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &["getSnapshot", "getStatus", "canEdit", "update", "refresh", "onUpdated"];

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
        let cap = capability.downcast_ref::<MetadataCapability>()?;
        Some(match method {
            "getSnapshot" => Outcome::Returned(cap.get_snapshot().unwrap_or(Value::Null)),
            "getStatus" => Outcome::Returned(json!(cap.get_status())),
            "canEdit" => Outcome::from_result(cap.can_edit().map(Value::Bool)),
            "update" => {
                Outcome::Promise(cap.update(args.into_iter().next().unwrap_or(Value::Null), None))
            }
            "refresh" => Outcome::promise_unit(cap.refresh()),
            "onUpdated" => {
                let callback = h.callback(args.first().unwrap_or(&Value::Null));
                let off = cap.on_updated(move |event| {
                    match callback.call(vec![serde_json::to_value(event).unwrap_or(Value::Null)]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    }
                });
                Outcome::Returned(h.register_fn(Rc::new(move |_| {
                    off.call();
                    Outcome::undefined()
                })))
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<MetadataCapability>()?;
        Some(MEMBERS.to_vec())
    }
}
