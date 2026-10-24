#![recursion_limit = "256"]

//! Behaviour the vectors don't pin: kept Rust-only divergences and safety
//! guards.
//!
//! H3: a record keeps its key from its first frame (no temp id to rename),
//! and one that leaves the view mid-gesture leaves the session in the same
//! notification, before any later pointer sample can reach it.
//!
//! N6: a ref of a kind no key is defined for is refused, where the
//! TypeScript keys it `"undefined"` and collapses every such record into one.

use std::rc::Rc;

use epdf_annotation_core::Json;

use epdf_geometry::Point;
use epdf_kernel::engine::{
    DocumentHandle, DocumentSecurity, Engine, EngineCall, ObjectNumbersLost,
    ObjectNumbersLostReason,
};
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_annotation::write::crud::CreateOptions;
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Map, Value, json};

const PAGE: &str = r#"{ "kind": "objectNumber", "objectNumber": 1 }"#;

fn page() -> Value {
    serde_json::from_str(PAGE).unwrap()
}

fn page_row() -> Value {
    json!({
        "index": 0, "ref": page(), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1, "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": {
            "media": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "crop": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "bleed": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "trim": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "art": { "x": 0, "y": 0, "width": 600, "height": 800 }
        }
    })
}

fn square_ref(object_number: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page(), "objectNumber": object_number })
}

/// A confirmed square at `x, y`, 100 by 100, as the engine reads it.
fn square(object_number: u32, x: f64, y: f64) -> Value {
    json!({
        "subtype": "square", "ref": square_ref(object_number), "page": page(),
        "hasAppearance": true, "appearanceState": null, "nm": null,
        "rect": { "x": x, "y": y, "width": 100, "height": 100 },
        "contents": null, "subject": null, "blendMode": "normal",
        "invisible": false, "hidden": false, "print": true, "noZoom": false, "noRotate": false,
        "noView": false, "readOnly": false, "locked": false, "toggleNoView": false,
        "lockedContents": false, "reply": null, "popup": null, "groupId": null, "author": null,
        "createdAt": null, "modifiedAt": null, "userId": null, "createdBy": null,
        "modifiedBy": null, "importedBy": null, "actions": null, "color": "#ef4444", "opacity": 1,
        "strokeWidth": 2, "borderStyle": "solid", "dashArray": null, "interiorColor": null,
        "box": { "x": x, "y": y, "width": 100, "height": 100 }, "rotation": null,
        "cloudyIntensity": null
    })
}

/// A kernel with the interaction hub and the annotation plugin over the
/// conformance fake engine; the document's annotations are `listed`, and
/// `responses` are tried before the list.
fn kernel(listed: Vec<Value>, responses: Vec<Value>) -> (Kernel, Rc<FakeEngine>) {
    kernel_over(listed, responses, |fake| Rc::new(AsEngine(fake)), None)
}

/// `kernel`, over the engine `engine` makes of the fake.
fn kernel_over(
    listed: Vec<Value>,
    responses: Vec<Value>,
    engine: impl FnOnce(Rc<FakeEngine>) -> Rc<dyn Engine>,
    report: Option<epdf_kernel::store::Report>,
) -> (Kernel, Rc<FakeEngine>) {
    rt::reset();
    let mut all = responses;
    all.push(json!({ "path": "annotations.list", "value": { "annotations": listed } }));
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [page_row()],
            "security": { "allows": "*" },
            "objectNumbers": [10001, 10002, 10003],
            "responses": all
        } }
    }));
    let kernel = Kernel::new(
        engine(fake.clone()),
        vec![interaction_plugin(InteractionConfig::default()), annotation_plugin(None)],
        KernelOptions { report, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    (kernel, fake)
}

fn annotation(kernel: &Kernel) -> AnnotationCapability {
    kernel.capability(&ANNOTATION, Some("a")).expect("the annotation capability")
}

fn refs(cap: &AnnotationCapability) -> Vec<Value> {
    cap.list(None).iter().map(|a| a["ref"].clone()).collect()
}

fn pointer(cap: &AnnotationCapability, phase: &str, x: f64, y: f64) {
    cap.edit_pointer(phase, &page(), Point { x, y }, false, None, None, None, None)
        .expect("a pointer sample");
}

/// A remote session deleted these records.
fn deleted(refs: Vec<Value>) -> Value {
    json!({
        "type": "annotations.deleted", "page": page(), "deleted": refs,
        "origin": { "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null },
        "meta": { "affectedPages": [page()], "cacheDelta": null, "changed": [], "opId": "remote-delete", "undoable": true }
    })
}

/// A remote session created this record.
fn created(record: Value) -> Value {
    json!({
        "type": "annotations.created", "page": page(), "annotation": record,
        "origin": { "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null },
        "meta": { "affectedPages": [page()], "cacheDelta": null, "changed": [], "opId": "remote-create", "undoable": true }
    })
}

// ── H3 ──────────────────────────────────────────────────────────────────────

#[test]
fn h3_a_created_record_keeps_its_key_before_and_after_the_engine_confirms_it() {
    let op_id = "00000000-0000-7000-8000-000000000001";
    // The engine's answer, and the event it publishes with it.
    let event = json!({
        "type": "annotations.created", "page": page(), "annotation": square(10001, 100.0, 100.0),
        "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true,
                  "changed": [square_ref(10001)] },
        "origin": { "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
                    "tx": { "id": op_id, "index": 0, "count": 1 } }
    });
    let (kernel, fake) = kernel(
        Vec::new(),
        vec![json!({ "path": "apply", "times": 1, "defer": "born", "events": [event] })],
    );
    let cap = annotation(&kernel);
    let mut fields = Map::new();
    fields.insert("subtype".into(), json!("square"));
    fields.insert("box".into(), json!({ "x": 100, "y": 100, "width": 100, "height": 100 }));
    let made = cap.create(&page(), &fields, None, CreateOptions::default());
    rt::run_until_stalled();
    // Shown at once, under the object number the engine handed out.
    assert_eq!(refs(&cap), vec![square_ref(10001)]);
    let confirmed = json!({
        "items": [{
            "type": "annotations.create", "page": page(), "annotation": square(10001, 100.0, 100.0),
            "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true,
                      "changed": [square_ref(10001)] }
        }],
        "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true }
    });
    assert!(fake.settle("born", Ok(confirmed)));
    rt::run_until_stalled();
    // The same key once confirmed: nothing to rename.
    assert_eq!(refs(&cap), vec![square_ref(10001)]);
    assert_eq!(
        made.peek().expect("settled").expect("created")["annotation"]["ref"],
        square_ref(10001)
    );
}

#[test]
fn h3_a_record_deleted_mid_drag_leaves_the_session_before_the_next_sample() {
    let (kernel, fake) = kernel(vec![square(7, 100.0, 100.0)], Vec::new());
    let cap = annotation(&kernel);
    cap.selection_set(&[square_ref(7)]);
    // On its left edge: a square with no fill is hit on its stroke.
    pointer(&cap, "down", 100.0, 150.0);
    pointer(&cap, "move", 110.0, 160.0);
    assert!(cap.model().draft().is_some(), "dragging");
    fake.emit("a", &deleted(vec![square_ref(7)])).expect("the event");
    // No turn has run: the forget is part of the notification itself.
    assert!(cap.model().draft().is_none(), "the drag went with its record");
    assert!(cap.list_selected().is_empty());
    // The rest of the gesture finds nothing to move and writes nothing.
    let before = fake.log_len();
    pointer(&cap, "move", 130.0, 180.0);
    pointer(&cap, "up", 130.0, 180.0);
    rt::run_until_stalled();
    assert!(refs(&cap).is_empty());
    let applied = fake.log_since(before).iter().filter(|call| call["path"] == "apply").count();
    assert_eq!(applied, 0, "no write for a record that is gone");
}

// ── N6 ──────────────────────────────────────────────────────────────────────

#[test]
fn n6_a_record_whose_ref_has_no_key_is_refused_and_the_others_stay() {
    let reports = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let sink = reports.clone();
    let (kernel, fake) = kernel_over(
        vec![square(7, 100.0, 100.0)],
        Vec::new(),
        |fake| Rc::new(AsEngine(fake)),
        Some(Rc::new(move |e: &epdf_kernel::Error| sink.borrow_mut().push(e.to_string()))),
    );
    let cap = annotation(&kernel);
    let mut stray = square(8, 300.0, 300.0);
    stray["ref"] = json!({ "kind": "bogus", "page": page(), "objectNumber": 8 });
    let mut other = square(9, 400.0, 400.0);
    other["ref"] = json!({ "kind": "bogus", "page": page(), "objectNumber": 9 });
    let first = fake.emit("a", &created(stray));
    let second = fake.emit("a", &created(other));
    rt::run_until_stalled();
    // Delivered, and each refusal reported through the kernel, not dropped.
    assert!(first.is_ok() && second.is_ok(), "{first:?} {second:?}");
    assert_eq!(
        *reports.borrow(),
        vec![
            "Error: [annotation] a ref of kind \"bogus\" has no annotation key".to_string(),
            "Error: [annotation] a ref of kind \"bogus\" has no annotation key".to_string(),
        ]
    );
    // TypeScript keys both "undefined", so the second replaces the first.
    // Rust keys neither: the list holds only the record it could key.
    assert_eq!(refs(&cap), vec![square_ref(7)]);
    assert!(cap.get(&json!({ "kind": "bogus", "page": page(), "objectNumber": 8 })).is_none());
    assert!(cap.get(&square_ref(7)).is_some());
}

// ── lost object numbers (Ruling13.17 onLost) ────────────────────────────────

type LostListeners = Rc<std::cell::RefCell<Vec<Rc<dyn Fn(&ObjectNumbersLost)>>>>;

/// The fake engine, with documents whose pool can lose numbers and, when
/// `read` is given, whose `doc.annotate.read` grant it switches.
struct LosingEngine(AsEngine, LostListeners, Option<Rc<std::cell::Cell<bool>>>);

impl Engine for LosingEngine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let listeners = self.1.clone();
        let read = self.2.clone();
        self.0.open(input, options).convert(move |inner| {
            Ok(Rc::new(LosingHandle(inner.clone(), listeners.clone(), read.clone()))
                as Rc<dyn DocumentHandle>)
        })
    }
    fn destroy(&self) -> EngineCall<()> {
        self.0.destroy()
    }
}

struct LosingHandle(Rc<dyn DocumentHandle>, LostListeners, Option<Rc<std::cell::Cell<bool>>>);

/// The fake's security, with `doc.annotate.read` as `read` says.
struct Gated(Rc<dyn DocumentSecurity>, Rc<std::cell::Cell<bool>>);

impl DocumentSecurity for Gated {
    fn allows(&self, capability: &str) -> bool {
        if capability == "doc.annotate.read" { self.1.get() } else { self.0.allows(capability) }
    }
    fn allows_annotation_create(&self) -> bool {
        self.0.allows_annotation_create()
    }
    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool {
        self.0.allows_annotation_mutation(action, target)
    }
    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool {
        self.0.allows_annotation_group_assignment(group_id)
    }
    fn effective_scope(&self) -> Vec<String> {
        self.0.effective_scope()
    }
    fn identity(&self) -> Value {
        self.0.identity()
    }
    fn current(&self) -> Value {
        self.0.current()
    }
    fn password_prompt(&self) -> Value {
        self.0.password_prompt()
    }
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        self.0.unlock(input)
    }
}

impl DocumentHandle for LosingHandle {
    fn is_local(&self) -> bool {
        self.0.is_local()
    }
    fn held_object_numbers(&self) -> usize {
        self.0.held_object_numbers()
    }
    fn take_object_number(&self) -> Result<Option<u32>, epdf_kernel::Error> {
        self.0.take_object_number()
    }
    fn on_lost_object_numbers(
        &self,
        listener: Rc<dyn Fn(&ObjectNumbersLost)>,
    ) -> epdf_kernel::Unsubscribe {
        self.1.borrow_mut().push(listener);
        epdf_kernel::Unsubscribe::noop()
    }
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        self.0.reserve_object_numbers(count)
    }
    fn id(&self) -> String {
        self.0.id()
    }
    fn capabilities(&self) -> Value {
        self.0.capabilities()
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        match &self.2 {
            Some(read) => Rc::new(Gated(self.0.security(), read.clone())),
            None => self.0.security(),
        }
    }
    fn has(&self, member: &str) -> bool {
        self.0.has(member)
    }
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.0.call(path, pon, args)
    }
    fn page_index(&self, pon: u32) -> Result<u32, epdf_kernel::Error> {
        self.0.page_index(pon)
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
        self.0.subscribe(listener)
    }
    fn last_server_id(&self) -> Option<String> {
        self.0.last_server_id()
    }
}

/// `services/object-numbers.ts`: once the session holds numbers, a number
/// the engine reports lost leaves the session, and the next create takes
/// another from the pool instead of naming the lost one.
#[test]
fn a_lost_object_number_leaves_the_session() {
    let listeners: LostListeners = Rc::default();
    let engines = listeners.clone();
    let (kernel, fake) = kernel_over(
        Vec::new(),
        vec![json!({ "path": "apply", "defer": "born" })],
        move |fake| Rc::new(LosingEngine(AsEngine(fake), engines, None)),
        None,
    );
    let cap = annotation(&kernel);
    // A square drawn by drag holds one number from the press.
    cap.create_pointer("square", "down", &page(), Point { x: 100.0, y: 100.0 }, false, None)
        .expect("a press");
    assert_eq!(*cap.model().object_numbers, vec![10001.0], "held from the press");
    assert_eq!(listeners.borrow().len(), 1, "watching once the session holds numbers");
    let lost =
        ObjectNumbersLost { numbers: vec![10001], reason: ObjectNumbersLostReason::Versioned };
    let watching: Vec<_> = listeners.borrow().clone();
    for listener in watching {
        listener(&lost);
    }
    assert!(cap.model().object_numbers.is_empty(), "the lost number left the session");
    cap.create_pointer("square", "move", &page(), Point { x: 200.0, y: 180.0 }, false, None)
        .expect("a move");
    cap.create_pointer("square", "up", &page(), Point { x: 200.0, y: 180.0 }, false, None)
        .expect("a release");
    rt::run_until_stalled();
    let applied: Vec<Value> =
        fake.log_since(0).into_iter().filter(|call| call["path"] == "apply").collect();
    let created = applied.last().expect("the create was written");
    assert_eq!(created["args"][0]["ops"][0]["objectNumber"].as_f64(), Some(10002.0), "{created}");
    assert_eq!(listeners.borrow().len(), 1, "one subscription");
}

// ── re-entry from a failure callback ────────────────────────────────────────

/// `annotation-live.test.ts` "the explicit internal failure-callback reentry
/// edge": inside `onWriteFailed` for a refused create, the drag on that
/// record is still a move; re-entering `editPointer` from the callback does
/// not fail, and afterwards the record is gone from every public read while
/// the raw session still names it as selected. The dependent move written
/// before the refusal is aborted, and nothing is reported.
#[test]
fn a_write_failed_listener_can_reenter_the_pointer_and_leaves_the_record_publicly_gone() {
    use std::cell::{Cell, RefCell};
    let reports = Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = reports.clone();
    let (kernel, fake) = kernel_over(
        Vec::new(),
        vec![
            json!({ "path": "apply", "times": 1, "defer": "born" }),
            json!({ "path": "apply", "defer": "moved" }),
        ],
        |fake| Rc::new(AsEngine(fake)),
        Some(Rc::new(move |e: &epdf_kernel::Error| sink.borrow_mut().push(e.to_string()))),
    );
    let cap = annotation(&kernel);
    let reentered = Rc::new(Cell::new(false));
    let callback_error = Rc::new(RefCell::new(None::<String>));
    let draft_kind = Rc::new(RefCell::new(None::<String>));
    let (inside, flag, error, kind) =
        (cap.clone(), reentered.clone(), callback_error.clone(), draft_kind.clone());
    let _off = cap.on_write_failed(move |_| {
        if flag.get() {
            return Ok(());
        }
        flag.set(true);
        *kind.borrow_mut() = inside
            .model()
            .draft
            .as_ref()
            .map(|d| d.json()["kind"].as_str().unwrap_or("").to_string());
        let page = page();
        let moved = inside
            .edit_pointer(
                "move",
                &page,
                Point { x: 120.0, y: 145.0 },
                false,
                None,
                None,
                None,
                None,
            )
            .and_then(|()| {
                inside.edit_pointer(
                    "up",
                    &page,
                    Point { x: 120.0, y: 145.0 },
                    false,
                    None,
                    None,
                    None,
                    None,
                )
            });
        if let Err(e) = moved {
            *error.borrow_mut() = Some(e.to_string());
        }
        Ok(())
    });
    let mut fields = Map::new();
    fields.insert("subtype".into(), json!("square"));
    fields.insert("box".into(), json!({ "x": 100, "y": 100, "width": 100, "height": 100 }));
    fields.insert("color".into(), json!("#ef4444"));
    fields.insert("strokeWidth".into(), json!(2));
    let create = cap.create(&page(), &fields, None, CreateOptions::default());
    rt::run_until_stalled();
    cap.selection_set(&[square_ref(10001)]);
    pointer(&cap, "down", 100.0, 125.0);
    pointer(&cap, "move", 110.0, 135.0);
    let refusal =
        json!({ "name": "EngineError", "code": "Refused", "message": "intended reentry refusal" });
    assert!(fake.settle("born", Err(&refusal)));
    rt::run_until_stalled();
    let rejected = create.peek().expect("settled").expect_err("the create was refused");
    assert!(rejected.to_string().contains("intended reentry refusal"), "{rejected}");

    assert!(reentered.get());
    assert_eq!(callback_error.borrow().clone(), None);
    assert_eq!(draft_kind.borrow().as_deref(), Some("move"), "still a move inside the callback");
    let model = cap.model();
    assert!(model.draft.is_none());
    // The raw session still names it (a separate current TypeScript edge).
    assert_eq!(*model.selected, vec!["obj:10001".to_string()]);
    assert!(cap.list_selected().is_empty());
    assert!(cap.selection_get_anchor().is_null());
    assert!(cap.get(&square_ref(10001)).is_none());
    assert!(!cap.can_update(&square_ref(10001)));
    assert!(!cap.can_delete(&square_ref(10001)));
    assert!(cap.list(None).is_empty());
    assert_eq!(cap.list_page_items(&page(), None), json!([]));
    assert_eq!(cap.list_chrome_nodes(&page(), None, None, None), json!([]));
    assert!(cap.draft_get().is_null());

    let log = fake.log_since(0);
    let writes: Vec<usize> =
        log.iter().enumerate().filter(|(_, e)| e["path"] == "apply").map(|(i, _)| i).collect();
    assert_eq!(writes.len(), 2, "{log:?}");
    assert!(
        log.iter().any(|e| e["abort"].as_u64() == Some(writes[1] as u64)),
        "the dependent move is aborted: {log:?}"
    );
    assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
    assert!(fake.unmatched().is_empty(), "{:?}", fake.unmatched());
}

// ── M19: comment threads stay linear and are built once per change ─────────

fn note(n: u32, reply_to: Option<u32>) -> Value {
    let mut dto = square(n, 10.0, 10.0);
    dto["subtype"] = json!("text");
    if let Some(parent) = reply_to {
        dto["reply"] = json!({ "to": square_ref(parent), "type": "reply" });
    }
    dto
}

/// The threads index is memoized on the records: two reads over unchanged
/// records are the same `Rc`, and a confirmed change builds a new one.
#[test]
fn m19_the_threads_index_is_built_once_per_change() {
    let (kernel, fake) = kernel(vec![note(5, None), note(6, Some(5))], Vec::new());
    let cap = annotation(&kernel);
    let first = cap.threads_index();
    assert_eq!(first.threads.len(), 1);
    assert!(Rc::ptr_eq(&first, &cap.threads_index()), "an unchanged read reuses the index");
    fake.emit("a", &created(note(7, None))).expect("the event");
    rt::run_until_stalled();
    let after = cap.threads_index();
    assert!(!Rc::ptr_eq(&first, &after), "a change builds it again");
    assert_eq!(after.threads.len(), 2);
}

/// `buildCommentThreads` over 10,000 annotations (2,000 roots, each with a
/// chain of four replies) finishes well inside a hang ceiling: the key
/// lookups, child lists and visited set are hashed, as the TypeScript's
/// `Map` and `Set` are. The ceiling catches a quadratic walk (seconds to
/// minutes), not a slow machine.
#[test]
fn m19_threading_ten_thousand_annotations_stays_linear() {
    use epdf_plugin_annotation::comments::build::build_comment_threads;
    const ROOTS: u32 = 2_000;
    const REPLIES: u32 = 4;
    let mut annotations = Vec::new();
    for t in 0..ROOTS {
        let root = 1 + t * (REPLIES + 1);
        annotations.push(note(root, None));
        for k in 1..=REPLIES {
            annotations.push(note(root + k, Some(root + k - 1)));
        }
    }
    let started = std::time::Instant::now();
    let threads = build_comment_threads(&annotations, None);
    let took = started.elapsed();
    assert_eq!(threads.len(), ROOTS as usize);
    assert!(
        threads.iter().all(|t| t["replies"].as_array().map(Vec::len) == Some(REPLIES as usize))
    );
    assert!(took < std::time::Duration::from_secs(10), "threading took {took:?}");
}

// ── M16: a grant after a forbidden load is seen on refresh ──────────────────

/// Superseded M16: the records load refused for lack of `doc.annotate.read`
/// does not stick. After the grant, the next `refresh` loads (the kernel
/// mirror clears `running` on every path, as `mirror.ts` does).
#[test]
fn m16_a_read_grant_after_a_forbidden_load_is_seen_on_refresh() {
    use epdf_kernel::mirror::ResourceStatus;
    let read = Rc::new(std::cell::Cell::new(false));
    let gate = read.clone();
    let (kernel, _fake) = kernel_over(
        vec![square(7, 100.0, 100.0)],
        Vec::new(),
        move |fake| Rc::new(LosingEngine(AsEngine(fake), Rc::default(), Some(gate))),
        None,
    );
    let cap = annotation(&kernel);
    assert_eq!(cap.get_status(), ResourceStatus::Forbidden);
    let _ = cap.refresh(None);
    rt::run_until_stalled();
    assert_eq!(cap.get_status(), ResourceStatus::Forbidden, "still no grant");
    assert!(refs(&cap).is_empty());
    read.set(true);
    let loaded = cap.refresh(None);
    rt::run_until_stalled();
    assert!(loaded.peek().expect("settled").is_ok());
    assert_eq!(cap.get_status(), ResourceStatus::Ready);
    assert_eq!(refs(&cap), vec![square_ref(7)]);
}

// ── getCursorAt: a cursorless handle is undefined, nothing is null ─────────

/// `getCursorAt` tells a handle without a cursor (`undefined`, here a
/// corner seen at a NaN display rotation, whose sector cursor TypeScript
/// reads as `SECTOR_CURSORS[NaN]`) from a point over nothing (`null`).
#[test]
fn a_cursorless_handle_reads_undefined_and_nothing_reads_null() {
    use epdf_plugin_annotation::read::chrome::HitView;
    let (kernel, _fake) = kernel(vec![square(7, 100.0, 100.0)], Vec::new());
    let cap = annotation(&kernel);
    cap.selection_set(&[square_ref(7)]);
    let at = |x: f64, y: f64, rotation: Option<f64>| {
        cap.get_cursor_at(&page(), Point { x, y }, HitView { scale: None, rotation, zoom: None })
    };
    let upright = at(100.0, 100.0, None);
    assert!(upright.is_string(), "a corner handle has a cursor upright: {upright}");
    assert!(epdf_kernel::is_undefined(&at(100.0, 100.0, Some(f64::NAN))));
    assert_eq!(at(400.0, 400.0, None), Value::Null);
}
