//! The typed API beyond what the vectors print, against
//! `packages/plugin/page-edit/src/{controller,model,state}.ts`: the
//! permission gates and atomic refusals, the history label and undo each
//! verb stages, turns from the rotation a pending change gives a page, and
//! the selection over the confirmed page list.

use std::cell::RefCell;
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
    staged: Rc<RefCell<Vec<Staged>>>,
}

impl Fixture {
    fn new(doc: Value) -> Fixture {
        rt::reset();
        let fake = FakeEngine::new(json!({ "documents": { "a": doc } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            vec![page_edit_plugin(), probe_plugin()],
            KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
        )
        .unwrap();
        let _ = kernel.start();
        rt::run_until_stalled();
        let _ = kernel
            .documents()
            .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
        rt::run_until_stalled();
        let page_edit = kernel.capability(&PAGE_EDIT, Some("a")).unwrap();
        let staged = Rc::new(RefCell::new(Vec::new()));
        let log = staged.clone();
        let queue = kernel.capability(&PROBE, Some("a")).unwrap();
        // The listener lives as long as the queue's hook.
        let _ = queue.on_staged().on(move |change: &epdf_kernel::changes::PendingChange| {
            log.borrow_mut().push((change.label().clone(), change.shows(), change.undo()));
            Ok(())
        });
        let fixture = Fixture { kernel, fake, page_edit, staged };
        fixture.mark();
        fixture
    }

    /// Forget the engine calls so far (the open's).
    fn mark(&self) {
        self.staged.borrow_mut().clear();
    }

    /// The engine calls after the open, as `path`s.
    fn engine_paths(&self) -> Vec<String> {
        self.fake
            .log_since(0)
            .iter()
            .filter_map(|call| call.get("path").and_then(Value::as_str).map(str::to_string))
            .filter(|path| !matches!(path.as_str(), "open" | "pages.list" | "render.getPolicy"))
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
    let _ = settled(pe.rotate_by(refs(&[10, 20]), 90.0, none()));
    let _ = settled(pe.set_rotation(refs(&[10, 30]), 180.0, none()));
    let _ = settled(pe.reorder(vec![PageArg::Index(2.0)], PagePosition::Start, none()));
    let _ = settled(pe.delete(refs(&[20]), none()));
    let _ = settled(pe.insert_blank(InsertBlankOptions { count: Some(3.0), ..Default::default() }));
    let _ = settled(pe.insert_from_bytes(Bytes(vec![1, 2, 3]), Default::default()));
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
