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
