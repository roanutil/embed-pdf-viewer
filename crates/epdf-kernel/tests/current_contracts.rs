use epdf_kernel::error::{Error, to_plugin_error};
use serde_json::{Value, json};

#[test]
fn current_engine_errors_keep_permission_and_conflict_details() {
    for (engine, plugin) in [
        ("WireFormat", "operation-failed"),
        ("PayloadTooLarge", "invalid-input"),
        ("ObjectNumberUnavailable", "conflict"),
        ("LayerFull", "conflict"),
        ("ChangeConflict", "conflict"),
        ("UndoUnavailable", "conflict"),
        ("IdempotencyKeyReused", "invalid-input"),
    ] {
        assert_eq!(
            to_plugin_error("test", &Error::engine(engine, "refused")).code(),
            Some(plugin),
            "{engine}"
        );
    }
    let error = to_plugin_error(
        "test",
        &Error::engine("Forbidden", "refused")
            .with("details", json!({"required":"annotations:update"})),
    );
    assert_eq!(error.plugin_error_info()["permission"], "annotations:update");
}

#[test]
fn a_permission_refusal_keeps_its_details_whole() {
    let refused = Error::permission_denied("doc.forms.modify", Some("placement"))
        .with("details", json!({"required":"doc.forms.modify","anyOf":["doc.forms.modify"]}));
    let error = to_plugin_error("form", &refused);
    assert_eq!(error.plugin_error_info()["permission"], "doc.forms.modify");
    assert_eq!(
        error.members["details"],
        json!({"required":"doc.forms.modify","anyOf":["doc.forms.modify"]})
    );
    let mut bare = Error::permission_denied("doc.forms.read", Some("list"));
    bare.members.remove("details");
    let error = to_plugin_error("form", &bare);
    assert_eq!(error.plugin_error_info()["permission"], Value::Null);
    assert!(!error.members.contains_key("details"));
}

use epdf_kernel::{
    Promise,
    changes::{DocumentChanges, StagedChange},
    event_hook::EventHookSource,
    mirror::{Fold, Loaded, Mirror, MirrorEnvironment, MirrorSpec},
    rt,
    signal::AbortSignal,
};
use std::rc::Rc;

#[test]
fn pending_refusal_preserves_remote_truth_and_close_revokes() {
    rt::reset();
    let (answer, reject) = Promise::pending();
    let q = DocumentChanges::new(
        Rc::new(move |_, _| answer.clone()),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let events = EventHookSource::new(None);
    let life = AbortSignal::new();
    let spec = MirrorSpec::new(
        "counter",
        json!(0),
        Rc::new(|_, _| Promise::resolved(Loaded { value: json!(0), cursor: None })),
        Rc::new(|_, event| Ok(Fold::Value(Rc::new(event["value"].clone())))),
    );
    let mut spec = spec;
    spec.predict = Some(Rc::new(|_, op| Ok(Rc::new(op["value"].clone()))));
    let mirror =
        Mirror::new(spec, MirrorEnvironment::detached(events.clone(), life.clone(), q.clone()));
    let change = q
        .for_plugin("test")
        .stage(StagedChange::ops(
            json!({"key":"edit"}),
            vec![json!({"type":"metadata.set", "value":1})],
        ))
        .unwrap();
    assert_eq!(*mirror.view(), json!(1));
    events.emit(&json!({"type":"metadata.changed","value":2,"origin":{"kind":"remote"}}));
    assert_eq!(*mirror.get(), json!(2));
    reject.reject(Error::engine("ChangeConflict", "stale"));
    rt::run_until_stalled();
    assert_eq!(*mirror.view(), json!(2));
    assert!(change.result().peek().unwrap().is_err());
    life.abort(Error::new("closed"));
    events.emit(&json!({"type":"metadata.changed","value":3}));
    assert_eq!(*mirror.get(), json!(2));
    q.close();
    assert!(
        q.for_plugin("test")
            .stage(StagedChange::ops(json!({"key":"edit"}), vec![json!({"type":"metadata.set"})]))
            .is_err()
    );
}

#[test]
fn own_fact_folds_before_settlement_and_every_origin_uses_same_fold() {
    rt::reset();
    let events = EventHookSource::new(None);
    let publish = events.clone();
    let q = DocumentChanges::new(
        Rc::new(move |_, options| {
            publish.emit(&json!({"type":"metadata.changed","value":4,"origin":{"kind":"local","tx":{"id": options["opId"], "index":0,"count":1}}}));
            Promise::resolved(json!({"applied":true}))
        }),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let mut spec = MirrorSpec::new(
        "counter",
        json!(0),
        Rc::new(|_, _| Promise::resolved(Loaded { value: json!(0), cursor: None })),
        Rc::new(|_, event| Ok(Fold::Value(Rc::new(event["value"].clone())))),
    );
    spec.predict = Some(Rc::new(|_, op| Ok(Rc::new(op["value"].clone()))));
    let mirror = Mirror::new(
        spec,
        MirrorEnvironment::detached(events.clone(), AbortSignal::new(), q.clone()),
    );
    let staged = q
        .for_plugin("test")
        .stage(StagedChange::ops(json!({"key":"edit"}), vec![json!({"value":9})]))
        .unwrap();
    assert_eq!(*mirror.view(), json!(4));
    assert!(staged.result().peek().is_none());
    events.emit(&json!({"type":"metadata.changed","value":5,"origin":{"kind":"remote"}}));
    assert_eq!(*mirror.get(), json!(5));
    rt::run_until_stalled();
    assert!(staged.result().peek().unwrap().is_ok());
}

#[test]
fn registration_settings_merge_reset_and_dispose_instance_listener() {
    use epdf_kernel::{
        scope::Scope,
        settings::{Settings, SettingsDeclaration},
    };
    let settings = Settings::new(
        SettingsDeclaration {
            defaults: json!({"person":{"name":"a","id":1},"nested":{"x":1,"y":2}}),
            registered: Some(json!({"nested":{"x":3}})),
            whole: vec!["person".into()],
        },
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let scope = Scope::new(Rc::new(|e| panic!("{e}")));
    let instance = settings.for_instance(&scope);
    let called = Rc::new(std::cell::Cell::new(0));
    let count = called.clone();
    instance.events.on(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    let same = settings.get();
    settings.update_settings(&json!({"nested":{"x":3}}));
    assert!(Rc::ptr_eq(&same, &settings.get()));
    settings.update_settings(&json!({"person":{"name":"b"},"nested":{"y":8}}));
    assert_eq!(*instance.get(), json!({"person":{"name":"b"},"nested":{"x":3,"y":8}}));
    scope.dispose();
    rt::run_until_stalled();
    settings.reset_settings();
    assert_eq!(called.get(), 1);
    assert_eq!(settings.get()["nested"]["x"], 3);
}

#[test]
fn current_refs_have_object_numbers_and_stable_inline_annotation_names() {
    use epdf_kernel::geometry::{AnnotationRef, PageRef};
    let page = PageRef::new(42);
    assert_eq!(page.to_json(), json!({"kind":"objectNumber","objectNumber":42}));
    assert!(PageRef::from_json(&json!({"kind":"objectNumber","pageObjectNumber":42})).is_none());
    let inline = json!({"kind":"baseIndex","page":page.to_json(),"baseIndex":0});
    let reference: AnnotationRef = serde_json::from_value(inline.clone()).unwrap();
    assert_eq!(serde_json::to_value(reference).unwrap(), inline);
}

#[test]
fn page_load_epochs_and_delete_prevent_stale_replacement() {
    use epdf_kernel::page_mirror::{Affected, PageMirror, PageMirrorSpec};
    rt::reset();
    let events = EventHookSource::new(None);
    let q = DocumentChanges::new(
        Rc::new(|_, _| Promise::resolved(json!({}))),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let (first, a) = Promise::pending();
    let (second, b) = Promise::pending();
    let loads = Rc::new(std::cell::RefCell::new(vec![first, second]));
    let source = loads.clone();
    let page = json!({"kind":"objectNumber","objectNumber":3});
    let mirror = PageMirror::new(
        PageMirrorSpec {
            name: "page".into(),
            load: Rc::new(move |_, _, _| source.borrow_mut().remove(0)),
            affected: Rc::new(|_| Affected::All),
            fold: Some(Rc::new(|_, e, _| Ok(Fold::Value(Rc::new(e["value"].clone()))))),
            predict: None,
            changed: None,
        },
        MirrorEnvironment::detached(events.clone(), AbortSignal::new(), q),
    );
    let load = mirror.ensure_loaded(page.clone());
    let shared = mirror.ensure_loaded(page.clone());
    assert!(load.same(&shared));
    let reload = mirror.refresh(Some(page.clone()));
    b.resolve(json!(2));
    rt::run_until_stalled();
    assert!(reload.peek().unwrap().is_ok());
    a.resolve(json!(1));
    rt::run_until_stalled();
    assert_eq!(*mirror.get(&page).unwrap(), json!(2));
    events.emit(&json!({"type":"updated","value":4,"origin":{"kind":"remote"}}));
    assert_eq!(*mirror.get(&page).unwrap(), json!(4));
    events.emit(&json!({"type":"pages.deleted","pages":[page]}));
    assert!(mirror.get(&page).is_none());
}

/// `page-mirror.ts:180-194`: the load's `.then` stores the read on turn T,
/// `.finally(() => inFlight.delete(key))` runs on T+1, and a caller's
/// reaction to the load runs on T+4. Measured in Node with the same counter:
/// `Promise.resolve(1).then(store).finally(clean).then(react)` gives store
/// 1, clean 2, react 5.
#[test]
fn a_page_load_settles_after_its_finally_as_typescript_chains_it() {
    use epdf_kernel::page_mirror::{Affected, PageMirror, PageMirrorSpec};
    use std::cell::Cell;
    rt::reset();
    let turn = Rc::new(Cell::new(0u32));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..32 {
            rt::tick().await;
            count.set(count.get() + 1);
        }
    });
    let stored = Rc::new(Cell::new(u32::MAX));
    let (seen, at) = (stored.clone(), turn.clone());
    let q = DocumentChanges::new(
        Rc::new(|_, _| Promise::resolved(json!({}))),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let page = json!({"kind":"objectNumber","objectNumber":3});
    let mirror = PageMirror::new(
        PageMirrorSpec {
            name: "page".into(),
            load: Rc::new(|_, _, _| Promise::resolved(json!(1))),
            affected: Rc::new(|_| Affected::All),
            fold: None,
            predict: None,
            changed: Some(Rc::new(move |_| {
                seen.set(at.get());
                Ok(())
            })),
        },
        MirrorEnvironment::detached(EventHookSource::new(None), AbortSignal::new(), q),
    );
    let load = mirror.ensure_loaded(page.clone());
    let reacted = Rc::new(Cell::new(u32::MAX));
    let (r, at) = (reacted.clone(), turn.clone());
    let watched = load.clone();
    rt::spawn(async move {
        let _ = watched.await;
        r.set(at.get());
    });
    rt::run_until_stalled();
    assert!(load.peek().unwrap().is_ok());
    assert_eq!(reacted.get(), stored.get() + 4);
    assert!(!mirror.ensure_loaded(page).same(&load), "the finally forgot the load");
}

#[test]
fn pending_holds_groups_and_dependency_refusal_keep_staging_order() {
    rt::reset();
    let sent = Rc::new(std::cell::RefCell::new(Vec::new()));
    let calls = sent.clone();
    let (answer, reject) = Promise::pending();
    let q = DocumentChanges::new(
        Rc::new(move |change, _| {
            calls.borrow_mut().push(change);
            answer.clone()
        }),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let queue = q.for_plugin("test");
    let held = queue.hold(json!({"key":"typing"}), Some("text".into())).unwrap();
    held.set(vec![json!({"type":"annotations.create","objectNumber":42})], vec![]);
    assert!(sent.borrow().is_empty());
    let first = held.change().unwrap();
    let later=queue.stage(StagedChange::ops(json!({"key":"edit"}),vec![json!({"type":"annotations.update","ref":{"kind":"objectNumber","objectNumber":42}})])).unwrap();
    assert_eq!(sent.borrow().len(), 2);
    assert!(!held.open());
    reject.reject(Error::engine("Forbidden", "no"));
    rt::run_until_stalled();
    assert_eq!(first.result().peek().unwrap().unwrap_err().code(), Some("permission-denied"));
    assert_eq!(
        later.result().peek().unwrap().unwrap_err().member("details").unwrap()["reason"],
        "dependency-refused"
    );
    assert!(!queue.has_pending());
    let before = sent.borrow().len();
    let failed = queue.group(json!({"key":"group"}), || {
        queue.stage(StagedChange::ops(
            json!({"key":"part"}),
            vec![json!({"type":"metadata.set"})],
        ))?;
        Err::<(), _>(Error::new("stop"))
    });
    assert!(failed.is_err());
    assert_eq!(sent.borrow().len(), before);
    assert!(!queue.has_pending());
}

#[test]
fn full_load_replays_only_events_after_cursor_and_keeps_prediction_until_reload() {
    rt::reset();
    let events = EventHookSource::new(None);
    let (applied, resolve) = Promise::pending();
    let q = DocumentChanges::new(
        Rc::new(move |_, _| applied.clone()),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let (loaded, land) = Promise::pending();
    let mut spec = MirrorSpec::new(
        "value",
        json!(0),
        Rc::new(move |_, _| loaded.clone()),
        Rc::new(|_, event| {
            if event["reload"] == true {
                Ok(Fold::Reload)
            } else {
                Ok(Fold::Value(Rc::new(event["value"].clone())))
            }
        }),
    );
    spec.predict = Some(Rc::new(|_, op| Ok(Rc::new(op["value"].clone()))));
    let mirror = Mirror::new(
        spec,
        MirrorEnvironment::detached(events.clone(), AbortSignal::new(), q.clone()),
    );
    let c = q
        .for_plugin("test")
        .stage(StagedChange::ops(json!({"key":"x"}), vec![json!({"value":7})]))
        .unwrap();
    events.emit(&json!({"type":"metadata.changed","reload":true,"origin":{"tx":{"id":c.op_id(),"index":0,"count":1}}}));
    resolve.resolve(json!({}));
    rt::run_until_stalled();
    assert!(q.shown().is_empty());
    assert_eq!(*mirror.view(), json!(7));
    events.emit(&json!({"type":"metadata.changed","value":3,"origin":{"serverId":2}}));
    events.emit(&json!({"type":"metadata.changed","value":9,"origin":{"serverId":4}}));
    land.resolve(Loaded { value: json!(7), cursor: Some(json!(3)) });
    rt::run_until_stalled();
    assert_eq!(*mirror.get(), json!(9));
    assert_eq!(*mirror.view(), json!(9));
}

#[test]
fn instance_clock_cancels_pending_and_refuses_new_timers_after_close() {
    use epdf_kernel::clock::{PluginClock, TimerClock};
    rt::reset();
    let life = AbortSignal::new();
    let clock = PluginClock::new(Rc::new(TimerClock), life.clone());
    let calls = Rc::new(std::cell::Cell::new(0));
    let count = calls.clone();
    clock.after(10.0, move || count.set(count.get() + 1));
    assert_eq!(rt::pending_timers(), 1);
    life.abort(Error::new("closed"));
    assert_eq!(rt::pending_timers(), 0);
    clock.after(1.0, || panic!("closed"));
    rt::advance(20.0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn caller_cancel_keeps_its_code_and_detaches_listener_when_answer_arrives() {
    use epdf_kernel::cancellable::cancellable;
    rt::reset();
    let signal = AbortSignal::new();
    let (task, answer) = Promise::<Value>::pending();
    let call = cancellable("test", Some(signal.clone()), task);
    signal.abort(Error::new("caller stopped"));
    assert_eq!(call.peek().unwrap().unwrap_err().code(), Some("operation-cancelled"));
    answer.resolve(json!(9));
    rt::run_until_stalled();
    assert_eq!(call.peek().unwrap().unwrap_err().code(), Some("operation-cancelled"));
    assert_eq!(signal.listener_count(), 0);
}

#[test]
fn settings_instance_reports_once_continues_siblings_and_disposes_all_listeners() {
    use epdf_kernel::{
        scope::Scope,
        settings::{Settings, SettingsDeclaration},
    };
    rt::reset();
    let errors = Rc::new(std::cell::RefCell::new(vec![]));
    let reported = errors.clone();
    let report: epdf_kernel::store::Report =
        Rc::new(move |e| reported.borrow_mut().push(e.message.clone()));
    let settings = Settings::new(
        SettingsDeclaration { defaults: json!({"x":0}), registered: None, whole: vec![] },
        Rc::new(|| {}),
        report.clone(),
    );
    let scope = Scope::new(report);
    let instance = settings.for_instance(&scope);
    let calls = Rc::new(std::cell::Cell::new(0));
    let count = calls.clone();
    instance.events.on(|_| Err(Error::new("instance listener failed")));
    instance.events.on(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    settings.update_settings(&json!({"x":1}));
    assert_eq!(*errors.borrow(), vec!["instance listener failed"]);
    assert_eq!(calls.get(), 1);
    scope.dispose();
    rt::run_until_stalled();
    settings.update_settings(&json!({"x":2}));
    assert_eq!(calls.get(), 1);
    assert_eq!(errors.borrow().len(), 1);
}

/// A handle that records the facts each `call_with` carried.
struct RecordsFacts(Rc<std::cell::RefCell<Vec<(epdf_kernel::engine::CallFacts, String)>>>);

impl epdf_kernel::engine::DocumentHandle for RecordsFacts {
    fn id(&self) -> String {
        "a".into()
    }
    fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn security(&self) -> Rc<dyn epdf_kernel::engine::DocumentSecurity> {
        unimplemented!()
    }
    fn has(&self, _: &str) -> bool {
        true
    }
    fn call(
        &self,
        _: &str,
        _: Option<u32>,
        _: Vec<serde_json::Value>,
    ) -> epdf_kernel::engine::EngineCall<serde_json::Value> {
        epdf_kernel::Promise::resolved(serde_json::json!("plain"))
    }
    fn call_with(
        &self,
        facts: &epdf_kernel::engine::CallFacts,
        path: &str,
        _: Option<u32>,
        _: Vec<serde_json::Value>,
    ) -> epdf_kernel::engine::EngineCall<serde_json::Value> {
        self.0.borrow_mut().push((facts.clone(), path.to_string()));
        epdf_kernel::Promise::resolved(serde_json::json!("with")).with_abort(|_| {})
    }
    fn page_index(&self, _: u32) -> Result<u32, epdf_kernel::Error> {
        Ok(0)
    }
    fn subscribe(&self, _: Rc<dyn Fn(&serde_json::Value)>) -> epdf_kernel::Unsubscribe {
        epdf_kernel::Unsubscribe::noop()
    }
}

/// `doc.with(facts)` (Ruling13.16): the guarded handle hands the facts to the
/// handle under the same guard as `call`, and a handle that does not rank
/// work answers `call_with` as `call`.
#[test]
fn call_facts_reach_the_handle_through_the_guard() {
    use epdf_kernel::engine::{CallFacts, CallPriority, DocumentHandle};
    use epdf_kernel::guarded::{GuardedLifetime, guard_handle};
    use epdf_kernel::signal::AbortSignal;

    epdf_kernel::rt::reset();
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let lifetime = GuardedLifetime { signal: AbortSignal::new(), instance_id: "a#1".into() };
    let guarded = guard_handle(Rc::new(RecordsFacts(seen.clone())), lifetime.clone(), "render");
    let facts = CallFacts { priority: Some(CallPriority::High), view: Some("stage".into()) };
    let call = guarded.call_with(&facts, "page.render.image", Some(11), Vec::new());
    assert!(call.has_abort(), "guarded like call");
    epdf_kernel::rt::run_until_stalled();
    assert_eq!(call.peek().unwrap().unwrap(), serde_json::json!("with"));
    assert_eq!(*seen.borrow(), vec![(facts.clone(), "page.render.image".to_string())]);
    assert_eq!(
        serde_json::to_value(&facts).unwrap(),
        serde_json::json!({ "priority": "high", "view": "stage" })
    );

    // After close the guard refuses it as it refuses `call`.
    lifetime.signal.abort(epdf_kernel::Error::new("closed"));
    let refused =
        guarded.call_with(&CallFacts::default(), "page.render.image", Some(11), Vec::new());
    epdf_kernel::rt::run_until_stalled();
    assert_eq!(refused.peek().unwrap().unwrap_err().plugin_code(), Some("instance-closed"));

    // The default: a handle that ignores facts answers as `call`.
    struct Plain;
    impl DocumentHandle for Plain {
        fn id(&self) -> String {
            "a".into()
        }
        fn capabilities(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn security(&self) -> Rc<dyn epdf_kernel::engine::DocumentSecurity> {
            unimplemented!()
        }
        fn has(&self, _: &str) -> bool {
            true
        }
        fn call(
            &self,
            path: &str,
            _: Option<u32>,
            _: Vec<serde_json::Value>,
        ) -> epdf_kernel::engine::EngineCall<serde_json::Value> {
            epdf_kernel::Promise::resolved(serde_json::json!(path))
        }
        fn page_index(&self, _: u32) -> Result<u32, epdf_kernel::Error> {
            Ok(0)
        }
        fn subscribe(&self, _: Rc<dyn Fn(&serde_json::Value)>) -> epdf_kernel::Unsubscribe {
            epdf_kernel::Unsubscribe::noop()
        }
    }
    let plain = Plain.call_with(&facts, "pages.list", None, Vec::new());
    assert_eq!(plain.peek().unwrap().unwrap(), serde_json::json!("pages.list"));
}

#[test]
fn a_string_load_cursor_skips_queued_events_at_or_below_it_as_javascript_compares() {
    rt::reset();
    let events = EventHookSource::new(None);
    let q = DocumentChanges::new(
        Rc::new(|_, _| Promise::resolved(json!({}))),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let (loaded, land) = Promise::pending();
    let spec = MirrorSpec::new(
        "value",
        json!([]),
        Rc::new(move |_, _| loaded.clone()),
        Rc::new(|value, event| {
            let mut list = value.as_array().cloned().unwrap_or_default();
            list.push(event["value"].clone());
            Ok(Fold::Value(Rc::new(Value::Array(list))))
        }),
    );
    let mirror = Mirror::new(
        spec,
        MirrorEnvironment::detached(events.clone(), AbortSignal::new(), q.clone()),
    );
    let _load = mirror.refresh();
    rt::run_until_stalled();
    // Queued while the load runs; the load's cursor is the string "2".
    events.emit(&json!({"type":"x","value":"equal","origin":{"serverId":"2"}}));
    events.emit(&json!({"type":"x","value":"lower","origin":{"serverId":"1"}}));
    // "10" <= "2" as strings: already in the load.
    events.emit(&json!({"type":"x","value":"string-ten","origin":{"serverId":"10"}}));
    events.emit(&json!({"type":"x","value":"higher","origin":{"serverId":"3"}}));
    // A number against the string compares as numbers: 10 > 2.
    events.emit(&json!({"type":"x","value":"number-ten","origin":{"serverId":10}}));
    events.emit(&json!({"type":"x","value":"no-id","origin":{}}));
    land.resolve(Loaded { value: json!([]), cursor: Some(json!("2")) });
    rt::run_until_stalled();
    assert_eq!(*mirror.get(), json!(["higher", "number-ten", "no-id"]));
}

/// `objectNumbers.onLost` (Ruling13.17 onLost): the guarded handle passes the
/// listener to the handle, the handle's losses reach it until it
/// unsubscribes, and a handle that never loses numbers keeps the silent
/// default.
#[test]
fn lost_object_numbers_reach_a_listener_through_the_guard() {
    use epdf_kernel::engine::{DocumentHandle, ObjectNumbersLost, ObjectNumbersLostReason};
    use epdf_kernel::guarded::{GuardedLifetime, guard_handle};
    use std::cell::RefCell;

    type Listeners = Rc<RefCell<Vec<Option<Rc<dyn Fn(&ObjectNumbersLost)>>>>>;
    struct Losing(Listeners);
    impl Losing {
        fn lose(&self, lost: &ObjectNumbersLost) {
            let listeners: Vec<_> = self.0.borrow().iter().flatten().cloned().collect();
            for listener in listeners {
                listener(lost);
            }
        }
    }
    impl DocumentHandle for Losing {
        fn id(&self) -> String {
            "a".into()
        }
        fn capabilities(&self) -> Value {
            json!({})
        }
        fn security(&self) -> Rc<dyn epdf_kernel::engine::DocumentSecurity> {
            unimplemented!()
        }
        fn has(&self, _: &str) -> bool {
            true
        }
        fn call(
            &self,
            _: &str,
            _: Option<u32>,
            _: Vec<Value>,
        ) -> epdf_kernel::engine::EngineCall<Value> {
            Promise::resolved(json!({}))
        }
        fn page_index(&self, _: u32) -> Result<u32, epdf_kernel::Error> {
            Ok(0)
        }
        fn subscribe(&self, _: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
            epdf_kernel::Unsubscribe::noop()
        }
        fn on_lost_object_numbers(
            &self,
            listener: Rc<dyn Fn(&ObjectNumbersLost)>,
        ) -> epdf_kernel::Unsubscribe {
            let index = self.0.borrow().len();
            self.0.borrow_mut().push(Some(listener));
            let listeners = self.0.clone();
            epdf_kernel::Unsubscribe::new(move || listeners.borrow_mut()[index] = None)
        }
    }

    rt::reset();
    let listeners: Listeners = Rc::new(RefCell::new(Vec::new()));
    let handle = Rc::new(Losing(listeners.clone()));
    let lifetime = GuardedLifetime { signal: AbortSignal::new(), instance_id: "a#1".into() };
    let guarded = guard_handle(handle.clone(), lifetime, "annotation");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let off =
        guarded.on_lost_object_numbers(Rc::new(move |lost| sink.borrow_mut().push(lost.clone())));
    let lost =
        ObjectNumbersLost { numbers: vec![10, 11], reason: ObjectNumbersLostReason::Versioned };
    handle.lose(&lost);
    assert_eq!(*seen.borrow(), vec![lost.clone()]);
    assert_eq!(
        serde_json::to_value(&lost).unwrap(),
        json!({ "numbers": [10, 11], "reason": "versioned" })
    );
    off.call();
    handle.lose(&lost);
    assert_eq!(seen.borrow().len(), 1, "unsubscribed");

    // The default never calls the listener.
    struct Silent;
    impl DocumentHandle for Silent {
        fn id(&self) -> String {
            "a".into()
        }
        fn capabilities(&self) -> Value {
            json!({})
        }
        fn security(&self) -> Rc<dyn epdf_kernel::engine::DocumentSecurity> {
            unimplemented!()
        }
        fn has(&self, _: &str) -> bool {
            true
        }
        fn call(
            &self,
            _: &str,
            _: Option<u32>,
            _: Vec<Value>,
        ) -> epdf_kernel::engine::EngineCall<Value> {
            Promise::resolved(json!({}))
        }
        fn page_index(&self, _: u32) -> Result<u32, epdf_kernel::Error> {
            Ok(0)
        }
        fn subscribe(&self, _: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
            epdf_kernel::Unsubscribe::noop()
        }
    }
    Silent.on_lost_object_numbers(Rc::new(|_| panic!("never lost"))).call();
}

/// L-N3: a cancellable call lets go of its signal once it settles, so a
/// signal reused across calls (the annotation plugin's
/// `renderAppearances`) does not collect listeners.
#[test]
fn a_cancellable_call_releases_its_signal_once_it_succeeds() {
    use epdf_kernel::cancellable::cancellable;
    rt::reset();
    let signal = AbortSignal::new();
    for round in 0..3 {
        let (task, answer) = Promise::<Value>::pending();
        let call = cancellable("annotation", Some(signal.clone()), task);
        assert_eq!(signal.listener_count(), 1, "round {round}: listening while pending");
        answer.resolve(json!([]));
        rt::run_until_stalled();
        assert_eq!(call.peek().unwrap().unwrap(), json!([]));
        assert_eq!(signal.listener_count(), 0, "round {round}: released on success");
    }
}

#[test]
fn a_mirror_cell_shows_its_value_through_to_json_only_when_dumped() {
    use epdf_kernel::store::Store;
    use std::cell::Cell;
    rt::reset();
    let q = DocumentChanges::new(
        Rc::new(|_, _| Promise::resolved(json!({"applied":true}))),
        Rc::new(|| {}),
        Rc::new(|e| panic!("{e}")),
    );
    let events = EventHookSource::new(None);
    let store = Store::new(Rc::new(|_| {}));
    let mut env = MirrorEnvironment::detached(events.clone(), AbortSignal::new(), q);
    let leases = store.clone();
    env.cell = Rc::new(move |name, initial| {
        leases.lease(
            name,
            Rc::new(|_, _| None),
            Rc::new(initial),
            Rc::new(epdf_kernel::mirror::cell_json),
            None,
        )
    });
    let shown = Rc::new(Cell::new(0));
    let mut spec = MirrorSpec::new(
        "tokens",
        json!({"token":0}),
        Rc::new(|_, _| Promise::resolved(Loaded { value: json!({"token":1}), cursor: None })),
        Rc::new(|_, event| Ok(Fold::Value(Rc::new(json!({"token":event["value"]}))))),
    );
    let calls = shown.clone();
    spec.to_json = Some(Rc::new(move |value| {
        calls.set(calls.get() + 1);
        json!({"shown":value["token"]})
    }));
    let mirror = Mirror::new(spec, env);
    assert_eq!(
        store.slice_json("mirror/tokens").unwrap(),
        json!({"value":{"shown":0},"status":"idle"})
    );
    mirror.start();
    rt::run_until_stalled();
    events.emit(&json!({"type":"metadata.changed","value":2}));
    let writes = shown.get();
    assert_eq!(*mirror.get(), json!({"token":2}));
    assert_eq!(shown.get(), writes, "a write never serialises");
    assert_eq!(
        store.slice_json("mirror/tokens").unwrap(),
        json!({"value":{"shown":2},"status":"ready"})
    );
    assert_eq!(shown.get(), writes + 1);
}
