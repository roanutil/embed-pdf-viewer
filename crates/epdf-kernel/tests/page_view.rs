//! `packages/core/main/test/page-view.test.ts`: the page list is a view, the
//! engine's pages with this session's pending page changes on top. A page
//! change shows at once and leaves the view as the engine answers; an
//! answer the prediction got right changes nothing; a refusal takes it
//! back; another session's change lands under the pending ones; and a page
//! mirror reads a page a pending change makes once the engine has it.

use epdf_kernel::{
    Error, Kernel, KernelOptions, OpenOptions, OpenSource, Promise,
    action::CoreAction,
    changes::{ChangeQueue, StagedChange},
    define,
    engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall},
    event_hook::EventHookSource,
    geometry::PageRef,
    page_mirror::{Affected, PageMirror, PageMirrorSpec},
    page_predict::{label_at, restored_pages_of},
    plugin::Controller,
    rt,
    store::Unsubscribe,
    token::Token,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Open;
impl DocumentSecurity for Open {
    fn allows(&self, _: &str) -> bool {
        true
    }
    fn allows_annotation_create(&self) -> bool {
        true
    }
    fn allows_annotation_mutation(&self, _: &str, _: &Value) -> bool {
        true
    }
    fn allows_annotation_group_assignment(&self, _: &str) -> bool {
        true
    }
    fn effective_scope(&self) -> Vec<String> {
        vec![]
    }
    fn identity(&self) -> Value {
        Value::Null
    }
    fn current(&self) -> Value {
        json!({ "locked": false })
    }
    fn password_prompt(&self) -> Value {
        json!({ "state": "none" })
    }
    fn unlock(&self, _: Value) -> EngineCall<Value> {
        Promise::resolved(self.current())
    }
}

/// One `apply` the document was asked: its change, its op id, and the
/// answer the test gives it.
struct Call {
    change: Value,
    op_id: String,
    answer: epdf_kernel::rt::Resolver<Value>,
}

/// A document whose `apply` answers when the test says.
struct Doc {
    pages: Value,
    labels: Value,
    calls: RefCell<Vec<Call>>,
    events: EventHookSource<Value>,
}
impl DocumentHandle for Doc {
    fn id(&self) -> String {
        "doc".into()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(Open)
    }
    fn has(&self, _: &str) -> bool {
        false
    }
    fn call(&self, path: &str, _: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        match path {
            "pages.list" => Promise::resolved(json!({
                "pageCount": self.pages.as_array().map_or(0, Vec::len),
                "pages": self.pages,
                "labels": self.labels,
            })),
            "apply" => {
                let (promise, answer) = Promise::pending();
                let op_id = args[1]["opId"].as_str().expect("an op id").to_string();
                self.calls.borrow_mut().push(Call { change: args[0].clone(), op_id, answer });
                promise
            }
            "close" => Promise::resolved(Value::Null),
            _ => Promise::rejected(Error::engine("NotImplemented", format!("unexpected {path}"))),
        }
    }
    fn page_index(&self, _: u32) -> Result<u32, Error> {
        Err(Error::engine("NotFound", "page"))
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.events.on(move |v| {
            listener(v);
            Ok(())
        })
    }
}
struct TestEngine(Rc<Doc>);
impl Engine for TestEngine {
    fn open(&self, _: Value, _: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        Promise::resolved(self.0.clone() as Rc<dyn DocumentHandle>)
    }
    fn destroy(&self) -> EngineCall<()> {
        Promise::resolved(())
    }
}

fn r(number: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

/// A page as a read returns it.
fn page(number: u32, index: usize, rotation: u32, label: Value) -> Value {
    let page_box = json!({ "x": 0, "y": 0, "width": 612, "height": 792 });
    json!({
        "index": index,
        "ref": r(number),
        "label": label,
        "size": { "width": 612, "height": 792 },
        "rotation": rotation,
        "userUnit": 1,
        "tabOrder": null,
        "boxes": { "media": page_box, "crop": page_box, "bleed": page_box, "trim": page_box,
            "art": page_box },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 612, "top": 792 },
    })
}

fn origin(local: bool, id: Option<&str>) -> Value {
    let mut origin = json!({
        "kind": if local { "local" } else { "remote" },
        "sessionId": if local { "me" } else { "them" },
        "sub": null, "ts": 0, "serverId": null,
    });
    if let Some(id) = id {
        origin["tx"] = json!({ "id": id, "index": 0, "count": 1 });
    }
    origin
}

/// The engine's pages as a layout event carries them.
fn layout_event(kind: &str, pages: &[Value], origin: Value) -> Value {
    let pages: Vec<Value> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let mut page = page.clone();
            page["index"] = json!(index);
            page
        })
        .collect();
    json!({
        "type": kind,
        "pages": [],
        "layout": { "pageCount": pages.len(), "pages": pages, "labels": [], "namedPages": [] },
        "origin": origin,
    })
}

struct Harness {
    kernel: Kernel,
    doc: Rc<Doc>,
    ctx: Controller<Value, CoreAction>,
}

impl Harness {
    /// A document of three pages, the second turned a quarter.
    fn new() -> Self {
        Self::with(
            vec![
                page(1, 0, 0, Value::Null),
                page(2, 1, 90, Value::Null),
                page(3, 2, 0, Value::Null),
            ],
            json!([]),
        )
    }

    fn with(pages: Vec<Value>, labels: Value) -> Self {
        rt::reset();
        let doc = Rc::new(Doc {
            pages: Value::Array(pages),
            labels,
            calls: RefCell::default(),
            events: EventHookSource::new(None),
        });
        let retained: Rc<RefCell<Option<Controller<Value, CoreAction>>>> = Rc::default();
        let save = retained.clone();
        let token = Token::<()>::new("pages");
        let plugin = define::<Value, CoreAction>("pages")
            .document_scoped()
            .create(&token, move |ctx| {
                *save.borrow_mut() = Some(ctx);
                Ok(((), None))
            })
            .build();
        let options = KernelOptions {
            // A refusal is the change's answer, which its caller reads.
            report: Some(Rc::new(|_| {})),
            ..Default::default()
        };
        let kernel = Kernel::new(Rc::new(TestEngine(doc.clone())), vec![plugin], options).unwrap();
        settle(kernel.start()).unwrap();
        settle(
            kernel
                .documents()
                .open(OpenSource::Input(json!({ "id": "doc" })), OpenOptions::default()),
        )
        .unwrap();
        let ctx = retained.borrow().clone().expect("the plugin made");
        Self { kernel, doc, ctx }
    }

    fn changes(&self) -> ChangeQueue {
        self.ctx.changes().unwrap()
    }

    fn stage(&self, key: &str, ops: Vec<Value>) {
        self.changes().stage(StagedChange::ops(json!({ "key": key }), ops)).unwrap();
    }

    fn undo(&self, shows: Vec<Value>) {
        let label = json!({ "key": "history.undo" });
        self.changes().stage(StagedChange::undo(label, "the-delete".into(), shows)).unwrap();
    }

    fn pages(&self) -> Rc<Value> {
        self.ctx.document().unwrap().pages
    }

    fn revision(&self) -> u64 {
        self.ctx.document().unwrap().revision
    }

    fn order(&self) -> Vec<u64> {
        numbers(&self.pages())
    }

    fn get_page(&self, number: u32) -> Value {
        self.ctx.get_page(PageRef::new(number)).expect("the page is in the list")
    }

    fn emit(&self, event: &Value) {
        self.doc.events.emit(event);
    }

    /// Answer a call as both engines do: its events first, then the result.
    fn answer(&self, index: usize, events: impl Fn(&str) -> Vec<Value>) {
        let (op_id, answer) = {
            let calls = self.doc.calls.borrow();
            let call = &calls[index];
            (call.op_id.clone(), call.answer.clone())
        };
        for event in events(&op_id) {
            self.emit(&event);
        }
        let meta = json!({ "affectedPages": [], "cacheDelta": null, "opId": op_id,
            "undoable": true });
        answer.resolve(json!({ "items": [], "meta": meta }));
    }

    fn refuse(&self, index: usize, message: &str) {
        let answer = self.doc.calls.borrow()[index].answer.clone();
        answer.reject(Error::engine("Locked", message));
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = settle(self.kernel.destroy());
    }
}

fn settle<T: Clone + 'static>(p: Promise<T>) -> Result<T, Error> {
    rt::run_until_stalled();
    p.peek().expect("operation must settle")
}

fn numbers(pages: &Value) -> Vec<u64> {
    pages.as_array().unwrap().iter().map(|p| p["ref"]["objectNumber"].as_u64().unwrap()).collect()
}

fn pending_of(h: &Harness) -> Value {
    serde_json::to_value(&*h.ctx.document().unwrap().pending_pages).unwrap()["$map"].clone()
}

#[test]
fn shows_a_page_change_at_once_and_an_answer_that_matches_it_changes_nothing() {
    let h = Harness::new();
    let revision = h.revision();

    h.stage(
        "pages.reorder",
        vec![json!({ "type": "pages.reorder", "pages": [r(3)], "position": "start" })],
    );
    assert_eq!(h.order(), vec![3, 1, 2]);
    let indexes: Vec<Value> =
        h.pages().as_array().unwrap().iter().map(|p| p["index"].clone()).collect();
    assert_eq!(indexes, vec![json!(0), json!(1), json!(2)]);
    assert_eq!(h.revision(), revision + 1);
    // An index names the page the user sees.
    assert_eq!(h.pages()[0]["ref"], r(3));

    let shown = h.pages();
    let pages = shown.as_array().unwrap().clone();
    h.answer(0, |id| vec![layout_event("pages.reordered", &pages, origin(true, Some(id)))]);
    rt::run_until_stalled();
    assert!(Rc::ptr_eq(&h.pages(), &shown));
    assert_eq!(h.revision(), revision + 1);
}

#[test]
fn takes_a_refused_page_change_back() {
    let h = Harness::new();
    h.stage("pages.delete", vec![json!({ "type": "pages.delete", "pages": [r(2)] })]);
    assert_eq!(h.order(), vec![1, 3]);

    h.refuse(0, "locked");
    rt::run_until_stalled();
    assert_eq!(h.order(), vec![1, 2, 3]);
    assert_eq!(h.pages()[1]["rotation"], json!(90));
}

#[test]
fn puts_another_sessions_page_change_under_the_pending_ones() {
    let h = Harness::new();
    h.stage(
        "pages.rotate",
        vec![json!({ "type": "pages.rotate", "pages": [r(1)], "rotation": 180 })],
    );
    let engine: Vec<Value> = h
        .pages()
        .as_array()
        .unwrap()
        .iter()
        .map(|page| {
            let mut page = page.clone();
            if page["ref"]["objectNumber"] == 1 {
                page["rotation"] = json!(0);
            }
            page
        })
        .collect();
    let moved = [engine[2].clone(), engine[0].clone(), engine[1].clone()];
    h.emit(&layout_event("pages.reordered", &moved, origin(false, None)));
    assert_eq!(h.order(), vec![3, 1, 2]);
    assert_eq!(h.get_page(1)["rotation"], json!(180));
}

#[test]
fn shows_blank_pages_before_the_engine_has_them_named_pending() {
    let h = Harness::new();
    h.stage(
        "pages.insertBlank",
        vec![json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 300, "height": 200 } },
            "position": { "after": r(1) }, "objectNumbers": [40] })],
    );
    assert_eq!(h.order(), vec![1, 40, 2, 3]);
    assert_eq!(h.get_page(40)["size"], json!({ "width": 300, "height": 200 }));
    assert_eq!(pending_of(&h), json!([[40, { "look": "blank" }]]));

    let shown = h.pages();
    let pages = shown.as_array().unwrap().clone();
    h.answer(0, |id| vec![layout_event("pages.inserted", &pages, origin(true, Some(id)))]);
    rt::run_until_stalled();
    assert!(Rc::ptr_eq(&h.pages(), &shown));
    assert!(h.ctx.document().unwrap().pending_pages.is_empty());
}

#[test]
fn an_engine_answer_in_floats_keeps_a_blank_page_predicted_in_integers() {
    let h = Harness::new();
    h.stage(
        "pages.insertBlank",
        vec![json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 300, "height": 200 } }, "objectNumbers": [40] })],
    );
    let shown = h.pages();
    let pages_changed = Rc::new(Cell::new(0));
    let count = pages_changed.clone();
    let _off = h.kernel.documents().events().unwrap().pages_changed.on(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    // The engine writes the blank page's geometry as floats.
    let mut pages = shown.as_array().unwrap().clone();
    let float = |v: &Value| json!(v.as_f64().unwrap());
    let blank = &mut pages[3];
    for path in ["/size/width", "/size/height", "/userUnit", "/pdfCropBox/right", "/pdfCropBox/top"]
    {
        let value = float(blank.pointer(path).unwrap());
        *blank.pointer_mut(path).unwrap() = value;
    }
    for name in ["media", "crop", "bleed", "trim", "art"] {
        for member in ["x", "y", "width", "height"] {
            let value = float(&blank["boxes"][name][member]);
            blank["boxes"][name][member] = value;
        }
    }
    assert_eq!(blank["size"]["width"].to_string(), "300.0");
    h.answer(0, |id| vec![layout_event("pages.inserted", &pages, origin(true, Some(id)))]);
    rt::run_until_stalled();
    assert!(Rc::ptr_eq(&h.pages(), &shown), "the prediction's pages, kept whole");
    assert_eq!(pages_changed.get(), 0, "no pagesChanged for pages that read the same");
}

#[test]
fn shows_a_copy_at_once_pending_as_a_copy_of_its_page() {
    let h = Harness::new();
    h.stage(
        "pages.duplicate",
        vec![json!({ "type": "pages.copy", "pages": [r(2)], "position": { "after": r(1) },
            "objectNumbers": [40] })],
    );
    assert_eq!(h.order(), vec![1, 40, 2, 3]);
    assert_eq!(h.get_page(40)["rotation"], json!(90));
    assert_eq!(pending_of(&h), json!([[40, { "look": "copy", "of": r(2) }]]));
}

#[test]
fn moves_the_labels_with_the_pages_as_the_engine_does_and_an_undo_puts_them_back() {
    let labels = json!([
        { "firstPage": 0, "style": "lowerRoman", "firstNumber": 1, "prefix": "" },
        { "firstPage": 1, "style": "decimal", "firstNumber": 1, "prefix": "" },
    ]);
    let h = Harness::with(
        vec![page(1, 0, 0, json!("i")), page(2, 1, 0, json!("1")), page(3, 2, 0, json!("2"))],
        labels.clone(),
    );
    let shown = || -> Vec<Value> {
        h.pages().as_array().unwrap().iter().map(|p| p["label"].clone()).collect()
    };
    assert_eq!(label_at(&labels, 1.0).unwrap(), json!("1"));
    // The body's first page goes: the body starts on its next page.
    h.stage("pages.delete", vec![json!({ "type": "pages.delete", "pages": [r(2)] })]);
    assert_eq!(shown(), vec![json!("i"), json!("1")]);
    // A page inserted before the body takes the front matter's numbering.
    h.stage(
        "pages.insertBlank",
        vec![json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 100, "height": 100 } },
            "position": { "after": r(1) }, "objectNumbers": [40] })],
    );
    assert_eq!(shown(), vec![json!("i"), json!("ii"), json!("1")]);
    // An undo that puts the sections back shows them so.
    h.undo(vec![json!({ "type": "pages.relabel", "labels": labels })]);
    assert_eq!(*h.ctx.document().unwrap().page_labels, labels);
    assert_eq!(shown(), vec![json!("i"), json!("1"), json!("2")]);
}

#[test]
fn shows_deleted_pages_back_at_once_when_an_undo_restores_them() {
    let h = Harness::new();
    let before = h.pages().as_array().unwrap().clone();
    let engine: Vec<Value> =
        before.iter().filter(|p| p["ref"]["objectNumber"] != 2).cloned().collect();
    h.emit(&layout_event("pages.deleted", &engine, origin(true, None)));
    assert_eq!(h.order(), vec![1, 3]);

    let restored = restored_pages_of(&before, &[r(2)]).unwrap();
    h.undo(vec![json!({ "type": "pages.restore", "pages": restored })]);
    assert_eq!(h.order(), vec![1, 2, 3]);
    assert_eq!(h.get_page(2)["rotation"], json!(90));
    assert_eq!(pending_of(&h), json!([[2, { "look": "own" }]]));
}

/// A page mirror whose reads answer `"read"`, and a blank insert of page 40
/// on its way.
fn mirrored() -> (Harness, PageMirror, Rc<Cell<u32>>) {
    let h = Harness::new();
    let loads = Rc::new(Cell::new(0));
    let count = loads.clone();
    let mirror = h
        .ctx
        .page_mirror(PageMirrorSpec {
            name: "notes".into(),
            load: Rc::new(move |_, _, _| {
                count.set(count.get() + 1);
                Promise::resolved(json!("read"))
            }),
            affected: Rc::new(|_| Affected::None),
            fold: None,
            predict: None,
            changed: None,
        })
        .unwrap();
    h.stage(
        "pages.insertBlank",
        vec![json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 300, "height": 200 } }, "objectNumbers": [40] })],
    );
    (h, mirror, loads)
}

#[test]
fn a_page_mirror_reads_a_page_a_pending_change_makes_once_the_engine_has_it() {
    let (h, mirror, loads) = mirrored();
    let listening = h.ctx.lifetime().listener_count();
    let loading = mirror.ensure_loaded(r(40));
    rt::run_until_stalled();
    assert_eq!(loads.get(), 0);
    assert_eq!(mirror.get_status(&r(40)).as_str(), "loading");
    assert_eq!(h.ctx.lifetime().listener_count(), listening + 1, "waiting on the lifetime");

    let pages = h.pages().as_array().unwrap().clone();
    h.answer(0, |id| vec![layout_event("pages.inserted", &pages, origin(true, Some(id)))]);
    settle(loading).unwrap();
    assert_eq!(loads.get(), 1);
    assert_eq!(*mirror.get(&r(40)).unwrap(), json!("read"));
    // The engine has the page: the wait takes its listener off the lifetime.
    assert_eq!(h.ctx.lifetime().listener_count(), listening);
}

#[test]
fn a_page_mirror_forgets_a_page_the_engine_refused_to_make_without_reading_it() {
    let (h, mirror, loads) = mirrored();
    let listening = h.ctx.lifetime().listener_count();
    let loading = mirror.ensure_loaded(r(40));
    h.refuse(0, "refused");
    settle(loading).unwrap();
    assert_eq!(loads.get(), 0);
    assert_eq!(mirror.get_status(&r(40)).as_str(), "idle");
    assert_eq!(h.ctx.lifetime().listener_count(), listening);
    assert!(h.doc.calls.borrow()[0].change["ops"][0]["type"] == "pages.insertBlank");
}

/// `LocalObjectNumberPool.reserve`: a count that isn't a whole number is
/// refused before the engine hears of it, as the queue's own refusal.
#[test]
fn a_reserve_of_a_fractional_count_is_refused_as_invalid_input() {
    let h = Harness::new();
    let error = settle(h.changes().reserve_object_number_count(1.5)).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
    assert!(error.message.contains("cannot reserve 1.5 object numbers"), "{}", error.message);
    assert!(h.doc.calls.borrow().is_empty());
}
