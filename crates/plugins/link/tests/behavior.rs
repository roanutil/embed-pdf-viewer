//! The typed API beyond what the vectors print, against
//! `packages/plugin/link/src/{controller,source,connect}.ts` and the kernel
//! page mirror it reads through (`packages/core/main/src/page-mirror.ts`):
//! a caller's cancel rejects only that caller while the shared read lands
//! for everyone, a close drops an in-flight read without an error or a
//! loaded event, hidden and noView links are filtered while invisible and
//! grouped ones survive, and destination activation with and without a
//! stage, a missing page and a website opener.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::geometry::{PageRef, Point};
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_link::*;
use epdf_plugin_stage::{STAGE, StageCapability, StagePluginOptions, stage_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn pr(n: u32) -> Value {
    PageRef::new(n).to_json()
}

/// A 600x800 page with its crop at the origin.
fn layout(pon: u32, index: u32) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    json!({
        "index": index,
        "ref": pr(pon),
        "label": null,
        "size": { "width": 600, "height": 800 },
        "rotation": 0,
        "userUnit": 1,
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "tabOrder": null,
    })
}

/// A current link annotation record on page `pon`.
fn link(pon: u32, n: u32, over: Value) -> Value {
    let mut record = json!({
        "subtype": "link",
        "page": pr(pon),
        "ref": { "kind": "objectNumber", "page": pr(pon), "objectNumber": n },
        "rect": { "x": 10 * n, "y": 10, "width": 10, "height": 10 },
        "target": { "kind": "uri", "uri": format!("https://example.com/{n}") },
    });
    for (key, value) in over.as_object().cloned().unwrap_or_default() {
        record[key] = value;
    }
    record
}

fn list(pon: u32, annotations: Vec<Value>) -> Value {
    json!({ "path": "page.annotations.list", "pon": pon, "value": { "annotations": annotations } })
}

fn deferred(pon: u32, label: &str) -> Value {
    json!({ "path": "page.annotations.list", "pon": pon, "times": 1, "defer": label })
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    link: LinkCapability,
    stage: Option<StageCapability>,
    loaded: Rc<RefCell<Vec<Value>>>,
}

impl Rig {
    /// Document `a` with pages 1 and 2 and the given engine responses; with
    /// a stage when `stage` is set.
    fn new(responses: Vec<Value>, stage: bool) -> Rig {
        rt::reset();
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [layout(1, 0), layout(2, 1)],
            "responses": responses,
        } } }));
        let mut plugins: Vec<AnyPlugin> = vec![interaction_plugin(InteractionConfig::default())];
        if stage {
            let mut options = StagePluginOptions::default();
            options.config.settings.view_units_per_point = Some(1.0);
            plugins.push(stage_plugin(options));
        }
        plugins.push(link_plugin());
        let kernel =
            Kernel::new(Rc::new(AsEngine(fake.clone())), plugins, KernelOptions::default())
                .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let link = kernel.capability(&LINK, Some("a")).expect("link");
        let stage = stage.then(|| kernel.capability(&STAGE, Some("a")).expect("stage"));
        let loaded: Rc<RefCell<Vec<Value>>> = Rc::default();
        let heard = loaded.clone();
        let _ = link.on_loaded(move |event| {
            heard.borrow_mut().push(event.page.clone());
            Ok(())
        });
        Rig { kernel, fake, link, stage, loaded }
    }

    /// How many `page.annotations.list` calls the engine saw.
    fn reads(&self) -> usize {
        self.fake
            .log_since(0)
            .iter()
            .filter(|entry| entry["path"] == "page.annotations.list")
            .count()
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn ids(links: &Value) -> Vec<String> {
    links.as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap().to_string()).collect()
}

fn with_signal(signal: &AbortSignal) -> OperationOptions {
    OperationOptions { signal: Some(signal.clone()) }
}

#[test]
fn a_callers_cancel_rejects_only_that_caller_and_the_shared_read_lands() {
    let rig = Rig::new(vec![deferred(1, "first"), list(1, vec![link(1, 1, json!({}))])], false);
    let signal = AbortSignal::new();
    let cancelled = rig.link.ensure_loaded(PageRef::new(1), with_signal(&signal));
    let joined = rig.link.ensure_loaded(PageRef::new(1), OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(rig.reads(), 1, "the second caller joins the read in flight");
    assert_eq!(rig.link.get_status(PageRef::new(1)), ResourceStatus::Loading);

    signal.abort_value(Error::abort(Some(json!("stop"))), json!("stop"));
    let error = settled(&cancelled).expect("cancelled at once").unwrap_err();
    assert_eq!(error.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert!(settled(&joined).is_none(), "the other caller still waits for the read");

    assert!(rig.fake.settle(
        "first",
        Ok(json!({ "annotations": [link(1, 1, json!({})), link(1, 2, json!({}))] }))
    ));
    assert!(settled(&joined).expect("landed").is_ok());
    assert_eq!(rig.reads(), 1);
    assert_eq!(rig.link.get_status(PageRef::new(1)), ResourceStatus::Ready);
    assert_eq!(ids(&rig.link.list_links(PageRef::new(1))), ["obj:1", "obj:2"]);
    // Reference-stable per page while unchanged, as `link.test.ts` checks.
    assert!(Rc::ptr_eq(
        &rig.link.list_links(PageRef::new(1)),
        &rig.link.list_links(PageRef::new(1))
    ));
    assert_eq!(*rig.loaded.borrow(), vec![pr(1)]);

    // Cached: a later call resolves without reading again.
    let again = rig.link.ensure_loaded(PageRef::new(1), OperationOptions::default());
    assert!(settled(&again).expect("cached").is_ok());
    assert_eq!(rig.reads(), 1);
}

#[test]
fn a_signal_already_aborted_rejects_while_the_read_still_fills_the_cache() {
    // `ctx.cancellable(signal, pages.ensureLoaded(ref))`: the read starts as
    // the argument is evaluated, and the mirror's promise has no `abort`.
    let rig = Rig::new(vec![list(1, vec![link(1, 1, json!({}))])], false);
    let signal = AbortSignal::new();
    signal.abort_value(Error::abort(None), json!("gone"));
    let cancelled = rig.link.ensure_loaded(PageRef::new(1), with_signal(&signal));
    let error = settled(&cancelled).expect("settled").unwrap_err();
    assert_eq!(error.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(rig.reads(), 1);
    assert!(rig.link.is_loaded(PageRef::new(1)));
    assert_eq!(ids(&rig.link.list_links(PageRef::new(1))), ["obj:1"]);
}

#[test]
fn list_all_links_rejects_cancelled_and_keeps_what_landed() {
    let rig = Rig::new(
        vec![
            list(1, vec![link(1, 1, json!({}))]),
            deferred(2, "second"),
            list(2, vec![link(2, 7, json!({}))]),
        ],
        false,
    );
    let signal = AbortSignal::new();
    let all = rig.link.list_all_links(with_signal(&signal));
    rt::run_until_stalled();
    assert!(all.peek().is_none());
    signal.abort_value(Error::abort(None), json!("stop"));
    let error = settled(&all).expect("cancelled").unwrap_err();
    assert_eq!(error.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert!(rig.link.is_loaded(PageRef::new(1)));
    assert!(!rig.link.is_loaded(PageRef::new(2)));
    assert!(rig.fake.settle("second", Ok(json!({ "annotations": [link(2, 7, json!({}))] }))));
    rt::run_until_stalled();
    assert!(rig.link.is_loaded(PageRef::new(2)));

    let whole = rig.link.list_all_links(OperationOptions::default());
    assert_eq!(ids(&settled(&whole).unwrap().unwrap()), ["obj:1", "obj:7"]);
    assert_eq!(rig.reads(), 2);
}

#[test]
fn a_close_drops_the_read_in_flight_without_an_error_or_a_loaded_event() {
    let rig = Rig::new(vec![deferred(1, "first")], false);
    let pending = rig.link.ensure_loaded(PageRef::new(1), OperationOptions::default());
    rt::run_until_stalled();
    assert!(pending.peek().is_none());
    let closed = rig.kernel.documents().close("a");
    rt::run_until_stalled();
    assert!(closed.peek().is_some_and(|r| r.is_ok()));
    // `page-mirror.ts`: a read whose cell is gone, or that fails
    // `instance-closed`, settles quietly.
    assert!(settled(&pending).expect("settled at close").is_ok());
    rig.fake.settle("first", Ok(json!({ "annotations": [link(1, 1, json!({}))] })));
    rt::run_until_stalled();
    assert!(rig.loaded.borrow().is_empty());
}

#[test]
fn a_page_not_in_the_document_is_not_found_and_reads_nothing() {
    let rig = Rig::new(Vec::new(), false);
    let error = settled(&rig.link.ensure_loaded(PageRef::new(99), OperationOptions::default()))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.plugin_code(), Some(code::NOT_FOUND));
    assert_eq!(error.message, "page 99 is not in this document");
    let error =
        settled(&rig.link.ensure_loaded(PageTarget::Index(2.0), OperationOptions::default()))
            .unwrap()
            .unwrap_err();
    assert_eq!(error.message, "there is no page 2 in this document");
    let error = rig.link.activate_at(PageTarget::Index(0.5), Point { x: 0.0, y: 0.0 }, None);
    assert_eq!(error.err().unwrap().message, "there is no page 0.5 in this document");
    assert_eq!(rig.reads(), 0);
    assert_eq!(rig.link.list_links(PageRef::new(99)).as_array().unwrap().len(), 0);
    assert_eq!(rig.link.get_status(PageRef::new(99)), ResourceStatus::Idle);
}

#[test]
fn hidden_and_no_view_links_are_filtered_while_invisible_and_grouped_ones_survive() {
    let base = json!({ "kind": "baseIndex", "page": pr(1), "baseIndex": 4 });
    let rig = Rig::new(
        vec![list(
            1,
            vec![
                link(1, 1, json!({ "hidden": true })),
                link(1, 2, json!({ "noView": true })),
                link(1, 3, json!({ "invisible": true })),
                link(
                    1,
                    5,
                    json!({ "reply": { "type": "group", "to": { "kind": "objectNumber", "page": pr(1), "objectNumber": 9 } } }),
                ),
                link(
                    1,
                    6,
                    json!({ "reply": { "type": "reply", "to": { "kind": "objectNumber", "page": pr(1), "objectNumber": 9 } } }),
                ),
                link(1, 4, json!({ "ref": base })),
                link(1, 8, json!({ "target": null })),
                json!({ "subtype": "square", "page": pr(1), "ref": { "kind": "objectNumber", "page": pr(1), "objectNumber": 10 }, "rect": { "x": 0, "y": 0, "width": 1, "height": 1 } }),
            ],
        )],
        false,
    );
    // By index, as `PageRef | number` takes it.
    assert!(
        settled(&rig.link.ensure_loaded(PageTarget::Index(0.0), OperationOptions::default()))
            .unwrap()
            .is_ok()
    );
    let links = rig.link.list_links(PageTarget::Index(0.0));
    assert_eq!(ids(&links), ["obj:3", "obj:5", "obj:6", "base:1:4"]);
    let attached: Vec<bool> =
        links.as_array().unwrap().iter().map(|l| l["attached"].as_bool().unwrap()).collect();
    assert_eq!(attached, [false, true, false, false]);
    assert_eq!(rig.link.get_link(PageRef::new(1), "base:1:4").unwrap()["ref"], base);
    // The members in `source.ts`'s order.
    let keys: Vec<&String> = links[0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["id", "bounds", "target", "ref", "attached"]);
}

#[test]
fn a_goto_reveals_through_the_stage_or_hands_back_its_destination() {
    let destination = json!({ "kind": "fit", "page": pr(2) });
    let goto = json!({ "kind": "goto", "destination": destination });

    let bare = Rig::new(Vec::new(), false);
    let heard: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let log = heard.clone();
    let _ = bare.link.on_activated(move |event| {
        log.borrow_mut().push(event.activation.outcome());
        Ok(())
    });
    match bare.link.activate(&goto, None) {
        LinkActivation::Destination { destination: d } => assert_eq!(d, destination),
        other => panic!("expected a destination, got {}", other.outcome()),
    }
    assert_eq!(
        bare.link.resolve(&goto),
        LinkResolution::Destination { destination: destination.clone() }
    );

    // A page the document doesn't have is reported whole, stage or not.
    let missing = json!({ "kind": "goto", "destination": { "kind": "fit", "page": pr(77) } });
    match bare.link.activate(&missing, None) {
        LinkActivation::Reported { target } => assert_eq!(target, missing),
        other => panic!("expected a report, got {}", other.outcome()),
    }
    // A destination naming its page by index resolves through `getPage`.
    let by_index = json!({ "kind": "goto", "destination": { "kind": "fit", "page": 1 } });
    assert!(matches!(bare.link.resolve(&by_index), LinkResolution::Destination { .. }));
    assert_eq!(*heard.borrow(), ["destination", "reported"]);

    let staged = Rig::new(Vec::new(), true);
    let stage = staged.stage.clone().unwrap();
    stage.set_viewport_size(epdf_geometry::Size { width: 1000.0, height: 700.0 }).unwrap();
    rt::run_until_stalled();
    assert_eq!(stage.get_current_page_index(), 0);
    assert_eq!(staged.link.activate(&goto, None).outcome(), "revealed");
    rt::run_until_stalled();
    assert_eq!(stage.get_current_page_index(), 1);
    assert_eq!(staged.link.activate(&missing, None).outcome(), "reported");
}

#[test]
fn a_website_goes_through_the_latest_opener() {
    let rig = Rig::new(Vec::new(), false);
    let uri = json!({ "kind": "uri", "uri": "javascript:alert(1)" });
    assert_eq!(rig.link.activate(&uri, None).outcome(), "uri");
    let opened: Rc<RefCell<Vec<Value>>> = Rc::default();
    let seen = opened.clone();
    let accept = rig.link.register_uri_opener(Rc::new(move |uri: &Value| {
        seen.borrow_mut().push(uri.clone());
        true
    }));
    let refuse = rig.link.register_uri_opener(Rc::new(|_: &Value| false));
    match rig.link.activate(&uri, None) {
        LinkActivation::Reported { target } => assert_eq!(target, uri),
        other => panic!("expected a report, got {}", other.outcome()),
    }
    refuse.call();
    match rig.link.activate(&uri, None) {
        LinkActivation::Uri { uri: opened_uri } => {
            assert_eq!(opened_uri, json!("javascript:alert(1)"))
        }
        other => panic!("expected uri, got {}", other.outcome()),
    }
    assert_eq!(*opened.borrow(), vec![json!("javascript:alert(1)")]);
    accept.call();
    assert_eq!(rig.link.activate(&uri, None).outcome(), "uri");
    assert_eq!(opened.borrow().len(), 1);
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

/// `promise.then(() => at = turn)`: the turn its reaction runs on.
fn reacts_on<T: Clone + 'static>(promise: Promise<T>, turn: &Rc<Cell<u32>>) -> Rc<Cell<u32>> {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    rt::spawn(async move {
        let _ = promise.await;
        seen.set(turn.get());
    });
    at
}

/// The turns were measured on the TypeScript (the real kernel, link and
/// interaction plugins over the conformance fake engine, whose answer is an
/// already settled `AbortablePromise`), with the counter above and
/// `promise.then(() => at = turn)`: a cached page reacts on 2 (the `async`
/// function's `await` of a settled promise), a page not in the document on
/// 1 (an `async` function that throws before its first `await`), and
/// `onLoaded` fires on 5, as the mirror stores the read.
///
/// The TypeScript's fresh reads react on 10 (`ensureLoaded`), 11 (with a
/// signal) and 12 (`listAllLinks` over two pages); Rust reacts on 7, 8 and
/// 9. The three turns are the `.finally` that `page-mirror.ts:194` chains
/// onto every load, which the kernel's `PageMirror::load`
/// (`crates/epdf-kernel/src/page_mirror.rs:212-280`) settles without. That
/// is the kernel's to repair (Task 13.21 report); these cases join this
/// test when it is.
#[test]
fn loads_settle_on_the_typescripts_turns() {
    let responses = || vec![list(1, vec![link(1, 1, json!({}))]), list(2, Vec::new())];
    let none = OperationOptions::default;

    let rig = Rig::new(responses(), false);
    let turn = turn_counter();
    let missing = reacts_on(rig.link.ensure_loaded(PageRef::new(99), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(missing.get(), 1);

    let rig = Rig::new(responses(), false);
    settled(&rig.link.ensure_loaded(PageRef::new(1), none())).unwrap().unwrap();
    let turn = turn_counter();
    let cached = reacts_on(rig.link.ensure_loaded(PageRef::new(1), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(cached.get(), 2);

    let rig = Rig::new(responses(), false);
    let turn = turn_counter();
    let loaded = Rc::new(Cell::new(u32::MAX));
    let (seen, counter) = (loaded.clone(), turn.clone());
    let _off = rig.link.on_loaded(move |_| {
        seen.set(counter.get());
        Ok(())
    });
    let _ = rig.link.ensure_loaded(PageRef::new(1), none());
    rt::run_until_stalled();
    assert_eq!(loaded.get(), 5);
}
