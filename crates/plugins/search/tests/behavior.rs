//! The typed API beyond what the vectors print, against
//! `packages/plugin/search/src/{controller,model}.ts`: a refusal that
//! rejects before the session changes, snippets denied degrading under one
//! start, a superseded scan that lands nothing, an empty scan, the reveal of
//! a hit's page, `findAll`'s order, the scan's low priority and the turns
//! each await takes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_geometry::Size;
use epdf_kernel::engine::{
    CallFacts, CallPriority, DocumentHandle, DocumentSecurity, Engine, EngineCall,
};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_search::*;
use epdf_plugin_stage::{STAGE, StageCapability, StagePluginOptions, stage_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn pr(n: u32) -> PageRef {
    PageRef::new(n)
}

/// A 300x400 page with its crop at the origin.
fn layout(pon: u32, index: u32) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": 300, "height": 400 });
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null,
        "size": { "width": 300, "height": 400 },
        "rotation": 0,
        "userUnit": 1,
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 300, "top": 400 },
        "tabOrder": null,
    })
}

/// One match on `pon` at `start`, one segment in page space.
fn m(pon: u32, start: u32) -> Value {
    json!({
        "page": pr(pon).to_json(),
        "start": start,
        "count": 4,
        "segments": [{
            "quad": {
                "upperLeft": { "x": 10, "y": 20 }, "upperRight": { "x": 50, "y": 20 },
                "lowerLeft": { "x": 10, "y": 40 }, "lowerRight": { "x": 50, "y": 40 }
            },
            "rect": { "x": 10, "y": 20, "width": 40, "height": 20 },
            "advance": 1
        }]
    })
}

fn slice(matches: Vec<Value>, next: Option<&str>, searched: u32) -> Value {
    json!({ "matches": matches, "nextCursor": next, "pagesSearched": searched, "pageCount": 3 })
}

/// A `search.query` request as `collect` builds it.
fn req(text: &str, snippets: bool, cursor: Option<&str>) -> Value {
    let mut request = json!({ "text": text, "snippets": snippets });
    if let Some(cursor) = cursor {
        request["cursor"] = json!(cursor);
    }
    json!([request])
}

fn answer(text: &str, snippets: bool, cursor: Option<&str>, value: Value) -> Value {
    json!({ "path": "search.query", "args": req(text, snippets, cursor), "value": value })
}

fn defer(text: &str, label: &str) -> Value {
    json!({ "path": "search.query", "args": req(text, true, None), "defer": label })
}

/// Each `call_with`: the path and the facts it carried.
type Facts = Rc<RefCell<Vec<(String, CallFacts)>>>;

/// The fake engine, whose handles record the facts of every `call_with`
/// before answering it as the fake does.
struct Recording {
    fake: Rc<FakeEngine>,
    facts: Facts,
}

impl Engine for Recording {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let facts = self.facts.clone();
        AsEngine(self.fake.clone()).open(input, options).convert(move |inner| {
            Ok(Rc::new(RecordingHandle { inner: inner.clone(), facts: facts.clone() })
                as Rc<dyn DocumentHandle>)
        })
    }

    fn destroy(&self) -> EngineCall<()> {
        AsEngine(self.fake.clone()).destroy()
    }
}

struct RecordingHandle {
    inner: Rc<dyn DocumentHandle>,
    facts: Facts,
}

impl DocumentHandle for RecordingHandle {
    fn id(&self) -> String {
        self.inner.id()
    }
    fn capabilities(&self) -> Value {
        self.inner.capabilities()
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.inner.security()
    }
    fn has(&self, member: &str) -> bool {
        self.inner.has(member)
    }
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.inner.call(path, pon, args)
    }
    fn call_with(
        &self,
        facts: &CallFacts,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
    ) -> EngineCall<Value> {
        self.facts.borrow_mut().push((path.to_string(), facts.clone()));
        self.inner.call_with(facts, path, pon, args)
    }
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.inner.page_index(pon)
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.inner.subscribe(listener)
    }
}

/// What the session's events said, in order.
type Heard = Rc<RefCell<Vec<String>>>;

struct Rig {
    _kernel: Kernel,
    fake: Rc<FakeEngine>,
    search: SearchCapability,
    stage: Option<StageCapability>,
    facts: Facts,
    heard: Heard,
    _hooks: Vec<Unsubscribe>,
}

impl Rig {
    /// Document `a` with three pages (11, 22, 33), `allows` and the
    /// `search.query` `responses`; with a Stage when `stage` is set.
    fn new(allows: Value, responses: Vec<Value>, stage: bool) -> Rig {
        rt::reset();
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "security": { "allows": allows },
            "pages": [layout(11, 0), layout(22, 1), layout(33, 2)],
            "responses": responses,
        } } }));
        let facts: Facts = Rc::default();
        let mut plugins: Vec<AnyPlugin> = Vec::new();
        if stage {
            let mut options = StagePluginOptions::default();
            options.config.settings.view_units_per_point = Some(1.0);
            plugins.push(stage_plugin(options));
        }
        plugins.push(search_plugin(SearchConfig::default()));
        let kernel = Kernel::new(
            Rc::new(Recording { fake: fake.clone(), facts: facts.clone() }),
            plugins,
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
        let search = kernel.capability(&SEARCH, Some("a")).expect("search");
        let stage = stage.then(|| kernel.capability(&STAGE, Some("a")).expect("stage"));
        let heard: Heard = Rc::default();
        let log = |name: &'static str| {
            let heard = heard.clone();
            move |event: &dyn erased::Event| {
                heard.borrow_mut().push(format!("{name} {}", event.json()));
                Ok(())
            }
        };
        let hooks = vec![
            search.on_started({
                let f = log("started");
                move |e| f(e)
            }),
            search.on_progress_changed({
                let f = log("progress");
                move |e| f(e)
            }),
            search.on_completed({
                let f = log("completed");
                move |e| f(e)
            }),
            search.on_cancelled({
                let f = log("cancelled");
                move |e| f(e)
            }),
            search.on_failed({
                let f = log("failed");
                move |e| f(e)
            }),
            search.on_active_hit_changed({
                let f = log("active");
                move |e| f(e)
            }),
            search.on_cleared({
                let f = log("cleared");
                move |e| f(e)
            }),
        ];
        Rig { _kernel: kernel, fake, search, stage, facts, heard, _hooks: hooks }
    }

    fn engine(&self) -> Vec<Value> {
        self.fake.log_since(0)
    }

    /// The `search.query` requests the engine heard, in order.
    fn requests(&self) -> Vec<Value> {
        self.engine()
            .into_iter()
            .filter(|call| call["path"] == "search.query")
            .map(|call| call["args"][0].clone())
            .collect()
    }

    fn heard(&self) -> Vec<String> {
        self.heard.borrow().clone()
    }
}

/// Every event serializes; the log prints it as JSON.
mod erased {
    pub trait Event {
        fn json(&self) -> String;
    }
    impl<T: serde::Serialize> Event for T {
        fn json(&self) -> String {
            serde_json::to_string(self).unwrap()
        }
    }
}

fn q(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), ..Default::default() }
}

fn settled<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::run_until_stalled();
    promise.peek().expect("settled")
}

fn code(error: &Error) -> Option<&str> {
    error.member("code").and_then(Value::as_str)
}

/// `controller.ts:198-203`: `search` is `async`, so `assertAllowed` and
/// `pageOf` reject before the run starts. The session stays as it was, no
/// event fires and the engine hears nothing.
#[test]
fn a_refusal_rejects_before_the_session_changes() {
    let rig = Rig::new(json!(["doc.text.copy"]), Vec::new(), false);
    let before = rig.search.list_hits(None);
    let error = settled(rig.search.search(q("the"), SearchOptions::default())).unwrap_err();
    assert_eq!(code(&error), Some("permission-denied"));
    assert_eq!(error.message, "search requires doc.text.search");
    let error = settled(rig.search.find_all(q("the"), Default::default())).unwrap_err();
    assert_eq!(error.message, "findAll requires doc.text.search");
    assert_eq!(rig.search.get_status(), SearchStatus::Idle);
    assert!(Rc::ptr_eq(&before, &rig.search.list_hits(None)));
    assert!(rig.heard().is_empty());
    assert!(rig.requests().is_empty());

    let rig = Rig::new(json!("*"), Vec::new(), false);
    for (from, message) in [
        (PageTarget::Ref(pr(99)), "page 99 is not in this document"),
        (PageTarget::Index(3.0), "there is no page 3 in this document"),
        (PageTarget::Index(1.5), "there is no page 1.5 in this document"),
    ] {
        let options = SearchOptions { from: Some(from), signal: None };
        let error = settled(rig.search.search(q("the"), options)).unwrap_err();
        assert_eq!((code(&error), error.message.as_str()), (Some("not-found"), message));
    }
    assert_eq!(rig.search.get_status(), SearchStatus::Idle);
    assert!(rig.heard().is_empty());
    assert!(rig.requests().is_empty());
}

/// `controller.ts:160-168`: a refusal of the first request with snippets
/// retries without them inside the same run, so the session starts once and
/// the denial never reaches state.
#[test]
fn denied_snippets_degrade_under_one_start() {
    let denied = json!({
        "path": "search.query", "args": req("the", true, None),
        "error": { "name": "PermissionDenied", "code": "Forbidden", "message": "denied",
            "required": "doc.text.copy", "context": "local" },
    });
    let rig = Rig::new(
        json!("*"),
        vec![denied, answer("the", false, None, slice(vec![m(11, 0)], None, 3))],
        false,
    );
    let result = settled(rig.search.search(q("the"), SearchOptions::default())).unwrap();
    assert_eq!(result, SearchResult { status: SearchResultStatus::Complete, hit_count: 1 });
    assert_eq!(
        rig.requests(),
        [json!({ "text": "the", "snippets": true }), json!({ "text": "the", "snippets": false })]
    );
    let heard = rig.heard();
    assert_eq!(heard.iter().filter(|e| e.starts_with("started")).count(), 1);
    assert!(!heard.iter().any(|e| e.starts_with("failed")));
    assert_eq!(rig.search.get_error(), None);
    assert_eq!(rig.search.get_status(), SearchStatus::Complete);
}

/// `controller.ts:217-218`: a superseded run's slices commit nothing, even
/// when its answer arrives after the newer run started.
#[test]
fn a_superseded_scan_lands_nothing() {
    let rig = Rig::new(json!("*"), vec![defer("old", "old"), defer("new", "new")], false);
    let old = rig.search.search(q("old"), SearchOptions::default());
    rt::run_until_stalled();
    let new = rig.search.search(q("new"), SearchOptions::default());
    rt::run_until_stalled();
    assert!(rig.fake.settle("old", Ok(slice(vec![m(11, 0), m(22, 0)], None, 3))));
    rt::run_until_stalled();
    assert_eq!(old.peek().unwrap().unwrap().status, SearchResultStatus::Superseded);
    assert_eq!(rig.search.get_hit_count(None), 0);
    assert!(rig.fake.settle("new", Ok(slice(vec![m(33, 1)], None, 3))));
    rt::run_until_stalled();
    assert_eq!(new.peek().unwrap().unwrap().hit_count, 1);
    let hits = rig.search.list_hits(None);
    assert_eq!(hits.iter().map(|h| h.page).collect::<Vec<_>>(), [pr(33)]);
    assert_eq!(rig.search.get_query(), Some(q("new")));
    assert_eq!(rig.search.get_hit_count(Some(PageTarget::Ref(pr(11)))), 0);
}

/// An exhausted scan with no matches completes with no active hit: progress
/// moves once, and no active-hit event fires (the index stays `-1`).
#[test]
fn an_empty_scan_completes_with_no_active_hit() {
    let rig =
        Rig::new(json!("*"), vec![answer("none", true, None, slice(Vec::new(), None, 3))], false);
    let result = settled(rig.search.search(q("none"), SearchOptions::default())).unwrap();
    assert_eq!(result, SearchResult { status: SearchResultStatus::Complete, hit_count: 0 });
    assert_eq!(rig.search.get_status(), SearchStatus::Complete);
    assert_eq!(rig.search.get_active_hit_index(), -1);
    assert_eq!(rig.search.get_active_hit(), None);
    assert_eq!(*rig.search.get_progress(), SearchProgress { pages_searched: 3.0, page_count: 3.0 });
    assert!(rig.search.list_pages_with_hits().is_empty());
    assert!(rig.search.next_hit(None).unwrap().is_none());
    assert_eq!(
        rig.heard(),
        [
            r#"started {"query":{"text":"none"}}"#,
            r#"progress {"pagesSearched":3.0,"pageCount":3.0,"hitCount":0}"#,
            r#"completed {"hitCount":0}"#,
        ]
    );
}

/// `controller.ts:269-277`: moving to a hit reveals its page through the
/// Stage at the hit's bounds; the scan started at the Stage's current page.
#[test]
fn navigation_reveals_the_hits_page() {
    let rig = Rig::new(
        json!("*"),
        vec![json!({
            "path": "search.query",
            "args": [{ "text": "the", "snippets": true, "from": pr(11).to_json() }],
            "value": slice(vec![m(11, 0), m(33, 2)], None, 3),
        })],
        true,
    );
    let stage = rig.stage.clone().unwrap();
    stage.set_viewport_size(Size { width: 300.0, height: 200.0 }).unwrap();
    rt::run_until_stalled();
    settled(rig.search.search(q("the"), SearchOptions::default())).unwrap();
    assert_eq!(stage.get_current_page(), Some(pr(11)));
    let instant = SearchRevealOptions {
        anchor: None,
        behavior: Some(Some(epdf_plugin_stage::ScrollBehaviorKind::Instant)),
    };
    let hit = rig.search.next_hit(Some(instant)).unwrap().unwrap();
    rt::run_until_stalled();
    assert_eq!((hit.page, rig.search.get_active_hit_index()), (pr(33), 1));
    assert_eq!(stage.get_current_page(), Some(pr(33)));
    // Revealing again leaves the active hit where it is.
    rig.search.reveal_active_hit(Some(instant)).unwrap();
    assert_eq!(rig.search.get_active_hit_index(), 1);
}

/// `controller.ts:327-345`: `findAll` walks the cursor chain from the start
/// of the document even with a Stage elsewhere, keeps the slices' order, and
/// leaves the session idle.
#[test]
fn find_all_keeps_the_chains_order_and_ignores_the_stage() {
    let rig = Rig::new(
        json!("*"),
        vec![
            answer("the", true, None, slice(vec![m(33, 0), m(11, 4)], Some("c1"), 2)),
            answer("the", true, Some("c1"), slice(vec![m(22, 1), m(11, 0)], None, 3)),
        ],
        true,
    );
    let stage = rig.stage.clone().unwrap();
    stage.set_viewport_size(Size { width: 300.0, height: 200.0 }).unwrap();
    stage.go_to_page(2usize, None);
    rt::run_until_stalled();
    let hits = settled(rig.search.find_all(q("the"), Default::default())).unwrap();
    let order: Vec<(u32, f64, usize)> =
        hits.iter().map(|h| (h.page.page_object_number, h.start, h.page_index)).collect();
    assert_eq!(order, [(33, 0.0, 2), (11, 4.0, 0), (22, 1.0, 1), (11, 0.0, 0)]);
    assert!(rig.requests().iter().all(|r| r.get("from").is_none()));
    assert_eq!(rig.search.get_status(), SearchStatus::Idle);
    assert!(rig.heard().is_empty());
}

/// `controller.ts:154-157` and Ruling13.16: every slice is asked for at low
/// priority, `ctx.doc.with({ priority: 'low' })`.
#[test]
fn every_slice_is_asked_for_at_low_priority() {
    let rig = Rig::new(
        json!("*"),
        vec![
            answer("the", true, None, slice(vec![m(11, 0)], Some("c1"), 1)),
            answer("the", true, Some("c1"), slice(Vec::new(), None, 3)),
        ],
        false,
    );
    settled(rig.search.search(q("the"), SearchOptions::default())).unwrap();
    settled(rig.search.find_all(q("the"), Default::default())).unwrap();
    let facts = rig.facts.borrow();
    assert_eq!(facts.len(), 4);
    for (path, facts) in facts.iter() {
        assert_eq!(path, "search.query");
        assert_eq!(*facts, CallFacts { priority: Some(CallPriority::Low), view: None });
    }
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

/// The turn a promise settles on.
fn settles_on<T: Clone + 'static>(promise: &Promise<T>, turn: &Rc<Cell<u32>>) -> Rc<Cell<u32>> {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    promise.on_settle(move |_| seen.set(turn.get()));
    at
}

/// The turns are counted from the TypeScript, with the fake engine's answer
/// an already settled `AbortablePromise` and the microtask queue first in,
/// first out. The guarded handle (`guarded-handle.ts:70`) settles its plain
/// promise in `Promise.resolve(pending).then(...)`: the resolve-thenable job,
/// its reaction, then the `then` (turn 3).
///
/// - `findAll` with no signal: `ctx.cancellable` hands back the guarded
///   promise, `collect`'s `await` resumes on 4 and returns, and `findAll`'s
///   `await collect(...)` resumes and settles on 5.
/// - With a signal, `cancellable.ts:31` settles its own promise in the
///   guarded promise's `then` (4): `collect` resumes on 5, `findAll` settles
///   on 6.
/// - `search`: the slice commits on 5 as `findAll`'s with a signal does (the
///   run's signal), the run's `await collect` completes the session on 6, the
///   lane's `await fn(run)` settles the run on 7 (`lanes.ts:79`), `.catch`
///   passes it on 8, and `return` from the `async` `search` adopts that
///   promise: its resolve-thenable job ran on 1, so the reaction settles
///   `search` on 9.
#[test]
fn scans_settle_on_the_typescripts_turns() {
    let one = || vec![answer("the", true, None, slice(vec![m(11, 0)], None, 3))];

    let rig = Rig::new(json!("*"), one(), false);
    let turn = turn_counter();
    let found = rig.search.find_all(q("the"), Default::default());
    let at = settles_on(&found, &turn);
    rt::run_until_stalled();
    assert_eq!(at.get(), 5);

    let rig = Rig::new(json!("*"), one(), false);
    let turn = turn_counter();
    let options = SearchFindAllOptions { snippets: None, signal: Some(AbortSignal::new()) };
    let found = rig.search.find_all(q("the"), options);
    let at = settles_on(&found, &turn);
    rt::run_until_stalled();
    assert_eq!(at.get(), 6);

    let rig = Rig::new(json!("*"), one(), false);
    let turn = turn_counter();
    let statuses: Rc<RefCell<Vec<(SearchStatus, usize, u32)>>> = Rc::default();
    let (log, counter, search) = (statuses.clone(), turn.clone(), rig.search.clone());
    let _off = rig.search.on_progress_changed(move |e| {
        log.borrow_mut().push((search.get_status(), e.hit_count, counter.get()));
        Ok(())
    });
    let (completed, counter) = (Rc::new(Cell::new(u32::MAX)), turn.clone());
    let seen = completed.clone();
    let _done = rig.search.on_completed(move |_| {
        seen.set(counter.get());
        Ok(())
    });
    let searched = rig.search.search(q("the"), SearchOptions::default());
    let at = settles_on(&searched, &turn);
    rt::run_until_stalled();
    assert_eq!(*statuses.borrow(), [(SearchStatus::Searching, 1, 5)]);
    assert_eq!(completed.get(), 6);
    assert_eq!(at.get(), 9);
}

/// `cancellable.ts:22`: a caller's signal aborted with a value that is not
/// an error hands the engine that value as given, and `findAll` rejects
/// `operation-cancelled`.
#[test]
fn a_callers_raw_abort_reason_reaches_the_engine_as_given() {
    let rig = Rig::new(json!("*"), vec![defer("the", "slice")], false);
    let signal = AbortSignal::new();
    let options = SearchFindAllOptions { snippets: None, signal: Some(signal.clone()) };
    let found = rig.search.find_all(q("the"), options);
    rt::run_until_stalled();
    signal.abort_value(Error::new("user left"), json!("user left"));
    let error = settled(found).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        (Some("operation-cancelled"), "findAll aborted")
    );
    let aborts: Vec<Value> =
        rig.engine().into_iter().filter(|e| e.get("abort").is_some()).collect();
    assert_eq!(aborts.len(), 1);
    assert_eq!(aborts[0]["reason"], json!("user left"));
}
