//! The typed API beyond what the vectors print, against
//! `packages/plugin/outline/src/{controller,model}.ts` and
//! `sync/tree.ts`: a confirmed event updates the mirror and fires
//! `on_changed` with its origin before the write's promise settles, a write
//! shows before the engine answers and goes when it refuses, the optional
//! plugins may all be absent, and the reads and writes settle on the
//! TypeScript's turns.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_outline::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::scenario::install_case_entropy;
use serde_json::{Value, json};

fn r(n: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

fn item(n: u32, parent: Option<u32>, title: &str) -> Value {
    json!({
        "ref": r(n), "parent": parent.map(r), "title": title, "target": null, "action": null,
        "open": false, "color": null, "bold": false, "italic": false,
    })
}

fn outline() -> Value {
    json!({ "items": [item(10, None, "A"), item(11, Some(10), "B"), item(12, None, "C")], "incomplete": false })
}

fn local(op_id: &str) -> Value {
    json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
            "tx": { "id": op_id, "index": 0, "count": 1 } })
}

/// The n-th change's id under the case entropy.
fn op(n: u32) -> String {
    format!("00000000-0000-7000-8000-{n:012x}")
}

fn meta(op_id: &str, changed: &[u32]) -> Value {
    json!({ "affectedPages": [], "cacheDelta": null, "opId": op_id, "undoable": true,
            "changed": changed.iter().map(|n| r(*n)).collect::<Vec<_>>() })
}

/// The engine's answer to change `n`: its event, then its result.
fn answer(n: u32, item: Value, event: Value) -> Value {
    let id = op(n);
    let mut event = event;
    event["origin"] = local(&id);
    json!({
        "path": "apply", "times": 1,
        "value": { "items": [item], "meta": { "affectedPages": [], "cacheDelta": null, "opId": id, "undoable": true } },
        "events": [event],
    })
}

fn created(n: u32) -> Value {
    let id = op(n);
    let order = json!({ "parent": null, "children": [r(10), r(12), r(500)] });
    answer(
        n,
        json!({ "type": "outline.create", "item": item(500, None, "N"), "order": order, "meta": meta(&id, &[500]) }),
        json!({ "type": "outline.created", "item": item(500, None, "N"), "order": order, "meta": meta(&id, &[500]) }),
    )
}

fn updated(n: u32) -> Value {
    let id = op(n);
    answer(
        n,
        json!({ "type": "outline.update", "item": item(10, None, "X"), "meta": meta(&id, &[10]) }),
        json!({ "type": "outline.updated", "item": item(10, None, "X"), "meta": meta(&id, &[10]) }),
    )
}

fn deleted(n: u32) -> Value {
    let id = op(n);
    answer(
        n,
        json!({ "type": "outline.delete", "meta": meta(&id, &[10, 11]) }),
        json!({ "type": "outline.deleted", "deleted": [r(10), r(11)], "meta": meta(&id, &[10, 11]) }),
    )
}

fn read() -> Value {
    json!({ "path": "outline.get", "value": outline() })
}

struct Rig {
    _kernel: Kernel,
    fake: Rc<FakeEngine>,
    outline: OutlineCapability,
    _entropy: plugin_conformance::scenario::CaseEntropy,
}

impl Rig {
    fn with(responses: Vec<Value>, allows: Value, numbers: Value) -> Rig {
        rt::reset();
        let entropy = install_case_entropy();
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [],
            "security": { "allows": allows },
            "objectNumbers": numbers,
            "responses": responses,
        } } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            vec![outline_plugin()],
            KernelOptions::default(),
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let outline = kernel.capability(&OUTLINE, Some("a")).expect("outline");
        Rig { _kernel: kernel, fake, outline, _entropy: entropy }
    }

    fn new(responses: Vec<Value>) -> Rig {
        Rig::with(responses, json!("*"), json!([500, 501]))
    }

    fn loaded(responses: Vec<Value>) -> Rig {
        let rig = Rig::new(responses);
        settled(&rig.outline.ensure_loaded(none())).expect("settled").expect("loaded");
        rig
    }

    fn calls(&self, path: &str) -> usize {
        self.fake.log_since(0).iter().filter(|entry| entry["path"] == path).count()
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn none() -> OperationOptions {
    OperationOptions::default()
}

fn numbers(list: &[Value]) -> Vec<u64> {
    list.iter().map(|i| i["ref"]["objectNumber"].as_u64().unwrap()).collect()
}

fn titled(title: &str) -> Value {
    json!({ "title": title })
}

#[test]
fn the_mirror_and_on_changed_lead_the_writes_promise() {
    let rig = Rig::loaded(vec![read(), created(1)]);
    let order: Rc<RefCell<Vec<String>>> = Rc::default();
    let (log, cap) = (order.clone(), rig.outline.clone());
    let _off = rig.outline.on_changed(move |event| {
        // The mirror already holds the confirmed item when the event fires.
        let listed = numbers(&cap.list());
        log.borrow_mut()
            .push(format!("changed {} {:?} {}", event.kind, listed, event.origin["tx"]["id"]));
        Ok(())
    });
    let promise = rig.outline.create(titled("N"), OutlineCreateOptions::default());
    let log = order.clone();
    promise.on_settle(move |result| log.borrow_mut().push(format!("settled {}", result.is_ok())));
    rt::run_until_stalled();
    assert_eq!(
        *order.borrow(),
        [format!("changed created [10, 11, 12, 500] \"{}\"", op(1)), "settled true".to_string()]
    );
    let answered = promise.peek().unwrap().unwrap();
    assert_eq!(answered["item"]["ref"], r(500));
}

#[test]
fn a_write_shows_before_the_answer_and_goes_when_the_engine_refuses() {
    let refused = json!({ "path": "apply", "times": 1, "error": { "name": "EngineError", "message": "no", "code": "Refused" } });
    let rig = Rig::loaded(vec![read(), refused]);
    let heard = Rc::new(Cell::new(0));
    let count = heard.clone();
    let _off = rig.outline.on_changed(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    let promise = rig.outline.update(r(10), titled("X"), none());
    // `update` stages synchronously: the prediction shows at once.
    assert_eq!(rig.outline.get(&r(10)).unwrap()["title"], "X");
    let error = settled(&promise).unwrap().unwrap_err();
    assert!(error.is_plugin_error(Some("operation-failed")));
    assert_eq!(rig.outline.get(&r(10)).unwrap()["title"], "A");
    assert_eq!(heard.get(), 0, "a refusal confirms nothing");
}

#[test]
fn no_optional_plugin_is_needed() {
    let rig = Rig::loaded(vec![read(), created(1)]);
    // No stage: no reading position and no default target.
    assert_eq!(rig.outline.get_current_item(), None);
    assert_eq!(rig.outline.get_current_view_target(), None);
    // No link, stage or actions: a target is handed back, an /A alone is none.
    assert_eq!(rig.outline.activate(&r(10)).unwrap().outcome(), "none");
    // No selection, no i18n: "Untitled".
    settled(&rig.outline.create(json!({}), OutlineCreateOptions::default())).unwrap().unwrap();
    let apply = rig.fake.log_since(0).into_iter().find(|e| e["path"] == "apply").unwrap();
    assert_eq!(apply["args"][0]["ops"][0]["data"], json!({ "title": "Untitled" }));
}

#[test]
fn refusals_come_before_the_engine() {
    let rig = Rig::with(vec![read()], json!(["doc.outline.read"]), json!([500]));
    settled(&rig.outline.ensure_loaded(none())).unwrap().unwrap();
    assert!(!rig.outline.can_edit());
    let error = settled(&rig.outline.delete(vec![r(10)], none())).unwrap().unwrap_err();
    assert!(error.is_plugin_error(Some("permission-denied")));

    let rig = Rig::loaded(vec![read()]);
    let signal = AbortSignal::new();
    signal.abort(Error::named("AbortError", "This operation was aborted"));
    let cancelled = rig
        .outline
        .create(titled("N"), OutlineCreateOptions { signal: Some(signal), select: false });
    assert!(settled(&cancelled).unwrap().unwrap_err().is_plugin_error(Some("operation-cancelled")));
    let unknown = rig.outline.update(r(99), titled("X"), none());
    assert!(settled(&unknown).unwrap().unwrap_err().is_plugin_error(Some("not-found")));
    assert_eq!(rig.calls("apply"), 0);
    assert_eq!(rig.calls("objectNumbers.reserve"), 0);
}

#[test]
fn a_ready_outline_resolves_ensure_loaded_at_once() {
    let rig = Rig::loaded(vec![read()]);
    // `mirror.ensureLoaded()` answers `Promise.resolve()` once ready, and
    // `ctx.cancellable` with no signal hands that promise on.
    assert!(rig.outline.ensure_loaded(none()).peek().is_some());
    assert_eq!(rig.calls("outline.get"), 1);
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

fn heard_on<E: 'static>(
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
    turn: &Rc<Cell<u32>>,
) -> (Rc<Cell<u32>>, epdf_kernel::Unsubscribe) {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    let off = subscribe(Box::new(move |_| {
        seen.set(turn.get());
        Ok(())
    }));
    (at, off)
}

/// The turns were measured on the TypeScript: the real kernel and outline
/// plugin over the conformance fake engine (`createFakeEngine`, whose
/// answers are already settled, with the case entropy), with the counter
/// above and `promise.then(() => at = turn)`, under `tsx
/// --conditions=development`. A fresh `ensureLoaded` reacts on 10 (its
/// `onResynced` on 5), a ready one on 1, `refresh` after a load on 10. A
/// `create` with a title and a held object number reacts on 5 (its
/// `onChanged` on 1, the confirmed item already listed); with no title and
/// no optional plugin ("Untitled") on 6; with an empty pool (one
/// `objectNumbers.reserve`) on 9. `update` reacts on 4 (`onChanged` on 0:
/// it stages synchronously and the fake publishes during the call),
/// `delete` on 4, `delete([])` on 1, a `create` whose signal already fired
/// rejects on 1, and an `update` the engine refuses rejects on 4.
#[test]
fn reads_and_writes_settle_on_the_typescripts_turns() {
    let rig = Rig::new(vec![read()]);
    let turn = turn_counter();
    let (resynced, _off) = heard_on(|l| rig.outline.on_resynced(l), &turn);
    let fresh = reacts_on(rig.outline.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!((fresh.get(), resynced.get()), (10, 5));
    let turn = turn_counter();
    let ready = reacts_on(rig.outline.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(ready.get(), 1);
    let turn = turn_counter();
    let refreshed = reacts_on(rig.outline.refresh(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(refreshed.get(), 10);

    let rig = Rig::loaded(vec![read(), created(1)]);
    let turn = turn_counter();
    let (changed, _off) = heard_on(|l| rig.outline.on_changed(l), &turn);
    let made = reacts_on(rig.outline.create(titled("N"), OutlineCreateOptions::default()), &turn);
    rt::run_until_stalled();
    assert_eq!((made.get(), changed.get()), (5, 1));

    let rig = Rig::loaded(vec![read(), created(1)]);
    let turn = turn_counter();
    let untitled = reacts_on(rig.outline.create(json!({}), OutlineCreateOptions::default()), &turn);
    rt::run_until_stalled();
    assert_eq!(untitled.get(), 6);

    let reserve =
        json!({ "path": "objectNumbers.reserve", "args": [1], "value": { "$undefined": true } });
    let rig = Rig::with(vec![read(), reserve, created(1)], json!("*"), json!([]));
    settled(&rig.outline.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let reserving =
        reacts_on(rig.outline.create(titled("N"), OutlineCreateOptions::default()), &turn);
    rt::run_until_stalled();
    assert_eq!(reserving.get(), 9);
    assert_eq!(rig.calls("objectNumbers.reserve"), 1);

    let rig = Rig::loaded(vec![read(), updated(1)]);
    let turn = turn_counter();
    let (changed, _off) = heard_on(|l| rig.outline.on_changed(l), &turn);
    let written = reacts_on(rig.outline.update(r(10), titled("X"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!((written.get(), changed.get()), (4, 0));

    let rig = Rig::loaded(vec![read(), deleted(1)]);
    let turn = turn_counter();
    let gone = reacts_on(rig.outline.delete(vec![r(10)], none()), &turn);
    rt::run_until_stalled();
    assert_eq!(gone.get(), 4);
    let turn = turn_counter();
    let nothing = reacts_on(rig.outline.delete(Vec::new(), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(nothing.get(), 1);

    let rig = Rig::loaded(vec![read()]);
    let signal = AbortSignal::new();
    signal.abort(Error::named("AbortError", "This operation was aborted"));
    let turn = turn_counter();
    let options = OutlineCreateOptions { signal: Some(signal), select: false };
    let cancelled = reacts_on(rig.outline.create(titled("N"), options), &turn);
    rt::run_until_stalled();
    assert_eq!(cancelled.get(), 1);

    let refused = json!({ "path": "apply", "times": 1, "error": { "name": "EngineError", "message": "no", "code": "Refused" } });
    let rig = Rig::loaded(vec![read(), refused]);
    let turn = turn_counter();
    let rejected = reacts_on(rig.outline.update(r(10), titled("X"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(rejected.get(), 4);
}
