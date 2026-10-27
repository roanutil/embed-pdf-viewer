//! The typed API beyond what the vectors print, against
//! `packages/plugin/metadata/src/{controller,model}.ts` and the kernel
//! mirrors it reads through (`packages/core/main/src/mirror.ts`): opening
//! reads nothing, a refresh during a read in flight queues one reread, the
//! events carry the exact previous value, changed keys and origin, the
//! verbs refuse before the engine, and loads settle on the TypeScript's
//! turns.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_metadata::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn md(title: &str) -> Value {
    json!({
        "title": title, "author": null, "subject": null, "keywords": null, "producer": null,
        "creator": null, "createdAt": null, "modifiedAt": null, "trapped": "unknown",
    })
}

fn local() -> Value {
    json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null })
}

fn remote() -> Value {
    json!({ "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null })
}

fn meta() -> Value {
    json!({ "affectedPages": [], "cacheDelta": null, "opId": "metadata-write", "undoable": true })
}

fn fact(metadata: Value, origin: Value) -> Value {
    json!({ "type": "metadata.updated", "metadata": metadata, "meta": meta(), "origin": origin })
}

fn custom_fact(custom: Value, origin: Value) -> Value {
    json!({ "type": "metadata.customUpdated", "custom": custom, "meta": meta(), "origin": origin })
}

/// The engine answers both reads at once, and a standard update with its
/// confirmed event.
fn answers() -> Vec<Value> {
    vec![
        json!({ "path": "metadata.get", "args": [], "value": md("Seed") }),
        json!({ "path": "metadata.custom.get", "args": [], "value": { "A": "1" } }),
        json!({
            "path": "metadata.update",
            "args": [{ "title": "New" }],
            "value": { "metadata": md("New"), "meta": meta() },
            "events": [fact(md("New"), local())],
        }),
    ]
}

struct Rig {
    _kernel: Kernel,
    fake: Rc<FakeEngine>,
    metadata: MetadataCapability,
    reports: Rc<RefCell<Vec<String>>>,
}

impl Rig {
    fn new(responses: Vec<Value>, allows: Value) -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<String>>> = Rc::default();
        let heard = reports.clone();
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [],
            "security": { "allows": allows },
            "responses": responses,
        } } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            vec![metadata_plugin()],
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| {
                    heard.borrow_mut().push(error.message.clone())
                })),
                ..KernelOptions::default()
            },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let metadata = kernel.capability(&METADATA, Some("a")).expect("metadata");
        Rig { _kernel: kernel, fake, metadata, reports }
    }

    fn calls(&self, path: &str) -> usize {
        self.fake.log_since(0).iter().filter(|entry| entry["path"] == path).count()
    }

    fn emit(&self, event: Value) {
        self.fake.emit("a", &event).expect("emit");
        rt::run_until_stalled();
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn none() -> OperationOptions {
    OperationOptions::default()
}

fn aborted() -> OperationOptions {
    let signal = AbortSignal::new();
    signal.abort_value(Error::abort(Some(json!("stop"))), json!("stop"));
    OperationOptions { signal: Some(signal) }
}

#[test]
fn opening_reads_nothing_and_both_halves_start_null_and_idle() {
    let rig = Rig::new(answers(), json!("*"));
    assert_eq!(rig.calls("metadata.get"), 0, "no eager standard read");
    assert_eq!(rig.calls("metadata.custom.get"), 0, "no eager custom read");
    assert_eq!(rig.metadata.get_snapshot(), None);
    assert_eq!(rig.metadata.get_status(), ResourceStatus::Idle);
    assert_eq!(rig.metadata.custom().get_snapshot(), None);
    assert_eq!(rig.metadata.custom().get_status(), ResourceStatus::Idle);
    // `canUpdate` answers without loading.
    assert!(rig.metadata.can_update());
    assert_eq!(rig.calls("metadata.get"), 0);

    settled(&rig.metadata.ensure_loaded(none())).expect("settled").expect("loaded");
    assert_eq!(rig.calls("metadata.get"), 1);
    assert_eq!(rig.calls("metadata.custom.get"), 1);
    assert_eq!(rig.metadata.get_snapshot().as_deref(), Some(&md("Seed")));
    assert_eq!(rig.metadata.custom().get_snapshot().as_deref(), Some(&json!({ "A": "1" })));
    assert_eq!(rig.metadata.get_status(), ResourceStatus::Ready);
    assert_eq!(rig.metadata.custom().get_status(), ResourceStatus::Ready);
    // Reference-stable until it changes.
    assert!(Rc::ptr_eq(
        &rig.metadata.get_snapshot().unwrap(),
        &rig.metadata.get_snapshot().unwrap()
    ));
}

#[test]
fn a_refresh_during_a_read_joins_it_and_queues_one_reread() {
    let rig = Rig::new(answers(), json!("*"));
    let loading = rig.metadata.ensure_loaded(none());
    assert_eq!(rig.metadata.get_status(), ResourceStatus::Loading);
    let first = rig.metadata.refresh(none());
    let second = rig.metadata.refresh(none());
    assert!(first.same(&second), "both refreshes join the running read");
    settled(&loading).expect("settled").expect("loaded");
    assert!(settled(&first).expect("settled").is_ok());
    assert_eq!(rig.calls("metadata.get"), 2, "one read, then one queued reread");
    assert_eq!(rig.calls("metadata.custom.get"), 1, "refresh is the standard mirror's only");
}

#[test]
fn events_carry_the_exact_previous_value_changed_keys_and_origin() {
    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let standard: Rc<RefCell<Vec<Value>>> = Rc::default();
    let custom: Rc<RefCell<Vec<Value>>> = Rc::default();
    let (heard, heard_custom) = (standard.clone(), custom.clone());
    let _off = rig.metadata.on_updated(move |event| {
        heard.borrow_mut().push(serde_json::to_value(event).unwrap());
        Ok(())
    });
    let _off_custom = rig.metadata.custom().on_updated(move |event| {
        heard_custom.borrow_mut().push(serde_json::to_value(event).unwrap());
        Ok(())
    });

    rig.emit(fact(md("Remote"), remote()));
    assert_eq!(
        *standard.borrow(),
        [json!({
            "metadata": md("Remote"),
            "previous": md("Seed"),
            "changedKeys": ["title"],
            "origin": remote(),
        })]
    );

    // A removed key, an added key and an emptied key are each a change; a
    // key whose value stayed is not. Previous keys first, then new ones.
    rig.emit(custom_fact(json!({ "B": "2", "A": "1" }), local()));
    rig.emit(custom_fact(json!({ "B": "", "C": "3" }), remote()));
    let seen = custom.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[1],
        json!({
            "custom": { "B": "", "C": "3" },
            "previous": { "B": "2", "A": "1" },
            "changedKeys": ["B", "A", "C"],
            "origin": remote(),
        })
    );
    assert_eq!(
        rig.metadata.custom().get_snapshot().as_deref(),
        Some(&json!({ "B": "", "C": "3" }))
    );
}

#[test]
fn key_order_alone_changes_no_key() {
    // `changedKeys` over the same entries in another order is empty. The
    // confirmed event is still a new value, so the TypeScript fires
    // `onUpdated` with no changed key (`on-updated-reports-each-confirmed-change`).
    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    rig.emit(custom_fact(json!({ "A": "1", "B": "2" }), local()));
    let keys: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let heard = keys.clone();
    let _off = rig.metadata.custom().on_updated(move |event| {
        heard.borrow_mut().push(event.changed_keys.clone());
        Ok(())
    });
    rig.emit(custom_fact(json!({ "B": "2", "A": "1" }), local()));
    assert_eq!(*keys.borrow(), [Vec::<String>::new()]);
}

#[test]
fn an_event_without_an_origin_or_with_a_null_value_fires_nothing() {
    let rig = Rig::new(answers(), json!("*"));
    let fired = Rc::new(Cell::new(0));
    let count = fired.clone();
    let _off = rig.metadata.on_updated(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    // Folded before any load: the value is set and `onUpdated` fires.
    rig.emit(fact(md("Early"), local()));
    assert_eq!(fired.get(), 1);
    // No `origin`: the value changes, no event fires.
    rig.emit(json!({ "type": "metadata.updated", "metadata": md("Quiet"), "meta": meta() }));
    assert_eq!(fired.get(), 1);
    assert_eq!(rig.metadata.get_snapshot().as_deref(), Some(&md("Quiet")));
    // A null value fires nothing.
    rig.emit(json!({ "type": "metadata.updated", "metadata": null, "origin": local() }));
    assert_eq!(fired.get(), 1);
    assert_eq!(rig.metadata.get_snapshot(), None);
    assert!(rig.reports.borrow().is_empty(), "{:?}", rig.reports.borrow());
}

#[test]
fn writes_refuse_before_the_engine() {
    let rig = Rig::new(answers(), json!(["doc.render"]));
    assert!(!rig.metadata.can_update());
    for (promise, verb) in [
        (rig.metadata.update(json!({ "title": "x" }), none()), "metadata.update"),
        (rig.metadata.custom().update(json!({ "A": null }), none()), "metadata.custom.update"),
    ] {
        let error = settled(&promise).expect("settled").unwrap_err();
        assert_eq!(error.plugin_code(), Some(code::PERMISSION_DENIED));
        assert_eq!(error.message, format!("{verb} requires doc.metadata.modify"));
    }

    // Allowed, but the caller's signal already fired: cancelled, and the
    // engine never sees the write.
    let rig = Rig::new(answers(), json!("*"));
    let error = settled(&rig.metadata.update(json!({ "title": "New" }), aborted()))
        .expect("settled")
        .unwrap_err();
    assert_eq!(error.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(error.message, "metadata.update was cancelled");
    let error = settled(&rig.metadata.custom().update(json!({ "A": null }), aborted()))
        .expect("settled")
        .unwrap_err();
    assert_eq!(error.message, "metadata.custom.update was cancelled");
    assert_eq!(rig.calls("metadata.update"), 0);
    assert_eq!(rig.calls("metadata.custom.update"), 0);
}

#[test]
fn an_update_resolves_with_the_engine_answer_after_the_confirmed_event() {
    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let snapshot_when_heard: Rc<RefCell<Option<Value>>> = Rc::default();
    let (heard, metadata) = (snapshot_when_heard.clone(), rig.metadata.clone());
    let _off = rig.metadata.on_updated(move |_| {
        *heard.borrow_mut() = metadata.get_snapshot().map(|s| (*s).clone());
        Ok(())
    });
    let result = settled(&rig.metadata.update(json!({ "title": "New" }), none()))
        .expect("settled")
        .expect("written");
    assert_eq!(result, json!({ "metadata": md("New") }));
    assert_eq!(*snapshot_when_heard.borrow(), Some(md("New")));
}

/// A counter that moves once per turn of the queue, as
/// `(async () => { for (;;) { await undefined; turn++ } })()` does.
fn turn_counter() -> Rc<Cell<u32>> {
    let turn = Rc::new(Cell::new(0));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..64 {
            rt::tick().await;
            count.set(count.get() + 1);
        }
    });
    turn
}

/// `promise.then(() => at = turn, () => at = turn)`: the turn its reaction
/// runs on.
fn reacts_on<T: Clone + 'static>(promise: Promise<T>, turn: &Rc<Cell<u32>>) -> Rc<Cell<u32>> {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    rt::spawn(async move {
        let _ = promise.await;
        seen.set(turn.get());
    });
    at
}

/// The turns were measured on the TypeScript: the real kernel and metadata
/// plugin over the conformance fake engine (`createFakeEngine`, whose
/// answers are already settled), with the counter above and
/// `promise.then(() => at = turn)`, run under `tsx --conditions=development`.
/// A fresh `ensureLoaded` reacts on 12 (both `onResynced` fire on 5), 13
/// with a signal; a loaded one on 3; `refresh` after a load on 10 (its
/// `onResynced` on 5), `custom.refresh` on 10; `update` on 5 (the fake
/// emits its confirmed event during the call, so `onUpdated` fires on 0);
/// an `update` whose signal already fired rejects on 1. With `ensureLoaded`
/// then two `refresh` calls, the refreshes react on 16 and `ensureLoaded`
/// on 18, after one queued reread.
#[test]
fn loads_and_writes_settle_on_the_typescripts_turns() {
    let rig = Rig::new(answers(), json!("*"));
    let turn = turn_counter();
    let (resynced, custom_resynced) = (Rc::new(Cell::new(u32::MAX)), Rc::new(Cell::new(u32::MAX)));
    let (a, b, ta, tb) = (resynced.clone(), custom_resynced.clone(), turn.clone(), turn.clone());
    let _off = rig.metadata.on_resynced(move |_| {
        a.set(ta.get());
        Ok(())
    });
    let _off_custom = rig.metadata.custom().on_resynced(move |_| {
        b.set(tb.get());
        Ok(())
    });
    let fresh = reacts_on(rig.metadata.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!((fresh.get(), resynced.get(), custom_resynced.get()), (12, 5, 5));

    let rig = Rig::new(answers(), json!("*"));
    let turn = turn_counter();
    let signal = OperationOptions { signal: Some(AbortSignal::new()) };
    let signalled = reacts_on(rig.metadata.ensure_loaded(signal), &turn);
    rt::run_until_stalled();
    assert_eq!(signalled.get(), 13);

    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let cached = reacts_on(rig.metadata.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(cached.get(), 3);

    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let resynced = Rc::new(Cell::new(u32::MAX));
    let (seen, counter) = (resynced.clone(), turn.clone());
    let _off = rig.metadata.on_resynced(move |_| {
        seen.set(counter.get());
        Ok(())
    });
    let refreshed = reacts_on(rig.metadata.refresh(none()), &turn);
    rt::run_until_stalled();
    assert_eq!((refreshed.get(), resynced.get()), (10, 5));

    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let refreshed = reacts_on(rig.metadata.custom().refresh(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(refreshed.get(), 10);

    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let updated = Rc::new(Cell::new(u32::MAX));
    let (seen, counter) = (updated.clone(), turn.clone());
    let _off = rig.metadata.on_updated(move |_| {
        seen.set(counter.get());
        Ok(())
    });
    let written = reacts_on(rig.metadata.update(json!({ "title": "New" }), none()), &turn);
    rt::run_until_stalled();
    assert_eq!((written.get(), updated.get()), (5, 0));

    let rig = Rig::new(answers(), json!("*"));
    settled(&rig.metadata.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let refused = reacts_on(rig.metadata.update(json!({ "title": "New" }), aborted()), &turn);
    rt::run_until_stalled();
    assert_eq!(refused.get(), 1);

    let rig = Rig::new(answers(), json!("*"));
    let turn = turn_counter();
    let loading = rig.metadata.ensure_loaded(none());
    let first = rig.metadata.refresh(none());
    let _second = rig.metadata.refresh(none());
    let (loaded, refreshed) = (reacts_on(loading, &turn), reacts_on(first, &turn));
    rt::run_until_stalled();
    assert_eq!((refreshed.get(), loaded.get()), (16, 18));
    assert_eq!(rig.calls("metadata.get"), 2);
}
