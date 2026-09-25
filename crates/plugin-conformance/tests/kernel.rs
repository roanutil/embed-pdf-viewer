//! K0: `packages/core/main` against `vectors/kernel/`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::types::{DocInfo, doc_info_equals, doc_info_list_equals};
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

    fn listen(
        hook: &EventHookSource<Value>,
        offs: &Rc<RefCell<HashMap<String, Unsubscribe>>>,
        log: &Rc<RefCell<Vec<Value>>>,
        id: String,
        then: Rc<dyn Fn() -> Result<(), Error>>,
    ) {
        let log = log.clone();
        let who = id.clone();
        let off = hook.on(move |event| {
            log.borrow_mut().push(json!({ "id": who, "event": event }));
            then()
        });
        offs.borrow_mut().insert(id, off);
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
            other => return Err(Error::new(format!("unknown op {other}"))),
        }
    }
    let out = log.borrow().clone();
    Ok(Value::Array(out))
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
    ];
    assert_suite("kernel/utils.json", &Registry::new(), &functions);
}
