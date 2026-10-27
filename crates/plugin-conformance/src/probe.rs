//! The probe: `tooling/plugin-conformance/src/probe.ts` in Rust.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::error::plugin_error;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::plugin::{ControllerContext, Created, EffectContext, PluginDef};
use epdf_kernel::rt::Promise;
use epdf_kernel::scope::cancelled;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::{SliceJson, SliceReducer, SliceState};
use epdf_kernel::types::PluginScope;
use epdf_kernel::{AnyPlugin, AnyToken, DOCUMENTS, Error};
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
        return Ok(Some(h.defer_unit(label)));
    }
    Ok(None)
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !epdf_kernel::is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub struct Probe;

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

    let initial = config.get("state").cloned();
    let initial_state: Rc<dyn Fn() -> SliceState> = Rc::new(move || {
        Rc::new(initial.clone().unwrap_or_else(epdf_kernel::undefined)) as SliceState
    });

    let reducer: SliceReducer = Rc::new(|_, _| None);
    let to_json: SliceJson =
        Rc::new(|state| state.downcast_ref::<Value>().cloned().unwrap_or(Value::Null));
    let (h, create_name) = (h.clone(), name.clone());
    let create =
        Some(Rc::new(move |cx: ControllerContext| create_api(&create_name, &config, &h, cx))
            as epdf_kernel::plugin::CreateFn);

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
        settings: None,
        capability: None,
        init: None,
        effects: None,
        create,
        resolves_any_capability: false,
        in_scope: None,
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
        None
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<ProbeController>().is_some() {
            return Some(API.to_vec());
        }
        None
    }
}

// ── the create mode (`createApi` in probe.ts) ─────────────────────────────────

/// The api's members, in the TypeScript object's order.
const API: &[&str] = &[
    "instanceId",
    "state",
    "replaceState",
    "mergeState",
    "sameState",
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
    "assertPageRef",
    "getPage",
    "get",
    "tryGet",
    "forDocument",
    "tryForDocument",
    "cleanup",
    "signalReason",
    "document",
    "documentId",
    "handleId",
    "handleFor",
    "pageOf",
    "notify",
];

/// `controller.abort()` with no reason: the platform's `DOMException`.
fn dom_abort_error() -> Error {
    Error::named("AbortError", "This operation was aborted")
}

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
    if let Some(error) = config.get("createThrows") {
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
    let connect = config
        .get("connect")
        .filter(|c| !c.is_null() && **c != Value::Bool(false))
        .cloned()
        .map(|connect| {
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
                let at = cx.document_id().map_or(Value::Null, |d| json!(d));
                if connect.get("watchState").and_then(Value::as_bool) == Some(true) {
                    let (h, name, cx, at) = (h.clone(), name.clone(), cx.clone(), at.clone());
                    let relay = RefCell::new(connect.get("watchMergeOnce").cloned());
                    let listener_error = connect.get("listenerThrows").cloned();
                    let lease = cx.lease().clone();
                    let owner = cx.clone();
                    let off = lease.on_change(move |change| {
                        let patch = relay.borrow_mut().take();
                        if let Some(patch) = patch {
                            let next = merge_state(&cx, &patch);
                            cx.lease().write(Rc::new(next));
                        }
                        let show = |v: &SliceState| {
                            v.downcast_ref::<Value>()
                                .cloned()
                                .unwrap_or_else(epdf_kernel::undefined)
                        };
                        h.log(
                            &format!("{name}.watchState"),
                            vec![at.clone(), show(&change.next), show(&change.previous)],
                        );
                        listener_error.as_ref().map_or(Ok(()), |e| Err(Error::from_spec(e)))
                    });
                    owner.listen(off);
                }
                if connect.get("watchActive").and_then(Value::as_bool) == Some(true) {
                    let docs = cx.get(&*DOCUMENTS)?;
                    let (h, name, at) = (h.clone(), name.clone(), at.clone());
                    let listener_error = connect.get("listenerThrows").cloned();
                    EffectContext { ctx: cx.plugin_context().clone() }.watch(
                        move || docs.get_active_id(),
                        |a, b| a == b,
                        move |next, previous| {
                            h.log(
                                &format!("{name}.watchActive"),
                                vec![at.clone(), json!(next), json!(previous)],
                            );
                            listener_error.as_ref().map_or(Ok(()), |e| Err(Error::from_spec(e)))
                        },
                    );
                }
                if let Some(step) =
                    connect.get("cleanup").filter(|s| **s != Value::Bool(false)).cloned()
                {
                    let (h, name, at) = (h.clone(), name.clone(), at.clone());
                    cx.cleanup(Box::new(move || {
                        h.log(&format!("{name}.teardown"), vec![at]);
                        finish(Some(&step), &h)
                    }));
                }
                if let Some(value) = connect.get("replaceState") {
                    cx.lease().write(Rc::new(value.clone()));
                }
                if let Some(token) = connect.get("tryGet").and_then(Value::as_str) {
                    h.log(
                        &format!("{name}.tryGet"),
                        vec![json!(token), json!(cx.try_get_any(&h.token(token, None))?.is_some())],
                    );
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
                    let width = cx
                        .get_page(page)
                        .and_then(|p| p.get("size").and_then(|s| s.get("width")).cloned())
                        .unwrap_or(Value::Null);
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
        });
    Ok(Created { api: api as Rc<dyn Any>, connect })
}

fn merge_state(cx: &ControllerContext, patch: &Value) -> Value {
    let mut next = cx
        .state_any()
        .and_then(|s| s.downcast_ref::<Value>().and_then(Value::as_object).cloned())
        .unwrap_or_default();
    if let Some(patch) = patch.as_object() {
        next.extend(patch.clone());
    }
    Value::Object(next)
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
            "replaceState" | "mergeState" => {
                let value = arg(args, 0).cloned().unwrap_or_else(epdf_kernel::undefined);
                let next = if method == "mergeState" { merge_state(cx, &value) } else { value };
                cx.lease().write(Rc::new(next));
                Outcome::undefined()
            }
            // `ctx.state.update(s => s)`: a write of the same state, which
            // still reports a closed instance.
            "sameState" => {
                let lease = cx.lease();
                lease.write(lease.read());
                Outcome::undefined()
            }
            "document" => Outcome::Returned(cx.document().map_or(Value::Null, |d| json!(d))),
            "documentId" => Outcome::Returned(cx.document_id().map_or(Value::Null, |d| json!(d))),
            "handleId" => Outcome::from_result(cx.doc().map(|d| json!(d.id()))),
            "handleFor" => Outcome::Returned(
                cx.document_handle(arg(args, 0).and_then(Value::as_str))
                    .map_or(Value::Null, |d| json!(d.id())),
            ),
            "pageOf" => {
                let page = arg(args, 0).cloned().unwrap_or(Value::Null);
                let found = if let Some(index) = page.as_u64() {
                    cx.document().and_then(|d| {
                        d.pages.as_array().and_then(|pages| pages.get(index as usize)).cloned()
                    })
                } else {
                    PageRef::from_json(&page).and_then(|p| cx.get_page(p))
                };
                match found {
                    Some(page) => Outcome::Returned(page),
                    None => Outcome::Threw(plugin_error(
                        "not-found",
                        &self.name,
                        format!(
                            "page {} is not in this document",
                            page.as_u64().or_else(|| page["objectNumber"].as_u64()).unwrap_or(0)
                        ),
                        None,
                        None,
                    )),
                }
            }
            "notify" => {
                cx.notify();
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
                    "objectNumbers.held" => json!(doc.held_object_numbers()),
                    "objectNumbers.take" => match doc.take_object_number() {
                        Ok(n) => json!(n),
                        Err(e) => return Outcome::Threw(e),
                    },
                    "capabilities" => doc.capabilities(),
                    "security.scope" => json!(doc.security().effective_scope()),
                    "security.identity" => doc.security().identity(),
                    "security.state" => doc.security().current(),
                    "security.passwordPrompt" => doc.security().password_prompt(),
                    "security.allowsAnnotation" => json!(doc.security().allows_annotation(
                        &str_arg(args, 1),
                        arg(args, 2).unwrap_or(&Value::Null)
                    )),
                    "security.allowsField" => json!(
                        doc.security()
                            .allows_field(&str_arg(args, 1), arg(args, 2).unwrap_or(&Value::Null))
                    ),
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
                    "events.lastServerId" => doc
                        .last_server_id()
                        .map_or(Value::Null, |n| n.parse::<i64>().map_or(json!(n), |n| json!(n))),
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
                    signal.abort(arg(args, 1).map_or_else(dom_abort_error, Error::from_spec));
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
            "assertPageRef" | "getPage" => {
                let page = PageRef::new(arg(args, 0).and_then(Value::as_u64).unwrap_or(0) as u32);
                match method {
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
            "signalReason" => Outcome::Returned(
                self.controllers
                    .borrow()
                    .get(&str_arg(args, 0))
                    .and_then(AbortSignal::reason)
                    .map_or_else(epdf_kernel::undefined, |reason| reason.to_canonical()),
            ),
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        }
    }
}
