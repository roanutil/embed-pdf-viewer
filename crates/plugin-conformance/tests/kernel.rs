//! K0: `packages/core/main` against `vectors/kernel/`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::compose::compose_api;
use epdf_kernel::error::{plugin_error, to_plugin_error, to_plugin_error_of_value};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::scope::cancelled;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::{DocInfo, doc_info_equals, doc_info_list_equals, origin_of};
use epdf_kernel::{Error, Unsubscribe};
use plugin_conformance::{FunctionTable, Registry, assert_suite};
use serde_json::{Value, json};

#[test]
fn the_kernel_matches_the_typescript() {
    assert_suite("kernel/lifecycle.json", &Registry::new(), &Vec::new());
}

#[test]
fn the_kernel_settles_on_the_turns_the_typescript_does() {
    assert_suite("kernel/turns.json", &Registry::new(), &Vec::new());
}

fn info(value: &Value) -> Result<DocInfo, Error> {
    serde_json::from_value(value.clone()).map_err(|e| Error::new(e.to_string()))
}

fn infos(value: &Value) -> Result<Vec<DocInfo>, Error> {
    serde_json::from_value(value.clone()).map_err(|e| Error::new(e.to_string()))
}

/// `eventHookScript` from `kernel-utils.ts`.
fn event_hook_script(args: &[Value]) -> Result<Value, Error> {
    let log: Rc<RefCell<Vec<Value>>> = Rc::default();
    let sink = log.clone();
    type Sink = Option<Rc<dyn Fn(&Error)>>;
    let on_error: Sink = if args[1] == json!(true) {
        Some(Rc::new(move |e: &Error| sink.borrow_mut().push(json!({ "error": e.message }))))
    } else {
        None
    };
    let hook = EventHookSource::<Value>::new(on_error);
    let offs: Rc<RefCell<HashMap<String, Unsubscribe>>> = Rc::default();
    let controllers: RefCell<HashMap<String, AbortSignal>> = RefCell::default();
    let signal_of =
        |name: &str| controllers.borrow_mut().entry(name.to_string()).or_default().clone();

    fn listen_with(
        hook: &EventHookSource<Value>,
        offs: &Rc<RefCell<HashMap<String, Unsubscribe>>>,
        log: &Rc<RefCell<Vec<Value>>>,
        id: String,
        then: Rc<dyn Fn() -> Result<(), Error>>,
        signal: Option<&AbortSignal>,
    ) {
        let log = log.clone();
        let who = id.clone();
        let off = hook.on_signal(
            move |event| {
                log.borrow_mut().push(json!({ "id": who, "event": event }));
                then()
            },
            signal,
        );
        offs.borrow_mut().insert(id, off);
    }
    fn listen(
        hook: &EventHookSource<Value>,
        offs: &Rc<RefCell<HashMap<String, Unsubscribe>>>,
        log: &Rc<RefCell<Vec<Value>>>,
        id: String,
        then: Rc<dyn Fn() -> Result<(), Error>>,
    ) {
        listen_with(hook, offs, log, id, then, None);
    }

    for step in args[0].as_array().into_iter().flatten() {
        let id = step["id"].as_str().unwrap_or_default().to_string();
        match step["op"].as_str().unwrap_or_default() {
            "on" => listen(&hook, &offs, &log, id, Rc::new(|| Ok(()))),
            "onThrow" => {
                let message = step["message"].as_str().unwrap_or_default().to_string();
                listen(&hook, &offs, &log, id, Rc::new(move || Err(Error::new(message.clone()))));
            }
            "onAdd" => {
                let adds = step["adds"].as_str().unwrap_or_default().to_string();
                let added = Rc::new(RefCell::new(false));
                let (h, o, l) = (hook.clone(), offs.clone(), log.clone());
                listen(
                    &hook,
                    &offs,
                    &log,
                    id,
                    Rc::new(move || {
                        if !*added.borrow() {
                            *added.borrow_mut() = true;
                            listen(&h, &o, &l, adds.clone(), Rc::new(|| Ok(())));
                        }
                        Ok(())
                    }),
                );
            }
            "off" => {
                let off = offs.borrow().get(&id).cloned();
                if let Some(off) = off {
                    off.call();
                }
            }
            "offTwice" => {
                let off = offs.borrow().get(&id).cloned();
                if let Some(off) = off {
                    off.call();
                    off.call();
                }
            }
            "emit" => {
                log.borrow_mut().push(json!({ "emit": step["event"] }));
                hook.emit(&step["event"]);
            }
            "dispose" => {
                hook.dispose();
                log.borrow_mut().push(json!("disposed"));
            }
            "onSignal" => {
                let signal = signal_of(step["signal"].as_str().unwrap_or_default());
                listen_with(&hook, &offs, &log, id, Rc::new(|| Ok(())), Some(&signal));
            }
            "abort" => {
                let name = step["signal"].as_str().unwrap_or_default();
                signal_of(name).abort(Error::new("aborted"));
                log.borrow_mut().push(json!({ "aborted": name }));
            }
            other => return Err(Error::new(format!("unknown op {other}"))),
        }
    }
    let out = log.borrow().clone();
    Ok(Value::Array(out))
}

/// `errorOf` from `kernel-utils.ts`: an error the vocabulary can meet,
/// including the classes a table cannot make. `Err` carries a thrown value
/// that is not an error: its `String()` and the value.
fn error_of(input: &Value) -> Result<Error, (String, Value)> {
    Ok(match input.get("class").and_then(Value::as_str) {
        Some("PermissionDenied") => Error::permission_denied(
            input["required"].as_str().unwrap_or_default(),
            input.get("context").and_then(Value::as_str),
        ),
        Some("CancelledError") => cancelled(input["what"].as_str().unwrap_or_default()),
        Some("PluginError") => plugin_error(
            input["code"].as_str().unwrap_or_default(),
            "origin",
            input["message"].as_str().unwrap_or_default(),
            None,
            input.get("details").cloned(),
        ),
        Some("string") => {
            let value = input["value"].clone();
            return Err((value.as_str().unwrap_or_default().to_string(), value));
        }
        _ => Error::from_spec(input),
    })
}

fn compose_api_script(args: &[Value]) -> Result<Value, Error> {
    let slices = args[0]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, keys)| {
            keys.as_array()
                .into_iter()
                .flatten()
                .map(|k| (k.as_str().unwrap_or_default().to_string(), json!(index)))
                .collect()
        })
        .collect();
    let api = compose_api("probe", slices)?;
    Ok(Value::Array(api.into_iter().map(|(k, v)| json!([k, v])).collect()))
}

#[test]
fn the_kernel_helpers_match_the_typescript() {
    let functions: FunctionTable = vec![
        ("docInfoEquals", |a| Ok(json!(doc_info_equals(&info(&a[0])?, &info(&a[1])?)))),
        ("docInfoListEquals", |a| Ok(json!(doc_info_list_equals(&infos(&a[0])?, &infos(&a[1])?)))),
        ("docInfoListEqualsItself", |a| {
            let list = infos(&a[0])?;
            Ok(json!(doc_info_list_equals(&list, &list)))
        }),
        ("eventHookScript", event_hook_script),
        ("composeApiScript", compose_api_script),
        ("toPluginError", |a| {
            Ok(match error_of(&a[0]) {
                Ok(error) => to_plugin_error("probe", &error).to_canonical(),
                Err((message, value)) => {
                    to_plugin_error_of_value("probe", &message, value).to_canonical()
                }
            })
        }),
        ("isPluginError", |a| {
            let code = a.get(1).and_then(Value::as_str);
            Ok(json!(error_of(&a[0]).is_ok_and(|e| e.is_plugin_error(code))))
        }),
        ("toPluginErrorInfo", |a| {
            error_of(&a[0]).map(|e| e.plugin_error_info()).map_err(|(m, _)| Error::new(m))
        }),
        ("originOf", |a| Ok(origin_of(&a[0]))),
    ];
    assert_suite("kernel/utils.json", &Registry::new(), &functions);
}

// ── the typed layer ───────────────────────────────────────────────────────────
//
// The probes are untyped, so the vectors never reach the typed wrappers every
// real plugin goes through: `Controller`, `Ctx`, the typed resolvers, and the
// accessors the TypeScript has as plain properties. This drives one typed
// document plugin and one typed workspace plugin through a real kernel.

mod typed {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::LazyLock;

    use epdf_kernel::kernel::{KernelOptions, OpenOptions, OpenSource};
    use epdf_kernel::plugin::slice_key;
    use epdf_kernel::rt;
    use epdf_kernel::{Controller, Ctx, Kernel, Token, define};
    use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
    use serde::Serialize;
    use serde_json::json;

    #[derive(Clone, Serialize)]
    struct Count {
        n: i64,
    }

    #[derive(Clone, Serialize)]
    #[serde(tag = "type")]
    enum Add {
        #[serde(rename = "ADD")]
        By { by: i64 },
    }

    fn reduce(state: &Count, action: &Add) -> Option<Count> {
        let Add::By { by } = action;
        Some(Count { n: state.n + by })
    }

    #[derive(Clone)]
    struct CounterApi(Controller<Count, Add>);

    #[derive(Clone)]
    struct WorkspaceApi(Ctx<Count, Add>);

    static COUNTER: LazyLock<Token<CounterApi>> = LazyLock::new(|| Token::new("counter"));
    static WORKSPACE: LazyLock<Token<WorkspaceApi>> = LazyLock::new(|| Token::new("tally"));

    #[test]
    fn typed_plugins_reach_their_state_their_lifetime_and_each_other() {
        rt::reset();
        let torn_down: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = torn_down.clone();
        let counter = define::<Count, Add>("counter")
            .document_scoped()
            .initial_state(|| Count { n: 0 })
            .reduce(reduce)
            .create(&COUNTER, move |cx: Controller<Count, Add>| {
                let log = log.clone();
                let id = cx.instance_id().to_string();
                cx.defer(Box::new(move || {
                    log.borrow_mut().push(id);
                    Ok(None)
                }));
                Ok((CounterApi(cx), None))
            })
            .build();
        let workspace = define::<Count, Add>("tally")
            .optional(COUNTER.any())
            .initial_state(|| Count { n: 10 })
            .reduce(reduce)
            .capability(&WORKSPACE, |ctx| Ok(WorkspaceApi(ctx)))
            .build();

        let page = json!({
            "index": 0,
            "ref": { "kind": "objectNumber", "pageObjectNumber": 1 },
            "label": null,
            "size": { "width": 600, "height": 800 },
            "rotation": 0,
            "userUnit": 1,
            "boxes": {
                "media": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
                "crop": { "left": 0, "bottom": 0, "right": 600, "top": 800 }
            }
        });
        let fake = FakeEngine::new(json!({ "documents": { "a": { "pages": [page] } } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![counter, workspace],
            KernelOptions::default(),
        )
        .unwrap();
        let events = kernel.documents().events().unwrap();
        events.opened.on(|_| Ok(()));
        events.closed.on(|_| Ok(()));
        let _ = kernel.start();
        rt::run_until_stalled();
        let input = json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } });
        let _ = kernel.documents().open(OpenSource::Input(input), OpenOptions::default());
        rt::run_until_stalled();

        assert_eq!(kernel.core().active_id.as_deref(), Some("a"));
        let CounterApi(cx) = kernel.try_capability(&COUNTER, Some("a")).unwrap().unwrap();
        assert_eq!(cx.id(), "counter");
        assert_eq!(cx.slice_key(), slice_key("counter", Some("a")));
        assert!(cx.engine().is_some());
        assert!(!cx.lifetime().aborted());
        cx.dispatch(Add::By { by: 2 });
        assert_eq!(cx.state().n, 2);

        let WorkspaceApi(ctx) = kernel.try_capability(&WORKSPACE, None).unwrap().unwrap();
        ctx.dispatch(Add::By { by: 5 });
        assert_eq!(ctx.try_state().map(|s| s.n), Some(15));
        let CounterApi(through_get) = ctx.try_get(&COUNTER).unwrap().unwrap();
        assert_eq!(through_get.state().n, 2);
        let CounterApi(through_doc) = ctx.try_for_document(&COUNTER, "a").unwrap().unwrap();
        assert_eq!(through_doc.instance_id(), cx.instance_id());

        let _ = kernel.documents().close("a");
        rt::run_until_stalled();
        assert!(cx.lifetime().aborted());
        assert_eq!(*torn_down.borrow(), [cx.instance_id().to_string()]);

        assert_eq!(events.opened.listener_count(), 1);
        let _ = kernel.destroy();
        rt::run_until_stalled();
        // The kernel's lifecycle events are disposed with the workspace scope.
        assert_eq!((events.opened.listener_count(), events.closed.listener_count()), (0, 0));
    }
}

/// A document plugin's capability that keeps its context, as most do.
struct Holds {
    _ctx: epdf_kernel::Ctx<Value, Value>,
}

static HOLDS: std::sync::LazyLock<epdf_kernel::Token<Rc<Holds>>> =
    std::sync::LazyLock::new(|| epdf_kernel::Token::new("holds"));

/// A closed document's capabilities are the kernel's no longer: once
/// nothing outside holds one, it is dropped. Its context holds the
/// document's session, and the session held the capability, so without the
/// close letting go this is a cycle that outlives the kernel. This is the
/// default (`release_closed_sessions: true`), so an embedder that takes
/// `KernelOptions::default()` does not leak a session per open and close.
#[test]
fn a_closed_documents_capability_is_dropped() {
    use epdf_kernel::rt;
    use epdf_kernel::{Kernel, KernelOptions, OpenOptions, OpenSource, define};

    rt::reset();
    let fake = plugin_conformance::fake_engine::FakeEngine::new(json!({
        "documents": { "a": { "pages": [] } },
    }));
    let plugin = define::<Value, Value>("holds")
        .document_scoped()
        .capability(&HOLDS, |ctx| Ok(Rc::new(Holds { _ctx: ctx })))
        .build();
    let engine = Rc::new(plugin_conformance::fake_engine::AsEngine(fake));
    let kernel = Kernel::new(engine, vec![plugin], KernelOptions::default()).unwrap();
    let _ = kernel.start();
    let input = json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } });
    let _ = kernel.documents().open(OpenSource::Input(input), OpenOptions::default());
    rt::run_until_stalled();

    let capability = Rc::downgrade(&kernel.capability(&HOLDS, Some("a")).unwrap());
    assert!(capability.upgrade().is_some());
    let _ = kernel.documents().close("a");
    rt::run_until_stalled();
    assert!(capability.upgrade().is_none(), "the closed session still holds its capability");
}

/// With `release_closed_sessions` turned off, a closed session's
/// capabilities (and their connects) stay put and only go quiet once nothing
/// outside holds them: the same session, kept alive by the cycle the flag
/// otherwise breaks.
#[test]
fn a_closed_sessions_capabilities_survive_without_the_flag() {
    use epdf_kernel::rt;
    use epdf_kernel::{Kernel, KernelOptions, OpenOptions, OpenSource, define};

    rt::reset();
    let fake = plugin_conformance::fake_engine::FakeEngine::new(json!({
        "documents": { "a": { "pages": [] } },
    }));
    let plugin = define::<Value, Value>("holds")
        .document_scoped()
        .capability(&HOLDS, |ctx| Ok(Rc::new(Holds { _ctx: ctx })))
        .build();
    let engine = Rc::new(plugin_conformance::fake_engine::AsEngine(fake));
    let options = KernelOptions { release_closed_sessions: false, ..KernelOptions::default() };
    let kernel = Kernel::new(engine, vec![plugin], options).unwrap();
    let _ = kernel.start();
    let input = json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } });
    let _ = kernel.documents().open(OpenSource::Input(input), OpenOptions::default());
    rt::run_until_stalled();

    let capability = Rc::downgrade(&kernel.capability(&HOLDS, Some("a")).unwrap());
    assert!(capability.upgrade().is_some());
    let _ = kernel.documents().close("a");
    rt::run_until_stalled();
    assert!(capability.upgrade().is_some(), "the closed session let go of its capability");
}

/// A ready document needs its open source no longer: only a failed open
/// retries. The source goes when the document commits (here, the thunk and
/// what it captured), while a `$host` it names stays held until close.
#[test]
fn a_ready_document_drops_its_source_and_keeps_its_host_hold() {
    use epdf_kernel::bytes::set_host_releaser;
    use epdf_kernel::rt::{self, Promise};
    use epdf_kernel::{Kernel, KernelOptions, OpenOptions, OpenSource};

    rt::reset();
    let released: Rc<RefCell<Vec<Value>>> = Rc::default();
    let log = released.clone();
    set_host_releaser(Some(Rc::new(move |value: &Value| log.borrow_mut().push(value.clone()))));
    let fake = plugin_conformance::fake_engine::FakeEngine::new(json!({
        "documents": { "a": { "pages": [] }, "b": { "pages": [] } },
    }));
    let engine = Rc::new(plugin_conformance::fake_engine::AsEngine(fake));
    let kernel = Kernel::new(engine, Vec::new(), KernelOptions::default()).unwrap();
    let _ = kernel.start();

    let captured = Rc::new(());
    let watch = Rc::downgrade(&captured);
    let thunk = move |_: AbortSignal| {
        let _keep = &captured;
        Ok(Promise::resolved(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })))
    };
    let _ = kernel.documents().open(OpenSource::Thunk(Rc::new(thunk)), OpenOptions::default());
    let host = json!({ "$host": 7 });
    let input = json!({ "kind": "bytes", "id": "b", "bytes": host });
    let _ = kernel.documents().open(OpenSource::Input(input), OpenOptions::default());
    rt::run_until_stalled();

    let statuses: Vec<Value> =
        kernel.documents().list().iter().map(|d| json!([d.id, d.status])).collect();
    assert_eq!(statuses, [json!(["a", "ready"]), json!(["b", "ready"])]);
    assert!(watch.upgrade().is_none(), "a ready document still holds its open thunk");
    assert!(released.borrow().is_empty(), "the host hold went before close");
    let _ = kernel.documents().close("b");
    rt::run_until_stalled();
    assert_eq!(*released.borrow(), vec![host]);
    set_host_releaser(None);
}
