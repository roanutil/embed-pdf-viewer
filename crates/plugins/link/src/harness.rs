//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's shim in
//! `tooling/plugin-conformance/src/suites/link.ts`: the `link` factory's
//! `onActivate` config is a scripted callback spec, and the event it logs
//! spells a dispatched activation's promise as the string `[promise]`.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined, undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::{
    LINK, LinkAction, LinkActivateContext, LinkActivateEvent, LinkCapability, LinkPluginConfig,
    LinkState, PdfLinkTarget, initial_link_state, link_plugin, link_reducer,
};

/// The adapter.
pub struct Link;

/// The TypeScript object literal's member order.
pub const MEMBERS: &[&str] = &["linksOn", "ensurePage", "engaged", "activate"];

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

fn pon(args: &[Value]) -> u32 {
    args.first().and_then(Value::as_f64).unwrap_or(f64::NAN) as u32
}

/// The event as the generator's shim logs it.
fn printable(event: &LinkActivateEvent) -> Value {
    json!({
        "target": to_value(&event.target),
        "activation": event.activation.to_json_with(|_| json!("[promise]")),
    })
}

impl HarnessPlugin for Link {
    fn factories(&self) -> Vec<&'static str> {
        vec!["link"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let on_activate = config.get("onActivate").filter(|s| s.get("$fn").is_some()).map(|spec| {
            let callback = h.callback(spec);
            Rc::new(move |event: &LinkActivateEvent| match callback.call(vec![printable(event)]) {
                Answer::Throws(error) => Err(error),
                _ => Ok(()),
            }) as crate::ActivateListener
        });
        Ok(link_plugin(LinkPluginConfig { on_activate }))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![LINK.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<LinkCapability>()?;
        Some(match method {
            "linksOn" => Outcome::from_result(cap.links_on(pon(&args)).map(|v| to_value(&v))),
            "ensurePage" => Outcome::from_result(cap.ensure_page(pon(&args)).map(|()| undefined())),
            "engaged" => Outcome::from_result(cap.engaged().map(|b| json!(b))),
            "activate" => {
                let decoded =
                    (|| -> Result<(PdfLinkTarget, Option<LinkActivateContext>), Error> {
                        let target = from_value(args.first().unwrap_or(&Value::Null))?;
                        let context = present(&args, 1).map(from_value).transpose()?;
                        Ok((target, context))
                    })();
                match decoded.and_then(|(target, context)| cap.activate(&target, context.as_ref()))
                {
                    Ok(activation) => Outcome::Returned(activation.to_json_with(|dispatch| {
                        h.track(dispatch.map(|result| Ok(to_value(&result))))
                    })),
                    Err(error) => Outcome::Threw(error),
                }
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<LinkCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_link_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_link_state()))
}

/// `{changed: next !== state, state: next}`.
fn link_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: LinkState = from_value(&a[0])?;
    let action: LinkAction = from_value(&a[1])?;
    let next = link_reducer(&state, &action);
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("initialLinkState", initial_link_state_fn), ("linkReducer", link_reducer_fn)]
}
