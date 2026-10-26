//! The behaviour Task 13.23 pins beyond the printed traces, against
//! `packages/plugin/redaction/src/{model,read/pending,write/apply,write/marks,services/store}.ts`
//! and the kernel's serial queue and `cancellable`
//! (`packages/core/main/src/{serial-queue,cancellable}.ts`): the turns an
//! apply settles on, per-page apply scopes that leave the other page's marks
//! (and their memoized list) alone, the pending view's reference
//! stability, a close or a refusal mid-apply, cancels before and during an
//! apply, `unmark`'s separate skipped, failed and rejected outcomes, the
//! report a throwing listener reaches, the overlay settings as the redact
//! tool's defaults, and the defensive missing-service branch, which no
//! vector reaches (the TypeScript fake always has a redaction service).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_redaction::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn pr(n: u32) -> Value {
    PageRef::new(n).to_json()
}

fn obj(n: u32, page: u32) -> Value {
    json!({ "kind": "objectNumber", "page": pr(page), "objectNumber": n })
}

fn layout(pon: u32, index: u32, width: f64, height: f64) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": width, "height": height });
    json!({
        "index": index,
        "ref": pr(pon),
        "label": null,
        "size": { "width": width, "height": height },
        "rotation": 0,
        "userUnit": 1,
        "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": width, "top": height },
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
    })
}

fn base(subtype: &str, n: u32, page: u32, rect: Value) -> serde_json::Map<String, Value> {
    let record = json!({
        "subtype": subtype,
        "ref": obj(n, page),
        "page": pr(page),
        "hasAppearance": true, "appearanceState": null, "nm": null,
        "rect": rect,
        "contents": null, "subject": null, "blendMode": "normal",
        "invisible": false, "hidden": false, "print": true, "noZoom": false, "noRotate": false,
        "noView": false, "readOnly": false, "locked": false, "toggleNoView": false,
        "lockedContents": false, "reply": null, "popup": null, "groupId": null, "author": null,
        "createdAt": null, "modifiedAt": null, "userId": null, "createdBy": null,
        "modifiedBy": null, "importedBy": null, "actions": null,
        "color": "#ef4444", "opacity": 1,
    });
    record.as_object().cloned().unwrap()
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Value {
    json!({ "x": x, "y": y, "width": width, "height": height })
}

/// A `redact` record, an area mark.
fn redact(n: u32, page: u32, area: Value) -> Value {
    let mut record = base("redact", n, page, area);
    for (k, v) in [
        ("quadPoints", json!([])),
        ("interiorColor", json!("#000000")),
        ("overlayText", Value::Null),
        ("repeat", json!(false)),
        ("fontFamily", json!("helvetica")),
        ("fontSize", json!(0)),
        ("fontColor", json!("#ffffff")),
        ("textAlign", json!("center")),
    ] {
        record.insert(k.into(), v);
    }
    Value::Object(record)
}

fn square(n: u32, page: u32, area: Value) -> Value {
    let mut record = base("square", n, page, area.clone());
    for (k, v) in [
        ("strokeWidth", json!(2)),
        ("borderStyle", json!("solid")),
        ("dashArray", Value::Null),
        ("interiorColor", Value::Null),
        ("box", area),
        ("rotation", Value::Null),
        ("cloudyIntensity", Value::Null),
    ] {
        record.insert(k.into(), v);
    }
    Value::Object(record)
}

fn listing(records: Vec<Value>) -> Value {
    json!({ "path": "annotations.list", "value": { "annotations": records } })
}

fn apply_result(scope: Value, pages: Vec<Value>, op: &str) -> Value {
    let results: Vec<Value> = pages
        .iter()
        .map(|p| json!({ "page": p, "status": "applied", "removedAnnotationCount": 1 }))
        .collect();
    json!({
        "scope": scope,
        "results": results,
        "removedAnnotationCount": pages.len(),
        "meta": { "affectedPages": pages, "cacheDelta": null, "opId": op, "undoable": false },
    })
}

/// A `redaction.apply` answer for `scope`, publishing its confirmed event.
fn apply_answer(scope: Value, pages: Vec<Value>, op: &str) -> Value {
    let result = apply_result(scope.clone(), pages, op);
    let mut event = json!({ "type": "redaction.applied" });
    for (k, v) in result.as_object().unwrap() {
        event[k] = v.clone();
    }
    event["origin"] = json!({
        "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
        "tx": { "id": op, "index": 0, "count": 1 },
    });
    json!({
        "path": "redaction.apply",
        "args": [scope],
        "value": result,
        "events": [event],
        "times": 1,
    })
}

/// Marks 10 (page 1) and 12 (page 2), and a square on each page under its
/// mark.
fn seed() -> Vec<Value> {
    vec![
        square(1, 1, rect(100.0, 100.0, 100.0, 100.0)),
        redact(10, 1, rect(90.0, 90.0, 120.0, 120.0)),
        redact(12, 2, rect(10.0, 10.0, 40.0, 40.0)),
        square(5, 2, rect(20.0, 20.0, 30.0, 30.0)),
    ]
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    redaction: RedactionCapability,
    annotation: AnnotationCapability,
    reports: Rc<RefCell<Vec<Error>>>,
}

impl Rig {
    /// Document `a`, pages 1 and 2, with `security` and the redaction
    /// service when `service`, its first listing `records`, then
    /// `responses`.
    fn with(security: Value, service: bool, records: Vec<Value>, responses: Vec<Value>) -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<Error>>> = Rc::default();
        let heard = reports.clone();
        let mut all = vec![json!({
            "path": "annotations.list",
            "value": { "annotations": records },
            "times": 1,
        })];
        all.extend(responses);
        let services = if service { json!(["redaction"]) } else { json!([]) };
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [layout(1, 0, 600.0, 800.0), layout(2, 1, 400.0, 500.0)],
            "security": security,
            "services": services,
            "objectNumbers": (10001..10050).collect::<Vec<u32>>(),
            "responses": all,
        } } }));
        let plugins: Vec<AnyPlugin> = vec![
            interaction_plugin(InteractionConfig::default()),
            annotation_plugin(None),
            redaction_plugin(RedactionConfig::default()),
        ];
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            plugins,
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| heard.borrow_mut().push(error.clone()))),
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
        let redaction = kernel.capability(&REDACTION, Some("a")).expect("redaction");
        let annotation = kernel.capability(&ANNOTATION, Some("a")).expect("annotation");
        Rig { kernel, fake, redaction, annotation, reports }
    }

    fn new(responses: Vec<Value>) -> Rig {
        Rig::with(json!({ "allows": "*" }), true, seed(), responses)
    }

    fn calls_to(&self, path: &str) -> Vec<Value> {
        self.fake.log_since(0).into_iter().filter(|entry| entry["path"] == path).collect()
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn success<T: Clone + 'static>(promise: &Promise<T>) -> T {
    match settled(promise) {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("rejected where the TypeScript resolves: {}", error.message),
        None => panic!("still pending"),
    }
}

fn failure<T: Clone + 'static>(promise: &Promise<T>) -> Error {
    match settled(promise) {
        Some(Err(error)) => error,
        Some(Ok(_)) => panic!("resolved where the TypeScript rejects"),
        None => panic!("still pending"),
    }
}

fn none() -> OperationOptions {
    OperationOptions::default()
}

fn keys(marks: &[RedactionMark]) -> Vec<Value> {
    marks.iter().map(|m| m.ref_.clone()).collect()
}

/// A task that counts turns: each one is a JavaScript `await undefined`
/// loop, started before the verb, so it runs first in every turn.
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

/// `promise.then(() => at = turn)`: the turn its reaction runs on.
fn reacts_on<T: Clone + 'static>(promise: &Promise<T>, turn: &Rc<Cell<u32>>) -> u32 {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn, promise) = (at.clone(), turn.clone(), promise.clone());
    rt::spawn(async move {
        let _ = promise.await;
        seen.set(turn.get());
    });
    rt::run_until_stalled();
    at.get()
}

/// The turns are the TypeScript's, measured with the shipping kernel,
/// annotation and redaction plugins over `tooling/plugin-conformance/src/fake-engine.ts`
/// (an already settled engine answer, a counter of `await undefined` steps
/// started before the verb, the turn read in `promise.then`): `apply` and
/// `applyAll` react on 7, with a caller's signal on 8 (`cancellable.ts`
/// settles its own promise in the call's `then`), and `applyPages` on 8
/// (`return runApply(...)` from an `async` function adopts the queue's
/// promise). A refusal computed in the queue's turn (`apply` of no pending
/// mark) reacts on 4, one before the queue (`applyPages([])`, and `unmark`
/// of a ref that is not a mark) on 1.
#[test]
fn applies_settle_on_the_typescripts_turns() {
    let pages = || vec![pr(1), pr(2)];
    let answers = || {
        vec![
            apply_answer(json!({ "annotations": [obj(10, 1)] }), vec![pr(1)], "one"),
            apply_answer(json!({ "pages": pages() }), pages(), "all"),
            apply_answer(json!({ "pages": [pr(1)] }), vec![pr(1)], "page"),
        ]
    };

    let rig = Rig::new(answers());
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.redaction.apply(&[obj(10, 1)], none()), &turn), 7);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.redaction.apply_all(none()), &turn), 7);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    let applied = rig.redaction.apply_pages(&[PageTarget::Ref(PageRef::new(1))], none());
    assert_eq!(reacts_on(&applied, &turn), 8);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    let options = OperationOptions { signal: Some(AbortSignal::new()) };
    assert_eq!(reacts_on(&rig.redaction.apply(&[obj(10, 1)], options), &turn), 8);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.redaction.apply(&[obj(99, 1)], none()), &turn), 4);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.redaction.apply_pages(&[], none()), &turn), 1);

    let rig = Rig::new(answers());
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.redaction.unmark(vec![obj(99, 1)], none()), &turn), 1);
}

/// `apply(refs)` sends only the pending marks among the refs, a page's
/// apply leaves the other page's marks as they were, and
/// the confirmed event, not the answer, sets `last_result` and fires
/// `on_applied` once with the event's origin.
#[test]
fn an_apply_commits_one_page_and_leaves_the_other_pages_marks() {
    let scope = json!({ "annotations": [obj(10, 1)] });
    let rig = Rig::new(vec![
        apply_answer(scope.clone(), vec![pr(1)], "one"),
        json!({
            "path": "annotations.list",
            "args": [{ "pages": [pr(1)] }],
            "value": { "annotations": [square(1, 1, rect(100.0, 100.0, 100.0, 100.0))] },
        }),
    ]);
    let second_page = rig
        .redaction
        .list_pending(Some(RedactionMarkFilter { page: Some(PageTarget::Ref(PageRef::new(2))) }));
    assert_eq!(keys(&second_page), vec![obj(12, 2)]);
    let events: Rc<RefCell<Vec<RedactionAppliedEvent>>> = Rc::default();
    let heard = events.clone();
    let _off = rig.redaction.on_applied(move |e| {
        heard.borrow_mut().push(e.clone());
        Ok(())
    });

    // A square and an unknown ref are not pending marks: only 10 goes.
    let applied = rig.redaction.apply(&[obj(10, 1), obj(1, 1), obj(99, 1)], none());
    let result = success(&applied);
    assert_eq!(rig.calls_to("redaction.apply")[0]["args"], json!([scope]));
    assert_eq!(
        result["results"],
        json!([{ "page": pr(1), "status": "applied", "removedAnnotationCount": 1 }])
    );

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].origin["tx"]["id"], json!("one"));
    assert!(events[0].result.get("type").is_none() && events[0].result.get("origin").is_none());
    assert!(Rc::ptr_eq(&rig.redaction.get_last_result().unwrap(), &events[0].result));
    assert!(!rig.redaction.is_applying());

    // Page 1 re-read without its mark; page 2 keeps its mark as it was. Its
    // list is rebuilt once, as the TypeScript's is: `annotation.list` memoizes
    // a page's list per model (`read/annotations.ts:66-80`), and the model
    // changed. It is then stable again.
    assert_eq!(keys(&rig.redaction.list_pending(None)), vec![obj(12, 2)]);
    let after = rig
        .redaction
        .list_pending(Some(RedactionMarkFilter { page: Some(PageTarget::Index(1.0)) }));
    assert_eq!(*second_page, *after);
    let again = rig
        .redaction
        .list_pending(Some(RedactionMarkFilter { page: Some(PageTarget::Ref(PageRef::new(2))) }));
    assert!(Rc::ptr_eq(&after, &again));
    assert_eq!(rig.redaction.get_pending_count(Some(PageTarget::Index(0.0))), 0);
}

/// The pending view is the same `Rc` while the plane holds, per page and
/// whole; a page or document without marks reads one shared empty list,
/// and an index that names no page reads it too.
#[test]
fn the_pending_view_is_reference_stable() {
    let rig = Rig::new(Vec::new());
    let all = rig.redaction.list_pending(None);
    assert!(Rc::ptr_eq(&all, &rig.redaction.list_pending(None)));
    assert_eq!(keys(&all), vec![obj(10, 1), obj(12, 2)]);
    assert_eq!(all[1].page_index, 1);
    assert_eq!(all[0].kind, RedactionMarkKind::Area);

    let rig = Rig::with(
        json!({ "allows": "*" }),
        true,
        vec![square(1, 1, rect(0.0, 0.0, 9.0, 9.0))],
        Vec::new(),
    );
    let empty = rig.redaction.list_pending(None);
    assert!(empty.is_empty());
    let missing = rig
        .redaction
        .list_pending(Some(RedactionMarkFilter { page: Some(PageTarget::Index(0.5)) }));
    assert!(Rc::ptr_eq(&empty, &missing));
    let first = rig
        .redaction
        .list_pending(Some(RedactionMarkFilter { page: Some(PageTarget::Ref(PageRef::new(1))) }));
    assert!(Rc::ptr_eq(&empty, &first));
    assert_eq!(
        rig.redaction.estimate_collateral(None),
        RedactionCollateral { count: 0, refs: vec![] }
    );
}

/// Closing the document while an apply runs: the guarded call rejects at
/// once, `finishApply` after the close is one dropped state update the
/// kernel reports, and the late engine answer publishes nothing.
#[test]
fn closing_mid_apply_rejects_and_publishes_nothing() {
    let pages = vec![pr(1), pr(2)];
    let mut answer = apply_answer(json!({ "pages": pages }), pages.clone(), "closing");
    answer["defer"] = json!("closing");
    let rig = Rig::new(vec![answer]);
    let fired = Rc::new(Cell::new(0));
    let count = fired.clone();
    let _off = rig.redaction.on_applied(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    let applying = rig.redaction.apply_all(none());
    rt::run_until_stalled();
    assert!(rig.redaction.is_applying());

    let closing = rig.kernel.documents().close("a");
    rt::run_until_stalled();
    assert!(matches!(closing.peek(), Some(Ok(()))));
    let refused = failure(&applying);
    assert_eq!(refused.plugin_code(), Some(code::INSTANCE_CLOSED));

    rig.fake
        .settle("closing", Ok(apply_result(json!({ "pages": pages }), pages.clone(), "closing")));
    rt::run_until_stalled();
    assert_eq!(fired.get(), 0);
    let dropped: Vec<String> = rig
        .reports
        .borrow()
        .iter()
        .filter(|e| e.plugin_code() == Some(code::INSTANCE_CLOSED))
        .map(|e| e.message.clone())
        .collect();
    assert_eq!(dropped, vec!["a state update after the instance closed (a#1); dropped"]);
}

/// A refused apply rolls the flag back and keeps the previous result and
/// the marks; the next apply in the queue still runs.
#[test]
fn a_refused_apply_rolls_back_and_the_queue_goes_on() {
    let pages = vec![pr(1), pr(2)];
    let rig = Rig::new(vec![
        json!({
            "path": "redaction.apply",
            "times": 1,
            "error": { "name": "EngineError", "message": "permission denied", "code": "Refused" },
        }),
        apply_answer(json!({ "pages": pages }), pages.clone(), "second"),
        listing(Vec::new()),
    ]);
    let first = rig.redaction.apply(&[obj(10, 1)], none());
    let second = rig.redaction.apply_all(none());
    let refused = failure(&first);
    assert_eq!(refused.plugin_code(), Some(code::OPERATION_FAILED));
    assert_eq!(refused.message, "permission denied");
    let result = success(&second);
    assert_eq!(result["meta"]["opId"], json!("second"));
    assert!(!rig.redaction.is_applying());
    assert_eq!(rig.calls_to("redaction.apply").len(), 2);
}

/// A queued apply whose signal fires while it waits never starts; one
/// cancelled while the engine applies rejects at once with the caller's
/// reason as its cause, and the flag drops.
#[test]
fn cancels_before_and_during_an_apply() {
    let pages = vec![pr(1), pr(2)];
    let mut slow = apply_answer(json!({ "pages": pages }), pages.clone(), "slow");
    slow["defer"] = json!("slow");
    let rig = Rig::new(vec![slow]);
    let running = rig.redaction.apply_all(none());
    let signal = AbortSignal::new();
    let waiting =
        rig.redaction.apply(&[obj(10, 1)], OperationOptions { signal: Some(signal.clone()) });
    rt::run_until_stalled();
    signal.abort_value(Error::new("stop"), json!("user"));
    rig.fake.settle("slow", Ok(apply_result(json!({ "pages": pages }), pages.clone(), "slow")));
    success(&running);
    let skipped = failure(&waiting);
    assert_eq!(skipped.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(skipped.message, "cancelled before it started");
    assert_eq!(rig.calls_to("redaction.apply").len(), 1);

    let mut slow = apply_answer(json!({ "pages": pages }), pages.clone(), "slow");
    slow["defer"] = json!("slow");
    let rig = Rig::new(vec![slow]);
    let signal = AbortSignal::new();
    let running = rig.redaction.apply_all(OperationOptions { signal: Some(signal.clone()) });
    rt::run_until_stalled();
    assert!(rig.redaction.is_applying());
    signal.abort_value(Error::new("stop"), json!("user"));
    let cancelled = failure(&running);
    assert_eq!(cancelled.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(cancelled.message, "operation cancelled");
    assert_eq!(cancelled.member("cause"), Some(&json!("user")));
    assert!(!rig.redaction.is_applying());
}

/// `unmark` keeps three outcomes apart: refs that are not pending marks
/// are skipped, a mark this session may not delete fails `permission-denied`
/// before any write, an engine refusal fails with the annotation plugin's
/// error; a signal that already fired rejects the whole call.
#[test]
fn unmark_separates_skips_failures_and_rejections() {
    let rig = Rig::new(vec![json!({
        "path": "apply",
        "error": { "name": "EngineError", "message": "locked", "code": "Refused" },
    })]);
    let batch = success(&rig.redaction.unmark(vec![obj(1, 1), obj(10, 1)], none()));
    assert!(batch.applied.is_empty());
    assert_eq!(
        batch.skipped,
        vec![BatchSkip { ref_: obj(1, 1), reason: "not a pending redaction mark".into() }]
    );
    assert_eq!(batch.failed.len(), 1);
    assert_eq!(batch.failed[0].error["message"], json!("locked"));
    assert_eq!(batch.failed[0].error["capability"], json!("annotation"));
    assert_eq!(keys(&rig.redaction.list_pending(None)), vec![obj(10, 1), obj(12, 2)]);

    let signal = AbortSignal::new();
    signal.abort_value(Error::new("stop"), json!("user"));
    let before = rig.calls_to("apply").len();
    let rejected =
        failure(&rig.redaction.unmark(vec![obj(10, 1)], OperationOptions { signal: Some(signal) }));
    assert_eq!(rejected.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(rig.calls_to("apply").len(), before);

    let rig = Rig::with(
        json!({ "allows": ["doc.annotate.read", "doc.annotate.create"] }),
        true,
        seed(),
        Vec::new(),
    );
    assert!(rig.redaction.can_mark());
    assert!(!rig.redaction.can_unmark(&obj(10, 1)));
    let batch = success(&rig.redaction.clear_pending(none()));
    assert_eq!(batch.failed.len(), 2);
    assert_eq!(
        batch.failed[0].error,
        json!({
            "code": "permission-denied",
            "message": "redaction.unmark requires annotations:delete",
            "capability": "redaction",
            "permission": "annotations:delete",
        })
    );
    assert!(rig.calls_to("apply").is_empty());
}

/// Marking needs annotation create authority; applying needs the service
/// and all three grants, each on its own.
#[test]
fn marking_and_applying_are_separate_powers() {
    for missing in APPLY_CAPABILITIES {
        let mut granted: Vec<&str> = vec!["doc.annotate.read", "doc.annotate.create"];
        granted.extend(APPLY_CAPABILITIES.iter().filter(|c| **c != missing));
        let rig = Rig::with(json!({ "allows": granted }), true, seed(), Vec::new());
        assert!(rig.redaction.can_mark(), "{missing}");
        assert!(!rig.redaction.can_apply(), "{missing}");
    }
    let rig = Rig::with(json!({ "allows": ["doc.annotate.read"] }), true, seed(), Vec::new());
    assert!(!rig.redaction.can_mark());
    let refused = failure(&rig.redaction.mark_page(PageTarget::Index(0.0), none()));
    assert_eq!(refused.plugin_code(), Some(code::PERMISSION_DENIED));
    assert_eq!(refused.message, "redaction.markPage requires annotations:create");
    assert_eq!(refused.member("permission"), Some(&json!("annotations:create")));
    let refused = failure(&rig.redaction.update_label(
        obj(10, 1),
        RedactionLabelPatch { overlay_text: Some(None), repeat: None },
        none(),
    ));
    assert_eq!(refused.plugin_code(), Some(code::PERMISSION_DENIED));
    assert_eq!(refused.member("permission"), Some(&json!("annotations:update")));
}

/// `requireService()` on an engine with no redaction service: the
/// TypeScript fake always builds one, so no vector reaches this branch.
/// The Rust fake can leave it out; the apply refuses `unsupported` before
/// the flag moves or the engine is asked, and `canApply` is false.
#[test]
fn without_a_redaction_service_applies_refuse_unsupported() {
    let rig = Rig::with(json!({ "allows": "*" }), false, seed(), Vec::new());
    assert!(!rig.redaction.can_apply());
    let refused = failure(&rig.redaction.apply_all(none()));
    assert_eq!(refused.plugin_code(), Some(code::UNSUPPORTED));
    assert_eq!(refused.message, "this engine has no redaction service");
    assert!(rig.calls_to("redaction.apply").is_empty());
    assert!(!rig.redaction.is_applying());
    assert_eq!(rig.redaction.get_pending_count(None), 2);
}

/// A throwing `on_applied` listener reaches the kernel's report; the next
/// listener still runs. A remote apply, with no call of this session, sets
/// the last result too.
#[test]
fn a_remote_apply_reaches_every_listener() {
    let rig = Rig::new(vec![listing(Vec::new())]);
    let _bad = rig.redaction.on_applied(|_| Err(Error::new("listener broke")));
    let heard = Rc::new(Cell::new(0));
    let count = heard.clone();
    let _good = rig.redaction.on_applied(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    let pages = vec![pr(1), pr(2)];
    let mut event = json!({ "type": "redaction.applied" });
    for (k, v) in
        apply_result(json!({ "pages": pages }), pages.clone(), "remote").as_object().unwrap()
    {
        event[k] = v.clone();
    }
    event["origin"] =
        json!({ "kind": "remote", "sessionId": "s2", "sub": null, "ts": 0, "serverId": "9" });
    rig.fake.emit("a", &event).expect("emit");
    rt::run_until_stalled();
    assert_eq!(heard.get(), 1);
    assert_eq!(rig.redaction.get_last_result().unwrap()["meta"]["opId"], json!("remote"));
    assert!(rig.reports.borrow().iter().any(|e| e.message == "listener broke"));
}

/// The overlay settings are the redact tool's defaults: written on
/// connect and again when `overlay` changes.
#[test]
fn the_overlay_settings_are_the_redact_tools_defaults() {
    let rig = Rig::new(Vec::new());
    let defaults = rig.annotation.tools_get_defaults("redact");
    assert_eq!(defaults["interiorColor"], json!("#000000"));
    assert_eq!(defaults["fontColor"], json!("#ffffff"));
    rig.redaction.update_settings(&RedactionConfig {
        overlay: Some(RedactionOverlayConfig {
            fill: Some("#ff0000".into()),
            text: Some(RedactionOverlayTextConfig {
                font_family: Some("courier".into()),
                ..RedactionOverlayTextConfig::default()
            }),
        }),
    });
    rt::run_until_stalled();
    let defaults = rig.annotation.tools_get_defaults("redact");
    assert_eq!(defaults["interiorColor"], json!("#ff0000"));
    assert_eq!(defaults["fontFamily"], json!("courier"));
    assert_eq!(rig.redaction.get_settings().overlay.text.color, "#ffffff");
    rig.redaction.reset_settings();
    rt::run_until_stalled();
    assert_eq!(rig.annotation.tools_get_defaults("redact")["interiorColor"], json!("#000000"));
}

/// A page filter that names no page refuses `markMatches` with `not-found`
/// before the search runs, and the page lookups refuse with the kernel's
/// messages.
#[test]
fn page_lookups_refuse_with_not_found() {
    let rig = Rig::new(Vec::new());
    let refused = failure(&rig.redaction.mark_page(PageTarget::Ref(PageRef::new(9)), none()));
    assert_eq!(refused.plugin_code(), Some(code::NOT_FOUND));
    assert_eq!(refused.message, "page 9 is not in this document");
    let refused = failure(&rig.redaction.apply_pages(&[PageTarget::Index(2.0)], none()));
    assert_eq!(refused.message, "there is no page 2 in this document");
    // No search plugin is installed: `unsupported` comes first.
    let refused = failure(&rig.redaction.mark_matches(
        epdf_plugin_search::SearchQuery { text: "x".into(), ..Default::default() },
        Some(vec![PageTarget::Index(7.0)]),
        none(),
    ));
    assert_eq!(refused.plugin_code(), Some(code::UNSUPPORTED));
    assert!(rig.calls_to("search.query").is_empty());
}
