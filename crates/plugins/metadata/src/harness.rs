//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error};
use plugin_conformance::{Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::{METADATA, MetadataCapability, metadata_plugin};

pub struct Metadata;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &["canEdit", "current", "update", "reload"];

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
        _h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<MetadataCapability>()?;
        Some(match method {
            "canEdit" => Outcome::Returned(json!(cap.can_edit())),
            "current" => Outcome::Returned(cap.current().unwrap_or(Value::Null)),
            "update" => {
                Outcome::promise_unit(cap.update(args.into_iter().next().unwrap_or(Value::Null)))
            }
            "reload" => Outcome::promise_unit(cap.reload()),
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
