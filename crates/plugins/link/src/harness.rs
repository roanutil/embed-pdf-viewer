//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's two shims in
//! `tooling/plugin-conformance/src/suites/link.ts`. The `link` factory's
//! `onActivate` config is a scripted callback spec, subscribed to
//! `onActivated` as the controller is built; the event it logs spells a
//! dispatched activation's promise as the string `[promise]`. And a
//! `console.error` whose message starts `[link]` logs as the callback
//! `console.error`.

use std::any::Any;
use std::rc::Rc;

use epdf_annotation_core::{PageRef, PdfLinkTarget};
use epdf_geometry::Point;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, is_undefined, undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::*;

/// The adapter.
pub struct LinkAdapter;

/// The capability object literal's member order.
pub const MEMBERS: &[&str] = &[
    "listLinks",
    "getLink",
    "getLinkAt",
    "listAllLinks",
    "ensureLoaded",
    "isLoaded",
    "resolve",
    "activate",
    "activateAt",
    "getLabel",
    "onActivated",
    "onLoaded",
    "isNavigationEngaged",
];

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

fn arg<T: serde::de::DeserializeOwned>(args: &[Value], index: usize) -> Result<T, Error> {
    from_value(args.get(index).unwrap_or(&undefined()))
}

/// `'target' in value && 'bounds' in value`.
fn is_link(value: &Value) -> bool {
    value.get("target").is_some() && value.get("bounds").is_some()
}

fn input_arg(args: &[Value]) -> Result<LinkInput, Error> {
    let value = args.first().cloned().unwrap_or(Value::Null);
    Ok(if is_link(&value) {
        LinkInput::Link(Box::new(from_value(&value)?))
    } else {
        LinkInput::Target(from_value::<PdfLinkTarget>(&value)?)
    })
}

fn context_arg(args: &[Value], index: usize) -> Result<Option<LinkActivateContext>, Error> {
    present(args, index).map(from_value).transpose()
}

/// The event as the generator's shim logs it.
fn printable(event: &LinkActivatedEvent) -> Value {
    json!({
        "target": to_value(&event.target),
        "activation": event.activation.to_json_with(|_| json!("[promise]")),
        "origin": event.origin,
    })
}

fn activation_json(activation: &LinkActivation, h: &Harness) -> Value {
    activation.to_json_with(|dispatch| h.track(dispatch.map(|result| Ok(to_value(&result)))))
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: 'static>(
    h: &Harness,
    spec: Option<&Value>,
    print: fn(&E) -> Value,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![print(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn call_capability(
    cap: &LinkCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    Ok(match method {
        "listLinks" => Outcome::Returned(to_value(&cap.list_links(&arg::<PageRef>(args, 0)?))),
        "getLink" => {
            let id: String = arg(args, 1)?;
            let link = cap.get_link(&arg(args, 0)?, &id);
            Outcome::Returned(link.map_or(Value::Null, |l| to_value(&l)))
        }
        "getLinkAt" => {
            let link = cap.get_link_at(&arg(args, 0)?, arg::<Point>(args, 1)?);
            Outcome::Returned(link.map_or(Value::Null, |l| to_value(&l)))
        }
        "listAllLinks" => Outcome::promise(cap.list_all_links()),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(&arg(args, 0)?)),
        "isLoaded" => Outcome::Returned(json!(cap.is_loaded(&arg(args, 0)?))),
        "resolve" => Outcome::Returned(cap.resolve(&arg(args, 0)?)?.to_json()),
        "activate" => {
            let activation = cap.activate(&input_arg(args)?, context_arg(args, 1)?.as_ref())?;
            Outcome::Returned(activation_json(&activation, h))
        }
        "activateAt" => {
            let page: PageRef = arg(args, 0)?;
            let point: Point = arg(args, 1)?;
            match cap.activate_at(&page, point, context_arg(args, 2)?.as_ref())? {
                Some(activation) => Outcome::Returned(activation_json(&activation, h)),
                None => Outcome::Returned(Value::Null),
            }
        }
        "getLabel" => {
            let input = args.first().cloned().unwrap_or(Value::Null);
            let target = if is_link(&input) { input["target"].clone() } else { input };
            Outcome::Returned(json!(cap.get_label(&target)))
        }
        "onActivated" => hook(h, args.first(), printable, |l| cap.on_activated(l)),
        "onLoaded" => {
            hook(h, args.first(), |e: &LinkLoadedEvent| to_value(e), |l| cap.on_loaded(l))
        }
        "isNavigationEngaged" => Outcome::Returned(json!(cap.is_navigation_engaged())),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for LinkAdapter {
    fn factories(&self) -> Vec<&'static str> {
        vec!["link"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let log = h.clone();
        let config_ = LinkConfig {
            host: LinkHost {
                console_error: Some(Rc::new(move |message: &str, error: &Error| {
                    log.log("console.error", vec![json!(message), error.to_canonical()]);
                })),
            },
        };
        let on_created = config.get("onActivate").filter(|s| s.get("$fn").is_some()).map(|spec| {
            let callback = h.callback(spec);
            Rc::new(move |api: &LinkCapability| {
                let callback = callback.clone();
                let _ =
                    api.on_activated(move |event| match callback.call(vec![printable(event)]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    });
            }) as Rc<dyn Fn(&LinkCapability)>
        });
        Ok(build_link_plugin(config_, on_created))
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
        Some(call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
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
