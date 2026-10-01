//! The probe: `tooling/plugin-conformance/src/probe.ts` in Rust.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::error::plugin_error;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::{PageRef, PageSpace, PdfEdges, Point, Quad, Rect};
use epdf_kernel::plugin::{ControllerContext, Created, EffectContext, PluginContext, PluginDef};
use epdf_kernel::rt::Promise;
use epdf_kernel::scope::{Teardown, cancelled};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::{SliceJson, SliceReducer, SliceState};
use epdf_kernel::types::PluginScope;
use epdf_kernel::{Action, AnyPlugin, AnyToken, DOCUMENTS, Error};
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
    "tryForDocument",
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
            "tryForDocument" => result(
                ctx.try_for_document_any(
                    &self.harness.token(&str_arg(args, 0), None),
                    &str_arg(args, 1),
                )
                .map(|c| c.is_some()),
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

    let create_config = config.get("create").filter(|v| v.is_object()).cloned();
    let create = create_config.clone().map(|create| {
        let h = h.clone();
        let name = name.clone();
        Rc::new(move |cx: ControllerContext| create_api(&name, &create, &h, cx))
            as epdf_kernel::plugin::CreateFn
    });
    let capability = token.as_ref().filter(|_| create_config.is_none()).map(|_| {
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
                let relay = std::cell::RefCell::new(effects.get("watchDispatchOnce").cloned());
                let dispatcher = fx.ctx.clone();
                fx.watch(
                    move || ctx.state_any(),
                    // `Object.is` on the state object.
                    |a, b| match (a, b) {
                        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    },
                    move |next, previous| {
                        let action = relay.borrow_mut().take();
                        if let Some(action) = action {
                            dispatcher.dispatch_any(&action);
                        }
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
        to_text: None,
        capability,
        init,
        effects,
        create,
    }))
}

impl HarnessPlugin for Probe {
    fn factories(&self) -> Vec<&'static str> {
        vec!["probe"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if let Some(same) = config.get("sameAs").and_then(Value::as_str) {
            return h
                .0
                .built
                .borrow()
                .get(same)
                .cloned()
                .ok_or_else(|| Error::new(format!("[probe] no earlier probe \"{same}\"")));
        }
        let plugin = build(config, h)?;
        h.0.built.borrow_mut().insert(plugin.id.clone(), plugin.clone());
        Ok(plugin)
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        if let Some(controller) = capability.downcast_ref::<ProbeController>() {
            return Some(controller.call(method, &args));
        }
        let probe = capability.downcast_ref::<ProbeCapability>()?;
        Some(probe.call(method, &args))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<ProbeController>().is_some() {
            return Some(API.to_vec());
        }
        capability.downcast_ref::<ProbeCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── the create mode (`createApi` in probe.ts) ─────────────────────────────────

/// The api's members, in the TypeScript object's order.
const API: &[&str] = &[
    "instanceId",
    "state",
    "dispatch",
    "docCall",
    "pageCall",
    "allows",
    "docId",
    "docRead",
    "on",
    "emit",
    "onThrowing",
    "abortSignal",
    "waitFor",
    "waitForThrowing",
    "queue",
    "latest",
    "latestCancel",
    "latestRunning",
    "acquire",
    "forPage",
    "tryForPage",
    "assertPageRef",
    "getPage",
    "get",
    "tryGet",
    "forDocument",
    "tryForDocument",
    "cleanup",
];

/// `probeError`: the classes a table cannot make.
fn probe_error(spec: &Value) -> Error {
    match spec.get("class").and_then(Value::as_str) {
        Some("PermissionDenied") => Error::permission_denied(
            spec.get("required").and_then(Value::as_str).unwrap_or_default(),
            spec.get("context").and_then(Value::as_str),
        ),
        Some("CancelledError") => {
            cancelled(spec.get("what").and_then(Value::as_str).unwrap_or_default())
        }
        Some("PluginError") => plugin_error(
            spec.get("code").and_then(Value::as_str).unwrap_or_default(),
            "probe",
            spec.get("message").and_then(Value::as_str).unwrap_or_default(),
            None,
            None,
        ),
        _ => Error::from_spec(spec),
    }
}

/// JavaScript `===` over JSON values: numbers by value, objects never.
fn strict_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Object(_) | Value::Array(_), _) => false,
        _ => a == b,
    }
}

fn describe_space(space: &PageSpace) -> Value {
    let point = Point { x: 10.0, y: 20.0 };
    let quad = Quad {
        p1: Point { x: 1.0, y: 2.0 },
        p2: Point { x: 3.0, y: 2.0 },
        p3: Point { x: 1.0, y: 4.0 },
        p4: Point { x: 3.0, y: 4.0 },
    };
    let edges = space.page_rect_to_pdf(Rect { x: 10.0, y: 20.0, width: 20.0, height: 40.0 });
    json!({
        "crop": space.crop,
        "width": space.width,
        "height": space.height,
        "pdfToPage": space.pdf_to_page(point),
        "pageToPdf": space.page_to_pdf(point),
        "pdfOriginToPage": space.pdf_to_page(Point { x: 0.0, y: 0.0 }),
        "pageOriginToPdf": space.page_to_pdf(Point { x: 0.0, y: 0.0 }),
        "pdfRectToPage": space.pdf_rect_to_page(PdfEdges { left: 10.0, bottom: 20.0, right: 30.0, top: 60.0 }),
        "pageRectToPdf": { "left": edges.left, "right": edges.right, "top": edges.top, "bottom": edges.bottom },
        "pdfQuadToPage": space.pdf_quad_to_page(quad),
        "pageQuadToPdf": space.page_quad_to_pdf(quad),
    })
}

/// A `create` probe's api.
pub struct ProbeController {
    cx: ControllerContext,
    harness: Harness,
    name: String,
    source: EventHookSource<Value>,
    controllers: RefCell<HashMap<String, AbortSignal>>,
}

fn create_api(
    name: &str,
    config: &Value,
    h: &Harness,
    cx: ControllerContext,
) -> Result<Created, Error> {
    h.log(
        &format!("{name}.create"),
        vec![cx.document_id().map_or(Value::Null, |d| json!(d)), json!(cx.instance_id())],
    );
    if let Some(error) = config.get("throws") {
        return Err(Error::from_spec(error));
    }
    let source = cx.event_source::<Value>();
    let api = Rc::new(ProbeController {
        cx: cx.clone(),
        harness: h.clone(),
        name: name.to_string(),
        source,
        controllers: RefCell::new(HashMap::new()),
    });
    let connect =
        config.get("connect").filter(|c| !c.is_null() && **c != Value::Bool(false)).cloned().map(
            |connect| {
                let (h, name, cx) = (h.clone(), name.to_string(), cx.clone());
                Rc::new(move || -> Result<(), Error> {
                    h.log(
                        &format!("{name}.connect"),
                        vec![cx.document_id().map_or(Value::Null, |d| json!(d))],
                    );
                    if connect.is_boolean() {
                        return Ok(());
                    }
                    if let Some(error) = connect.get("throws") {
                        return Err(Error::from_spec(error));
                    }
                    if let Some(action) = connect.get("dispatch") {
                        cx.dispatch_any(action);
                    }
                    if connect.get("listenDoc").and_then(Value::as_bool) == Some(true) {
                        let (h, name) = (h.clone(), name.clone());
                        let off = cx.doc()?.subscribe(Rc::new(move |event: &Value| {
                            h.log(
                                &format!("{name}.docEvent"),
                                vec![event.get("type").cloned().unwrap_or(Value::Null)],
                            );
                        }));
                        cx.listen(off);
                    }
                    if let Some(pon) = connect.get("readPage").and_then(Value::as_u64) {
                        let page = PageRef::new(pon as u32);
                        let index = cx
                            .get_page(page)
                            .and_then(|p| p.get("index").cloned())
                            .unwrap_or(Value::Null);
                        let width = cx.try_for_page(page).map_or(Value::Null, |s| json!(s.width));
                        h.log(&format!("{name}.page"), vec![index, width]);
                    }
                    if connect.get("listenActive").and_then(Value::as_bool) == Some(true) {
                        let documents = cx.get(&*DOCUMENTS)?;
                        if let Some(events) = documents.events() {
                            let (h, name) = (h.clone(), name.clone());
                            let off = events.active_changed.on(move |event| {
                                h.log(&format!("{name}.activeChanged"), vec![event.clone()]);
                                Ok(())
                            });
                            cx.listen(off);
                        }
                    }
                    Ok(())
                }) as epdf_kernel::plugin::ConnectFn
            },
        );
    Ok(Created { api: api as Rc<dyn Any>, connect })
}

impl ProbeController {
    fn signal_of(&self, label: Option<&str>) -> Option<AbortSignal> {
        let label = label?;
        Some(self.controllers.borrow_mut().entry(label.to_string()).or_default().clone())
    }

    fn call(&self, method: &str, args: &[Value]) -> Outcome {
        let cx = &self.cx;
        let h = &self.harness;
        let token = |i: usize| h.token(&str_arg(args, i), None);
        let bool_of = |r: Result<bool, Error>| match r {
            Ok(b) => Outcome::Returned(json!(b)),
            Err(e) => Outcome::Threw(e),
        };
        let signal_arg = |i: usize| self.signal_of(arg(args, i).and_then(Value::as_str));
        match method {
            "instanceId" => Outcome::Returned(json!(cx.instance_id())),
            "state" => Outcome::Returned(
                cx.state_any()
                    .and_then(|s| s.downcast_ref::<Value>().cloned())
                    .unwrap_or(Value::Null),
            ),
            "dispatch" => {
                cx.dispatch_any(&arg(args, 0).cloned().unwrap_or(Value::Null));
                Outcome::undefined()
            }
            "docCall" => match cx.doc() {
                Ok(doc) => Outcome::Promise(doc.call(&str_arg(args, 0), None, args[1..].to_vec())),
                Err(error) => Outcome::Threw(error),
            },
            "pageCall" => {
                let pon = arg(args, 0).and_then(Value::as_u64).unwrap_or(0) as u32;
                let doc = match cx.doc() {
                    Ok(doc) => doc,
                    Err(error) => return Outcome::Threw(error),
                };
                if let Err(error) = doc.page_index(pon) {
                    return Outcome::Threw(error);
                }
                let path = format!("page.{}", str_arg(args, 1));
                Outcome::Promise(doc.call(
                    &path,
                    Some(pon),
                    args.get(2..).map(<[Value]>::to_vec).unwrap_or_default(),
                ))
            }
            "allows" => match cx.doc() {
                Ok(doc) => Outcome::Returned(json!(doc.security().allows(&str_arg(args, 0)))),
                Err(error) => Outcome::Threw(error),
            },
            "docId" => match cx.doc() {
                Ok(doc) => Outcome::Returned(json!(doc.id())),
                Err(error) => Outcome::Threw(error),
            },
            "docRead" => match cx.doc() {
                Ok(doc) => Outcome::Returned(match str_arg(args, 0).as_str() {
                    "id" => json!(doc.id()),
                    "capabilities" => doc.capabilities(),
                    "security.effectiveScope" => json!(doc.security().effective_scope()),
                    "security.identity" => doc.security().identity(),
                    "security.current" => doc.security().current(),
                    "security.passwordPrompt" => doc.security().password_prompt(),
                    "security.allowsAnnotationCreate" => {
                        json!(doc.security().allows_annotation_create())
                    }
                    "security.allowsAnnotationMutation" => {
                        json!(doc.security().allows_annotation_mutation(
                            &str_arg(args, 1),
                            arg(args, 2).unwrap_or(&Value::Null)
                        ))
                    }
                    "security.allowsAnnotationGroupAssignment" => {
                        json!(doc.security().allows_annotation_group_assignment(&str_arg(args, 1)))
                    }
                    "events.lastServerId" => json!(doc.last_server_id()),
                    _ => epdf_kernel::undefined(),
                }),
                Err(error) => Outcome::Threw(error),
            },
            "on" => {
                let label = arg(args, 0).cloned().unwrap_or(Value::Null);
                let (h2, name) = (h.clone(), self.name.clone());
                let off = self.source.on_signal(
                    move |event| {
                        h2.log(&format!("{name}.event"), vec![label.clone(), event.clone()]);
                        Ok(())
                    },
                    signal_arg(1).as_ref(),
                );
                Outcome::Returned(h.register_fn(Rc::new(move |_| {
                    off.call();
                    Outcome::undefined()
                })))
            }
            "emit" => {
                self.source.emit(&arg(args, 0).cloned().unwrap_or_else(epdf_kernel::undefined));
                Outcome::undefined()
            }
            "onThrowing" => {
                let message = str_arg(args, 0);
                let off = self.source.on(move |_| Err(Error::new(message.clone())));
                Outcome::Returned(h.register_fn(Rc::new(move |_| {
                    off.call();
                    Outcome::undefined()
                })))
            }
            "abortSignal" => {
                if let Some(signal) = signal_arg(0) {
                    signal.abort(Error::new("aborted"));
                }
                Outcome::undefined()
            }
            "waitFor" => {
                let key = str_arg(args, 0);
                let want = arg(args, 1).cloned().unwrap_or_else(epdf_kernel::undefined);
                let state = cx.plugin_context().clone();
                Outcome::promise_unit(cx.wait_for(
                    move || {
                        let current =
                            state.state_any().and_then(|s| s.downcast_ref::<Value>().cloned());
                        Ok(current
                            .and_then(|s| s.get(&key).cloned())
                            .is_some_and(|v| strict_equal(&v, &want)))
                    },
                    signal_arg(2),
                ))
            }
            "waitForThrowing" => {
                let message = str_arg(args, 0);
                Outcome::promise_unit(cx.wait_for(move || Err(Error::new(message.clone())), None))
            }
            "queue" => {
                let (key, label) = (str_arg(args, 0), str_arg(args, 1));
                let (h2, name) = (h.clone(), self.name.clone());
                let queued = json!([key, label]);
                Outcome::Promise(cx.serial_queue(&key).run(move || {
                    h2.log(
                        &format!("{name}.queued"),
                        queued.as_array().cloned().unwrap_or_default(),
                    );
                    Ok(h2.defer(&label))
                }))
            }
            "latest" => {
                let (key, label) = (str_arg(args, 0), str_arg(args, 1));
                let throws = arg(args, 3).cloned();
                let (h2, name) = (h.clone(), self.name.clone());
                Outcome::Promise(cx.latest(&key).run(
                    move |run| {
                        Promise::spawn(async move {
                            h2.log(
                                &format!("{name}.run"),
                                vec![json!(run.id), json!(run.signal.aborted())],
                            );
                            let value = h2.defer(&label).await?;
                            if let Some(spec) = &throws {
                                return Err(probe_error(spec));
                            }
                            let committed = run.commit(|| {
                                h2.log(
                                    &format!("{name}.commit"),
                                    vec![json!(run.id), value.clone()],
                                )
                            });
                            h2.log(
                                &format!("{name}.ran"),
                                vec![json!(run.id), json!(committed), json!(run.superseded())],
                            );
                            Ok(value)
                        })
                    },
                    signal_arg(2),
                ))
            }
            "latestCancel" => {
                cx.latest(&str_arg(args, 0)).cancel(arg(args, 1));
                Outcome::undefined()
            }
            "latestRunning" => Outcome::Returned(json!(cx.latest(&str_arg(args, 0)).running())),
            "acquire" => {
                let label = str_arg(args, 0);
                let (h2, name) = (h.clone(), self.name.clone());
                let (h3, name3) = (h.clone(), self.name.clone());
                Outcome::Promise(cx.acquire(
                    move |lifetime| {
                        h2.log(&format!("{name}.acquiring"), vec![json!(lifetime.aborted())]);
                        Ok(h2.defer(&label))
                    },
                    Rc::new(move |resource: Value| {
                        h3.log(&format!("{name3}.dispose"), vec![resource]);
                        Ok(None)
                    }),
                ))
            }
            "forPage" | "tryForPage" | "assertPageRef" | "getPage" => {
                let page = PageRef::new(arg(args, 0).and_then(Value::as_u64).unwrap_or(0) as u32);
                match method {
                    "forPage" => match cx.for_page(page) {
                        Ok(space) => Outcome::Returned(describe_space(&space)),
                        Err(error) => Outcome::Threw(error),
                    },
                    "tryForPage" => Outcome::Returned(
                        cx.try_for_page(page).map_or(Value::Null, |space| describe_space(&space)),
                    ),
                    "assertPageRef" => match cx.assert_page_ref(page) {
                        Ok(()) => Outcome::undefined(),
                        Err(error) => Outcome::Threw(error),
                    },
                    _ => Outcome::Returned(cx.get_page(page).unwrap_or(Value::Null)),
                }
            }
            "get" => bool_of(cx.get_any(&token(0)).map(|_| true)),
            "tryGet" => bool_of(cx.try_get_any(&token(0)).map(|c| c.is_some())),
            "forDocument" => {
                bool_of(cx.for_document_any(&token(0), &str_arg(args, 1)).map(|_| true))
            }
            "tryForDocument" => {
                bool_of(cx.try_for_document_any(&token(0), &str_arg(args, 1)).map(|c| c.is_some()))
            }
            "cleanup" => {
                let label = arg(args, 0).cloned().unwrap_or_else(epdf_kernel::undefined);
                let (h2, name) = (h.clone(), self.name.clone());
                cx.cleanup_fn(move || h2.log(&format!("{name}.cleanup"), vec![label]));
                Outcome::undefined()
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        }
    }
}
