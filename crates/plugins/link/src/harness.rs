//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's shim in
//! `tooling/plugin-conformance/src/suites/link.ts`: the `link` factory's
//! `onActivate` config is a scripted callback spec, subscribed to
//! `onActivated` as the controller is built (and released with the
//! instance). The event it logs spells a dispatched activation's promise as
//! the string `[promise]`.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::geometry::Point;
use epdf_kernel::{
    AnyPlugin, AnyToken, ControllerContext, Error, Unsubscribe, is_undefined, undefined,
};
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
    "getStatus",
    "resolve",
    "activate",
    "activateAt",
    "getLabel",
    "onActivated",
    "onLoaded",
    "registerUriOpener",
    "isNavigationEngaged",
];

fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(undefined)
}

/// A `PageRef | number` argument. Anything else names no page.
fn page_arg(args: &[Value], index: usize) -> PageTarget {
    let value = arg(args, index);
    match &value {
        Value::Number(n) => PageTarget::Index(n.as_f64().unwrap_or(f64::NAN)),
        other => PageRef::from_json(other).map_or(PageTarget::Index(f64::NAN), PageTarget::Ref),
    }
}

fn point_arg(args: &[Value], index: usize) -> Result<Point, Error> {
    from_value(&arg(args, index))
}

/// An optional `LinkActivateContext` argument.
fn context_arg(args: &[Value], index: usize) -> Option<LinkActivateContext> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null()).map(LinkActivateContext::from_json)
}

/// `OperationOptions` carry only a signal, which no vector passes.
fn options() -> OperationOptions {
    OperationOptions::default()
}

/// The event as the generator's shim logs it.
fn printable(event: &LinkActivatedEvent) -> Value {
    json!({
        "target": event.target,
        "activation": event.activation.to_json_with(|_| json!("[promise]")),
    })
}

fn activation_json(activation: &LinkActivation, h: &Harness) -> Value {
    activation.to_json_with(|dispatch| h.track(dispatch.map(|result| Ok(to_value(&result)))))
}

fn or_null(link: Option<Link>) -> Value {
    link.unwrap_or(Value::Null)
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
        "listLinks" => Outcome::Returned((*cap.list_links(page_arg(args, 0))).clone()),
        "getLink" => {
            let id = arg(args, 1);
            let link = id.as_str().and_then(|id| cap.get_link(page_arg(args, 0), id));
            Outcome::Returned(or_null(link))
        }
        "getLinkAt" => {
            Outcome::Returned(or_null(cap.get_link_at(page_arg(args, 0), point_arg(args, 1)?)))
        }
        "listAllLinks" => Outcome::Promise(cap.list_all_links(options())),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(page_arg(args, 0), options())),
        "isLoaded" => Outcome::Returned(json!(cap.is_loaded(page_arg(args, 0)))),
        "getStatus" => Outcome::Returned(json!(cap.get_status(page_arg(args, 0)).as_str())),
        "resolve" => Outcome::Returned(cap.resolve(&arg(args, 0)).to_json()),
        "activate" => {
            let activation = cap.activate(&arg(args, 0), context_arg(args, 1).as_ref());
            Outcome::Returned(activation_json(&activation, h))
        }
        "activateAt" => {
            let point = point_arg(args, 1)?;
            match cap.activate_at(page_arg(args, 0), point, context_arg(args, 2).as_ref())? {
                Some(activation) => Outcome::Returned(activation_json(&activation, h)),
                None => Outcome::Returned(Value::Null),
            }
        }
        "getLabel" => Outcome::Returned(json!(cap.get_label(&arg(args, 0)))),
        "onActivated" => hook(h, args.first(), printable, |l| cap.on_activated(l)),
        "onLoaded" => hook(h, args.first(), LinkLoadedEvent::to_json, |l| cap.on_loaded(l)),
        "isNavigationEngaged" => Outcome::Returned(json!(cap.is_navigation_engaged())),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for LinkAdapter {
    fn factories(&self) -> Vec<&'static str> {
        vec!["link"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let on_created = config.get("onActivate").filter(|s| s.get("$fn").is_some()).map(|spec| {
            let callback = h.callback(spec);
            Rc::new(move |api: &LinkCapability, cx: &ControllerContext| {
                let callback = callback.clone();
                // `void callback(...)`: what it answers is dropped, a throw
                // is the listener's.
                cx.listen(api.on_activated(
                    move |event| match callback.call(vec![printable(event)]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    },
                ));
            }) as Rc<dyn Fn(&LinkCapability, &ControllerContext)>
        });
        Ok(build_link_plugin(on_created))
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

/// The generator's `resolveLinkTarget(target, pages)`: a page is known when
/// one of `pages` has its `objectNumber`.
fn resolve_link_target_fn(args: &[Value]) -> Result<Value, Error> {
    let target = arg(args, 0);
    let pages = arg(args, 1);
    let known = pages.as_array().cloned().unwrap_or_default();
    let resolution = resolve_link_target(&target, |page| {
        let number = |p: &Value| p.get("objectNumber").and_then(epdf_annotation_core::json::number);
        known.iter().any(|p| number(p) == number(page))
    });
    Ok(resolution.to_json())
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("resolveLinkTarget", resolve_link_target_fn)]
}
