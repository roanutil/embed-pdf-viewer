//! The typed API beyond what the vectors print, against
//! `packages/plugin/page-edit/src/{controller,model,state}.ts`: the
//! permission gates and atomic refusals, the history label and undo each
//! verb stages, turns from the rotation a pending change gives a page, and
//! the selection over the confirmed page list.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::changes::ChangeQueue;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, Error, Kernel, KernelOptions, OpenOptions, Promise, Token, define, rt,
};
use epdf_plugin_page_edit::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// A sibling plugin whose capability is its own change queue: what the
/// history sees of each staged change.
static PROBE: LazyLock<Token<ChangeQueue>> = LazyLock::new(|| Token::new("probe"));

fn probe_plugin() -> AnyPlugin {
    define::<Value, Value>("probe")
        .document_scoped()
        .create(&PROBE, |cx| Ok((cx.changes()?, None)))
        .build()
}

fn pr(n: u32) -> PageRef {
    PageRef::new(n)
}

/// `page(n, index, rotation, width, height)` of the suite's fixtures.
fn page(n: u32, index: u32, rotation: u32, width: f64, height: f64) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": width, "height": height });
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": n },
        "label": null,
        "size": { "width": width, "height": height },
        "rotation": rotation,
        "userUnit": 1,
        "tabOrder": null,
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": width, "top": height },
    })
}

fn three() -> Vec<Value> {
    vec![
        page(10, 0, 0, 100.0, 200.0),
        page(20, 1, 90, 300.0, 400.0),
        page(30, 2, 270, 500.0, 600.0),
    ]
}

fn meta() -> Value {
    json!({ "affectedPages": [], "cacheDelta": null, "opId": "x", "undoable": true })
}

/// An `apply` answer with one page item, and its confirmed fact.
fn answer(item_type: &str, fact_type: &str, item: Value, after: Vec<Value>) -> Value {
    let layout = json!({ "pageCount": after.len(), "pages": after, "namedPages": [] });
    let mut item = item;
    item["type"] = json!(item_type);
    item["layout"] = layout;
    item["meta"] = meta();
    let mut fact = item.clone();
    fact["type"] = json!(fact_type);
    fact["origin"] =
        json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null });
    json!({
        "path": "apply",
        "times": 1,
        "value": { "items": [item], "meta": meta() },
        "events": [fact],
    })
}

/// A staged change as the history sees it: label, ops shown, undo.
type Staged = (Value, Vec<Value>, Vec<Value>);

struct Fixture {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    page_edit: PageEditCapability,
    queue: ChangeQueue,
    staged: Rc<RefCell<Vec<Staged>>>,
    /// The engine log's length at [`Fixture::mark`].
    since: Cell<usize>,
}

impl Fixture {
    /// Document `a`, opened.
    fn new(doc: Value) -> Fixture {
        Fixture::over(json!({ "documents": { "a": doc } }), &[])
    }

    /// The engine `spec`, with `a` and then `others` opened.
    fn over(spec: Value, others: &[&str]) -> Fixture {
        rt::reset();
        let fake = FakeEngine::new(spec);
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            vec![page_edit_plugin(), probe_plugin()],
            KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
        )
        .unwrap();
        let _ = kernel.start();
        rt::run_until_stalled();
        for id in std::iter::once(&"a").chain(others) {
            let input = OpenSource::Input(json!({ "kind": "bytes", "id": id }));
            let _ = kernel.documents().open(input, OpenOptions::default());
            rt::run_until_stalled();
        }
        let page_edit = kernel.capability(&PAGE_EDIT, Some("a")).unwrap();
        let staged = Rc::new(RefCell::new(Vec::new()));
        let log = staged.clone();
        let queue = kernel.capability(&PROBE, Some("a")).unwrap();
        // The listener lives as long as the queue's hook.
        let _ = queue.on_staged().on(move |change: &epdf_kernel::changes::PendingChange| {
            log.borrow_mut().push((change.label().clone(), change.shows(), change.undo()));
            Ok(())
        });
        let fixture = Fixture { kernel, fake, page_edit, queue, staged, since: Cell::new(0) };
        fixture.mark();
        fixture
    }

    /// Forget the staged changes and the engine calls so far (the opens').
    fn mark(&self) {
        self.staged.borrow_mut().clear();
        self.since.set(self.fake.log_len());
    }

    /// The engine calls since [`Fixture::mark`], as `path`s.
    fn engine_paths(&self) -> Vec<String> {
        self.fake
            .log_since(self.since.get())
            .iter()
            .filter_map(|call| call.get("path").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    fn labels(&self) -> Vec<Value> {
        self.staged.borrow().iter().map(|(label, _, _)| label.clone()).collect()
    }

    fn pages(&self) -> Vec<(u64, u64)> {
        self.kernel
            .documents()
            .list_pages(Some("a"))
            .iter()
            .map(|p| (p["ref"]["objectNumber"].as_u64().unwrap(), p["rotation"].as_u64().unwrap()))
            .collect()
    }
}

fn doc(responses: Vec<Value>, allows: Value) -> Value {
    json!({ "pages": three(), "security": { "allows": allows }, "responses": responses })
}

fn settled<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::run_until_stalled();
    promise.peek().expect("the verb settled")
}

fn code(error: &Error) -> Option<&str> {
    error.member("code").and_then(Value::as_str)
}

fn refs(pages: &[u32]) -> Vec<PageArg> {
    pages.iter().map(|n| PageArg::Ref(pr(*n))).collect()
}

#[test]
fn without_assemble_every_write_refuses_before_the_engine_hears_of_it() {
    let f = Fixture::new(doc(Vec::new(), json!(["doc.render", "doc.download"])));
    let pe = &f.page_edit;
    assert!(!pe.can_edit());
    assert!(pe.can_extract());
    let none = OperationOptions::default;
    let refusals: Vec<(&str, Result<(), Error>)> = vec![
        ("pageEdit.rotateBy", settled(pe.rotate_by(refs(&[10]), 90.0, none()))),
        ("pageEdit.setRotation", settled(pe.set_rotation(refs(&[10]), 90.0, none()))),
        ("pageEdit.reorder", settled(pe.reorder(refs(&[10]), PagePosition::End, none()))),
        ("pageEdit.delete", settled(pe.delete(refs(&[10]), none()))),
        ("pageEdit.insertBlank", settled(pe.insert_blank(Default::default())).map(|_| ())),
        (
            "pageEdit.insertFromBytes",
            settled(pe.insert_from_bytes(Bytes(vec![1]), Default::default())).map(|_| ()),
        ),
        (
            "pageEdit.insertFromDocument",
            settled(pe.insert_from_document("a".into(), refs(&[10]), Default::default()))
                .map(|_| ()),
        ),
        ("pageEdit.duplicate", settled(pe.duplicate(refs(&[10]), Default::default())).map(|_| ())),
    ];
    for (operation, result) in refusals {
        let error = result.expect_err(operation);
        assert_eq!(code(&error), Some("permission-denied"), "{operation}");
        assert_eq!(error.message, format!("{operation} requires doc.pages.assemble"));
        assert_eq!(error.member("permission"), Some(&json!("doc.pages.assemble")));
    }
    assert!(f.engine_paths().is_empty(), "{:?}", f.engine_paths());
    assert!(f.labels().is_empty());
}

#[test]
fn without_download_copying_pages_out_refuses() {
    let f = Fixture::new(doc(Vec::new(), json!(["doc.pages.assemble"])));
    let pe = &f.page_edit;
    assert!(pe.can_edit() && !pe.can_extract());
    let error = settled(pe.extract(refs(&[10]), Default::default())).unwrap_err();
    assert_eq!(error.message, "pageEdit.extract requires doc.download");
    let error = settled(pe.duplicate(refs(&[10]), Default::default())).unwrap_err();
    assert_eq!(error.message, "pageEdit.duplicate requires doc.download");
    assert!(f.engine_paths().is_empty());
    assert!(f.labels().is_empty());
}

#[test]
fn one_bad_page_refuses_the_whole_edit() {
    let f = Fixture::new(doc(Vec::new(), json!("*")));
    let pe = &f.page_edit;
    let none = OperationOptions::default;
    let error = settled(pe.rotate_by(refs(&[10, 99]), 90.0, none())).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        (Some("not-found"), "page 99 is not in this document")
    );
    let error = settled(pe.rotate_by(vec![PageArg::Index(3.0)], 90.0, none())).unwrap_err();
    assert_eq!(error.message, "there is no page 3 in this document");
    let error = settled(pe.reorder(refs(&[10]), PagePosition::After(PageArg::Ref(pr(99))), none()))
        .unwrap_err();
    assert_eq!(error.message, "page 99 is not in this document");
    let error = settled(pe.delete(refs(&[10, 20, 30]), none())).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        (Some("invalid-input"), "a document keeps at least one page")
    );
    let error = settled(pe.delete(Vec::new(), none())).unwrap_err();
    assert_eq!(error.message, "no pages given");
    assert!(f.engine_paths().is_empty());
    assert!(f.labels().is_empty());
    assert_eq!(f.pages(), [(10, 0), (20, 90), (30, 270)]);
}

#[test]
fn a_cancelled_signal_refuses_before_anything_is_staged() {
    let f = Fixture::new(doc(Vec::new(), json!("*")));
    let signal = AbortSignal::new();
    signal.abort(Error::new("stop"));
    let options = OperationOptions { signal: Some(signal) };
    let error = settled(f.page_edit.rotate_by(refs(&[10]), 90.0, options)).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"));
    assert_eq!(error.message, "pageEdit.rotateBy was cancelled");
    assert!(f.engine_paths().is_empty());
    assert!(f.labels().is_empty());
}

#[test]
fn each_edit_is_one_labelled_change_with_its_undo() {
    let f = Fixture::new(doc(
        vec![
            json!({ "path": "apply", "value": { "items": [{ "insertedPages": [] }], "meta": meta() } }),
            json!({ "path": "pages.extract", "value": { "$bytes": "CgsM" } }),
        ],
        json!("*"),
    ));
    let pe = &f.page_edit;
    let none = OperationOptions::default;
    // Pages 10 (0°) and 20 (90°) turn to 90° and 180°: two ops, and the
    // undo turns each back.
    settled(pe.rotate_by(refs(&[10, 20]), 90.0, none())).unwrap();
    settled(pe.set_rotation(refs(&[10, 30]), 180.0, none())).unwrap();
    settled(pe.reorder(vec![PageArg::Index(2.0)], PagePosition::Start, none())).unwrap();
    settled(pe.delete(refs(&[20]), none())).unwrap();
    let blank =
        settled(pe.insert_blank(InsertBlankOptions { count: Some(3.0), ..Default::default() }));
    assert_eq!(blank.unwrap(), PageEditInsertResult::default());
    let copied = settled(pe.insert_from_bytes(Bytes(vec![1, 2, 3]), Default::default()));
    assert_eq!(copied.unwrap(), PageEditInsertResult::default());
    settled(pe.duplicate(refs(&[10]), Default::default())).unwrap();
    assert_eq!(
        f.labels(),
        [
            json!({ "key": "pages.rotate", "count": 2 }),
            json!({ "key": "pages.rotate", "count": 2 }),
            json!({ "key": "pages.reorder", "count": 1 }),
            json!({ "key": "pages.delete", "count": 1 }),
            json!({ "key": "pages.insertBlank", "count": 3 }),
            json!({ "key": "pages.insert" }),
            json!({ "key": "pages.duplicate", "count": 1 }),
        ]
    );
    let staged = f.staged.borrow();
    let rotate = |pages: &[u32], rotation: u32| json!({ "type": "pages.rotate", "pages": pages.iter().map(|n| pr(*n).to_json()).collect::<Vec<_>>(), "rotation": rotation });
    assert_eq!(staged[0].1, [rotate(&[10], 90), rotate(&[20], 180)]);
    assert_eq!(staged[0].2, [rotate(&[10], 0), rotate(&[20], 90)]);
    assert_eq!(staged[1].1, [rotate(&[10, 30], 180)]);
    assert_eq!(staged[1].2, [rotate(&[10], 0), rotate(&[30], 270)]);
    // The index named page 30 when the verb was called.
    assert_eq!(
        staged[2].1,
        [json!({ "type": "pages.reorder", "pages": [pr(30).to_json()], "position": "start" })]
    );
    // A blank page matches the page it goes next to (the last, by default).
    assert_eq!(
        staged[4].1,
        [
            json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 500.0, "height": 600.0 }, "count": 3 }, "position": "end" })
        ]
    );
    // Only rotations carry an undo of their own; the engine records the rest.
    assert!(staged[2..].iter().all(|(_, _, undo)| undo.is_empty()));
    // A duplicate lands right after the last page it copies.
    assert_eq!(staged[6].1[0]["position"], json!({ "after": pr(10).to_json() }));
}

#[test]
fn two_quick_turns_are_a_half_turn() {
    let f = Fixture::new(doc(
        vec![
            json!({ "path": "apply", "times": 1, "defer": "first" }),
            json!({ "path": "apply", "times": 1, "defer": "second" }),
        ],
        json!("*"),
    ));
    let none = OperationOptions::default;
    let first = f.page_edit.rotate_by(refs(&[20]), 90.0, none());
    let second = f.page_edit.rotate_by(refs(&[20]), 90.0, none());
    rt::run_until_stalled();
    let staged = f.staged.borrow();
    assert_eq!(staged[0].1[0]["rotation"], json!(180));
    // The second turn starts from where the first one leaves the page.
    assert_eq!(staged[1].1[0]["rotation"], json!(270));
    assert_eq!(staged[1].2[0]["rotation"], json!(180));
    assert!(!first.is_settled() && !second.is_settled());
}

#[test]
fn the_selection_follows_the_confirmed_pages() {
    let deleted = answer(
        "pages.delete",
        "pages.deleted",
        json!({ "pages": [pr(20).to_json()] }),
        vec![page(10, 0, 0, 100.0, 200.0), page(30, 1, 270, 500.0, 600.0)],
    );
    let f = Fixture::new(doc(vec![deleted], json!("*")));
    let pe = &f.page_edit;
    pe.select(&[
        PageArg::Index(1.0),
        PageArg::Ref(pr(30)),
        PageArg::Ref(pr(30)),
        PageArg::Ref(pr(99)),
    ]);
    let selected = pe.get_selected();
    assert_eq!(*selected, [pr(20), pr(30)]);
    // Nothing changed, so the same list.
    assert!(Rc::ptr_eq(&selected, &pe.get_selected()));
    assert_eq!(*page_edit_state(Some(pe)).selected, [pr(20), pr(30)]);
    settled(pe.delete(refs(&[20]), OperationOptions::default())).unwrap();
    // Page 20 went: it drops out of the read, and the layout is the engine's.
    assert_eq!(*pe.get_selected(), [pr(30)]);
    assert_eq!(f.pages(), [(10, 0), (30, 270)]);
    pe.select(&[]);
    assert!(pe.get_selected().is_empty());
    assert_eq!(page_edit_state(None), PageEditStateView::default());
}

#[test]
fn an_insert_resolves_the_pages_the_engine_names() {
    let inserted = answer(
        "pages.insertBlank",
        "pages.inserted",
        json!({ "insertedPages": [pr(99).to_json()] }),
        {
            let mut pages = three();
            pages.push(page(99, 3, 0, 500.0, 600.0));
            pages
        },
    );
    let f = Fixture::new(doc(vec![inserted], json!("*")));
    let result = settled(f.page_edit.insert_blank(Default::default())).unwrap();
    assert_eq!(result.page_refs(), [pr(99)]);
    assert_eq!(f.pages().last(), Some(&(99, 0)));
}

/// A task that counts turns: each one is a JavaScript `await null` loop,
/// started before the verbs, so it runs first in every turn.
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

/// The turn each change is staged on, by label key.
fn staged_turns(f: &Fixture, turn: &Rc<Cell<u32>>) -> Rc<RefCell<Vec<(String, u32)>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (log, turn) = (seen.clone(), turn.clone());
    let _ = f.queue.on_staged().on(move |change: &epdf_kernel::changes::PendingChange| {
        let key = change.label()["key"].as_str().unwrap_or_default().to_string();
        log.borrow_mut().push((key, turn.get()));
        Ok(())
    });
    seen
}

fn racing_engine() -> Value {
    let applied =
        json!({ "path": "apply", "value": { "items": [{ "insertedPages": [] }], "meta": meta() } });
    json!({
        "defaultDocument": "src",
        "documents": {
            "a": doc(vec![applied], json!("*")),
            "b": doc(vec![json!({ "path": "pages.extract", "value": { "$bytes": "BwgJ" } })], json!("*")),
            "src": doc(vec![json!({ "path": "pages.extract", "value": { "$bytes": "BAUG" } })], json!("*")),
        },
    })
}

/// The turns are counted from the TypeScript, with the fake engine's
/// answers already settled `AbortablePromise`s and the microtask queue
/// first in, first out:
///
/// - `insertFromDocument` (`controller.ts:341`) awaits
///   `mapErrors(() => other.pages.extract(refs))`. Inside, `return await`
///   on the subclass promise takes the resolve-thenable job, its `then`
///   reaction and the await's own (3); the verb's `await mapErrors(...)` is
///   the 4th, and `insertBytes` stages there.
/// - `insertFromBytes` with `pageIndexes` awaits `pagesOfBytes` (`:221`):
///   `await mapErrors(open)` 4, `await mapErrors(pages.list)` 4,
///   `return await mapErrors(pages.extract)` 4, `finally { await
///   source.close() }` 3 (a subclass), then the verb's `await
///   pagesOfBytes(...)` 1: it stages on turn 16.
/// - With a signal, `ctx.cancellable` (`cancellable.ts:20`) returns a plain
///   promise its `task.then` reaction settles (1), `mapErrors` resumes on
///   the next (2) and the verb on the one after: turn 3.
#[test]
fn await_first_verbs_stage_on_the_typescripts_turns() {
    let f = Fixture::over(racing_engine(), &["b"]);
    let turn = turn_counter();
    let seen = staged_turns(&f, &turn);
    let options = InsertFromBytesOptions { page_indexes: Some(vec![1.0]), ..Default::default() };
    let subset = f.page_edit.insert_from_bytes(Bytes(vec![1, 2, 3]), options);
    let copy = f.page_edit.insert_from_document("b".into(), refs(&[20]), Default::default());
    rt::run_until_stalled();
    assert_eq!(*seen.borrow(), [("pages.insert".to_string(), 4), ("pages.insert".to_string(), 16)]);
    // Both calls reach the engine at once (the open, b's extract); on turn
    // 4 the subset lists its source, then the copy is sent; the subset
    // extracts on 8 and closes on 12.
    assert_eq!(
        f.engine_paths(),
        ["open", "pages.extract", "pages.list", "apply", "pages.extract", "close", "apply"]
    );
    assert!(subset.peek().is_some_and(|r| r.is_ok()) && copy.peek().is_some_and(|r| r.is_ok()));

    let f = Fixture::over(racing_engine(), &["b"]);
    let turn = turn_counter();
    let seen = staged_turns(&f, &turn);
    let options = PagePositionOptions { signal: Some(AbortSignal::new()), ..Default::default() };
    let copy = f.page_edit.insert_from_document("b".into(), refs(&[20]), options);
    rt::run_until_stalled();
    assert_eq!(*seen.borrow(), [("pages.insert".to_string(), 3)]);
    assert!(copy.peek().is_some_and(|r| r.is_ok()));
}

/// `controller.ts:60-62` (contract): a signal that fires after the change
/// was staged rejects the verb `operation-cancelled`, and the change still
/// applies.
#[test]
fn a_signal_fired_mid_flight_rejects_the_verb_and_the_edit_still_lands() {
    let mut deferred = answer(
        "pages.rotate",
        "pages.rotated",
        json!({ "pages": [pr(10).to_json()], "rotation": 90 }),
        vec![
            page(10, 0, 90, 100.0, 200.0),
            page(20, 1, 90, 300.0, 400.0),
            page(30, 2, 270, 500.0, 600.0),
        ],
    );
    let value = deferred["value"].take();
    deferred.as_object_mut().unwrap().remove("value");
    deferred["defer"] = json!("rotate");
    let f = Fixture::new(doc(vec![deferred], json!("*")));
    let signal = AbortSignal::new();
    let options = OperationOptions { signal: Some(signal.clone()) };
    let rotate = f.page_edit.rotate_by(refs(&[10]), 90.0, options);
    rt::run_until_stalled();
    assert_eq!(f.labels().len(), 1);
    assert!(!rotate.is_settled());
    signal.abort(Error::new("stop"));
    rt::run_until_stalled();
    let error = rotate.peek().unwrap().unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        (Some("operation-cancelled"), "operation cancelled")
    );
    assert_eq!(f.pages()[0], (10, 0), "nothing shows before the engine answers");
    assert!(f.fake.settle("rotate", Ok(value)));
    rt::run_until_stalled();
    assert_eq!(f.pages()[0], (10, 90));
    assert_eq!(f.engine_paths(), ["apply"]);
}

/// `inserted` (`controller.ts:149`) reads `'insertedPages' in item`: an
/// array item answers `false` and resolves no pages; a primitive throws.
#[test]
fn an_insert_answer_item_is_read_with_the_in_operator() {
    let items = |item: Value| json!({ "path": "apply", "times": 1, "value": { "items": [item], "meta": meta() } });
    let f = Fixture::new(doc(vec![items(json!([])), items(json!(5))], json!("*")));
    let none = settled(f.page_edit.insert_blank(Default::default())).unwrap();
    assert_eq!(none, PageEditInsertResult::default());
    let error = settled(f.page_edit.insert_blank(Default::default())).unwrap_err();
    assert_eq!(error.name, "TypeError");
    assert_eq!(error.message, "Cannot use 'in' operator to search for 'insertedPages' in 5");
}
