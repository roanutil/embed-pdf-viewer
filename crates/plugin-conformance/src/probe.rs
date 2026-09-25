//! The probe: `tooling/plugin-conformance/src/probe.ts` in Rust.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::plugin::{EffectContext, PluginContext, PluginDef};
use epdf_kernel::rt::Promise;
use epdf_kernel::scope::Teardown;
use epdf_kernel::store::{SliceJson, SliceReducer, SliceState};
use epdf_kernel::types::PluginScope;
use epdf_kernel::{Action, AnyPlugin, AnyToken, Error};
use serde_json::{Value, json};

use crate::harness::{Harness, HarnessPlugin, Outcome};

/// How a probe's init or teardown finishes.
fn finish(step: Option<&Value>, h: &Harness) -> Result<Option<Promise<()>>, Error> {
    let Some(step) = step else { return Ok(None) };
    if step.is_boolean() {
        return Ok(None);
    }
    if let Some(error) = step.get("throws") {
        return Err(Error::from_spec(error));
    }
    if let Some(error) = step.get("rejects") {
        return Ok(Some(Promise::rejected(Error::from_spec(error))));
    }
    if let Some(label) = step.get("defer").and_then(Value::as_str) {
        return Ok(Some(h.defer(label).map(|_| Ok(()))));
    }
    Ok(None)
}

/// A probe's capability.
#[derive(Clone)]
pub struct ProbeCapability {
    ctx: PluginContext,
    harness: Harness,
    name: String,
}

const MEMBERS: &[&str] = &[
    "dispatch",
    "state",
    "document",
    "core",
    "documentId",
    "handleId",
    "handleFor",
    "get",
    "tryGet",
    "forDocument",
    "cleanup",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !epdf_kernel::is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl ProbeCapability {
    fn call(&self, method: &str, args: &[Value]) -> Outcome {
        let ctx = &self.ctx;
        let value = |v: Value| Outcome::Returned(v);
        let result = |r: Result<bool, Error>| match r {
            Ok(b) => Outcome::Returned(json!(b)),
            Err(e) => Outcome::Threw(e),
        };
        match method {
            "dispatch" => {
                let action = arg(args, 0).cloned().unwrap_or(Value::Null);
                ctx.dispatch_any(&action);
                value(epdf_kernel::undefined())
            }
            "state" => value(match ctx.state_any() {
                Some(state) => state.downcast_ref::<Value>().cloned().unwrap_or(Value::Null),
                None => epdf_kernel::undefined(),
            }),
            "document" => {
                value(ctx.document().map_or(Value::Null, |m| serde_json::to_value(m).unwrap()))
            }
            "core" => value(serde_json::to_value(&*ctx.core()).unwrap()),
            "documentId" => value(ctx.document_id().map_or(Value::Null, |d| json!(d))),
            "handleId" => value(ctx.doc().map_or(Value::Null, |d| json!(d.id()))),
            "handleFor" => {
                let id = arg(args, 0).and_then(Value::as_str);
                value(ctx.document_handle(id).map_or(Value::Null, |d| json!(d.id())))
            }
            "get" => {
                result(ctx.get_any(&self.harness.token(&str_arg(args, 0), None)).map(|_| true))
            }
            "tryGet" => result(
                ctx.try_get_any(&self.harness.token(&str_arg(args, 0), None)).map(|c| c.is_some()),
            ),
            "forDocument" => result(
                ctx.for_document_any(
                    &self.harness.token(&str_arg(args, 0), None),
                    &str_arg(args, 1),
                )
                .map(|_| true),
            ),
            "cleanup" => {
                let label = arg(args, 0).cloned().unwrap_or_else(epdf_kernel::undefined);
                let h = self.harness.clone();
                let name = self.name.clone();
                ctx.cleanup_fn(move || h.log(&format!("{name}.cleanup"), vec![label]));
                value(epdf_kernel::undefined())
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        }
    }
}

pub struct Probe;

fn where_of(ctx: &PluginContext) -> Value {
    ctx.document_id().map_or(Value::Null, |d| json!(d))
}

fn build(config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
    let config = config.clone();
    let name = config["id"].as_str().unwrap_or_default().to_string();
    let token = config
        .get("token")
        .and_then(Value::as_str)
        .map(|t| h.token(t, config.get("hint").and_then(Value::as_str)));
    let tokens = |key: &str| -> Vec<AnyToken> {
        config
            .get(key)
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(|t| {
                        let hint = config
                            .get("requiresHint")
                            .and_then(|m| m.get(t))
                            .and_then(Value::as_str);
                        h.token(t, if key == "requires" { hint } else { None })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let requires = tokens("requires");
    let optional = tokens("optional");
    let scope = match config.get("scope").and_then(Value::as_str) {
        Some("document") => PluginScope::Document,
        _ => PluginScope::Workspace,
    };

    let initial = config.get("initialState").cloned();
    let initial_state: Rc<dyn Fn() -> SliceState> =
        Rc::new(move || Rc::new(initial.clone().unwrap_or_else(|| json!({}))) as SliceState);

    let rules = config.get("reduce").cloned().unwrap_or(Value::Null);
    let reducer: SliceReducer = Rc::new(move |state: &SliceState, action: &dyn Action| {
        let action = action.to_json();
        let kind = action.get("type").and_then(Value::as_str).unwrap_or_default();
        match rules.get(kind).and_then(Value::as_str) {
            Some("replace") => {
                Some(Rc::new(action.get("state").cloned().unwrap_or_else(epdf_kernel::undefined))
                    as SliceState)
            }
            Some("merge") => {
                let mut next = state
                    .downcast_ref::<Value>()
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Some(patch) = action.get("state").and_then(Value::as_object) {
                    for (k, v) in patch {
                        next.insert(k.clone(), v.clone());
                    }
                }
                Some(Rc::new(Value::Object(next)) as SliceState)
            }
            _ => None,
        }
    });
    let to_json: SliceJson =
        Rc::new(|state: &dyn Any| state.downcast_ref::<Value>().cloned().unwrap_or(Value::Null));

    let capability = token.as_ref().map(|_| {
        let h = h.clone();
        let name = name.clone();
        let throws = config.get("capabilityThrows").cloned();
        Rc::new(move |ctx: PluginContext| {
            h.log(&format!("{name}.capability"), vec![where_of(&ctx)]);
            if let Some(error) = &throws {
                return Err(Error::from_spec(error));
            }
            Ok(Rc::new(ProbeCapability { ctx, harness: h.clone(), name: name.clone() })
                as Rc<dyn Any>)
        }) as epdf_kernel::plugin::CapabilityFn
    });

    let init =
        config.get("init").filter(|v| !v.is_null() && *v != &Value::Bool(false)).cloned().map(
            |step| {
                let h = h.clone();
                let name = name.clone();
                Rc::new(move |ctx: PluginContext| {
                    h.log(&format!("{name}.init"), vec![where_of(&ctx)]);
                    finish(Some(&step), &h)
                }) as epdf_kernel::plugin::InitFn
            },
        );

    let effects = config.get("effects").filter(|v| !v.is_null()).cloned().map(|effects| {
        let h = h.clone();
        let name = name.clone();
        Rc::new(move |fx: EffectContext| {
            h.log(&format!("{name}.effects"), vec![where_of(&fx)]);
            if let Some(error) = effects.get("throws") {
                return Err(Error::from_spec(error));
            }
            let listener_throws = effects.get("listenerThrows").cloned();
            let listened = {
                let h = h.clone();
                let name = name.clone();
                Rc::new(move |what: &str, args: Vec<Value>| -> Result<(), Error> {
                    h.log(&format!("{name}.{what}"), args);
                    match &listener_throws {
                        Some(error) => Err(Error::from_spec(error)),
                        None => Ok(()),
                    }
                })
            };
            for kind in effects.get("onAction").and_then(Value::as_array).into_iter().flatten() {
                let listened = listened.clone();
                let at = where_of(&fx);
                fx.on_action(kind.as_str().unwrap_or_default(), move |action| {
                    listened("onAction", vec![at.clone(), action.to_json()])
                });
            }
            if effects.get("watchActive").and_then(Value::as_bool) == Some(true) {
                let listened = listened.clone();
                let at = where_of(&fx);
                let ctx = fx.ctx.clone();
                fx.watch(
                    move || ctx.core().active_id.clone(),
                    |a, b| a == b,
                    move |next, previous| {
                        let show =
                            |v: &Option<String>| v.as_ref().map_or(Value::Null, |s| json!(s));
                        listened("watchActive", vec![at.clone(), show(next), show(previous)])
                    },
                );
            }
            if effects.get("watchState").and_then(Value::as_bool) == Some(true) {
                let listened = listened.clone();
                let at = where_of(&fx);
                let ctx = fx.ctx.clone();
                fx.watch(
                    move || ctx.state_any(),
                    // `Object.is` on the state object.
                    |a, b| match (a, b) {
                        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    },
                    move |next, previous| {
                        let show = |v: &Option<SliceState>| match v {
                            Some(s) => s.downcast_ref::<Value>().cloned().unwrap_or(Value::Null),
                            None => epdf_kernel::undefined(),
                        };
                        listened("watchState", vec![at.clone(), show(next), show(previous)])
                    },
                );
            }
            if let Some(step) = effects.get("cleanup").filter(|v| !v.is_null()).cloned() {
                let h = h.clone();
                let name = name.clone();
                let at = where_of(&fx);
                let teardown: Teardown = Box::new(move || {
                    h.log(&format!("{name}.teardown"), vec![at]);
                    finish(Some(&step), &h)
                });
                fx.cleanup(teardown);
            }
            if let Some(action) = effects.get("dispatch") {
                fx.dispatch_any(action);
            }
            Ok(())
        }) as epdf_kernel::plugin::EffectsFn
    });

    Ok(Rc::new(PluginDef {
        id: name,
        token,
        scope,
        requires,
        optional,
        initial_state,
        reducer,
        to_json,
        capability,
        init,
        effects,
    }))
}

impl HarnessPlugin for Probe {
    fn factories(&self) -> Vec<&'static str> {
        vec!["probe"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        build(config, h)
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        let probe = capability.downcast_ref::<ProbeCapability>()?;
        Some(probe.call(method, &args))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<ProbeCapability>()?;
        Some(MEMBERS.to_vec())
    }
}
