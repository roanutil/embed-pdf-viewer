//! The behaviour Task 13.24 pins beyond the printed traces, against
//! `packages/plugin/stamp/src/{services/*,read/catalog,write/*}.ts` and the
//! kernel's serial queue (`packages/core/main/src/serial-queue.ts`): the
//! turns each verb settles on, a queued mutation cancelled before it
//! starts, a cancel after the rewrite that keeps nothing, per-library queue
//! isolation, a mutation in flight at teardown that completes (distinct
//! from one cancelled), ghost renders cached per width, a library's assets
//! going with it, and a setting change that re-resolves the asset engine.
//! No vector passes a signal or changes a setting.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_stamp::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn bytes(text: &str) -> Value {
    epdf_kernel::bytes::Bytes(text.as_bytes().to_vec()).to_json()
}

fn layout(index: u32, on: u32) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": 200, "height": 100 });
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": on },
        "label": null,
        "size": { "width": 200, "height": 100 },
        "rotation": 0,
        "userUnit": 1,
        "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 200, "top": 100 },
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
    })
}

/// The local library document every open answers: one page, 100, plus 900
/// for an inserted page, and an answer for every call the plugin makes.
/// The turn test's TypeScript twin answers the same table.
fn library(extra: Vec<Value>) -> Value {
    let pages = vec![layout(0, 100), layout(1, 900)];
    let meta = json!({ "affectedPages": [], "cacheDelta": null, "opId": "w", "undoable": true });
    let listing = json!({ "pageCount": 2, "pages": pages, "namedPages": [] });
    let mut responses = extra;
    responses.extend([
        json!({ "path": "pages.list", "value": listing }),
        json!({ "path": "metadata.get", "value": { "title": null } }),
        json!({ "path": "metadata.update", "value": { "metadata": { "title": "x" }, "meta": meta } }),
        json!({ "path": "pieceInfo.get", "value": null }),
        json!({ "path": "pieceInfo.update", "value": { "pieceInfo": { "entries": {}, "modifiedAt": null }, "meta": meta } }),
        json!({ "path": "page.pieceInfo.get", "value": null }),
        json!({ "path": "page.pieceInfo.update", "value": { "pieceInfo": { "entries": {}, "modifiedAt": null }, "meta": meta } }),
        json!({ "path": "pages.setName", "value": { "layout": listing, "meta": meta } }),
        json!({ "path": "pages.extract", "value": bytes("%PDF-page") }),
        json!({ "path": "pages.insert", "value": { "insertedPages": [{ "kind": "objectNumber", "objectNumber": 900 }], "layout": {}, "meta": meta } }),
        json!({ "path": "pages.delete", "value": { "layout": {}, "meta": meta } }),
        json!({ "path": "page.render.image", "value": { "contentType": "image/png", "source": { "kind": "bytes", "bytes": bytes("png") } } }),
        json!({ "path": "download", "value": bytes("%PDF-canonical") }),
    ]);
    json!({ "local": true, "pages": pages, "responses": responses })
}

/// The fake engine with the local brand, as a native host's engine is.
struct Branded(AsEngine);

impl epdf_kernel::engine::Engine for Branded {
    fn open(
        &self,
        input: Value,
        options: Option<Value>,
    ) -> epdf_kernel::engine::EngineCall<Rc<dyn epdf_kernel::engine::DocumentHandle>> {
        epdf_kernel::engine::Engine::open(&self.0, input, options)
    }
    fn destroy(&self) -> epdf_kernel::engine::EngineCall<()> {
        epdf_kernel::engine::Engine::destroy(&self.0)
    }
    fn is_local(&self) -> bool {
        true
    }
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    stamp: StampCapability,
    reports: Rc<RefCell<Vec<Error>>>,
}

impl Rig {
    /// A kernel over the fake engine, with `opens` answered first, then the
    /// library document for any other open, and the stamp plugin over a
    /// local port of the same engine (`local` false: none configured).
    fn with(document: Value, opens: Vec<Value>, local: bool) -> Rig {
        Rig::over(document, opens, local, false)
    }

    /// [`Rig::with`], the kernel's engine carrying the local brand when
    /// `branded`.
    fn over(document: Value, opens: Vec<Value>, local: bool, branded: bool) -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<Error>>> = Rc::default();
        let heard = reports.clone();
        let fake = FakeEngine::new(json!({
            "documents": { "lib": document },
            "defaultDocument": "lib",
            "responses": opens,
        }));
        let engine: Rc<dyn epdf_kernel::engine::Engine> = if branded {
            Rc::new(Branded(AsEngine(fake.clone())))
        } else {
            Rc::new(AsEngine(fake.clone()))
        };
        let config = StampConfig {
            asset_engine: local
                .then(|| Some(StampAssetEngine::Engine(LocalEngine::new(engine.clone())))),
            ..StampConfig::default()
        };
        let kernel = Kernel::new(
            engine,
            vec![stamp_plugin(config)],
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| heard.borrow_mut().push(error.clone()))),
                ..KernelOptions::default()
            },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        let stamp = kernel.capability(&STAMP, None).expect("stamp");
        Rig { kernel, fake, stamp, reports }
    }

    fn new() -> Rig {
        Rig::with(library(Vec::new()), Vec::new(), true)
    }

    fn calls_to(&self, path: &str) -> usize {
        self.fake.log_since(0).iter().filter(|entry| entry["path"] == path).count()
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

fn with_signal(signal: &AbortSignal) -> OperationOptions {
    OperationOptions { signal: Some(signal.clone()) }
}

fn pdf() -> Value {
    bytes("%PDF-1.7\n")
}

/// A task that counts turns, each an `await undefined`, started before the
/// verb so it runs first in every turn.
fn turn_counter() -> Rc<Cell<u32>> {
    let turn = Rc::new(Cell::new(0));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..200 {
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

/// The turns are the TypeScript's, measured with the shipping kernel and
/// stamp plugin over `tooling/plugin-conformance/src/fake-engine.ts`
/// answering this table, a local port that hands back the fake's own
/// `open`, a counter of `await undefined` steps started before the verb,
/// and the turn read in `promise.then`: createLibrary 18, updateLibrary 21,
/// deleteLibrary 5 (an unknown one 5 too: the queue runs either way),
/// exportLibrary 1, importLibrary 66, createAsset of a PDF 37, deleteAsset
/// 18 (an unknown one 2: no queue), renderAssetPreview 17.
#[test]
fn verbs_settle_on_the_typescripts_turns() {
    let rig = Rig::new();
    let turn = turn_counter();
    let created = rig
        .stamp
        .create_library("A", CreateLibraryOptions { id: Some("a".into()), ..Default::default() });
    assert_eq!(reacts_on(&created, &turn), 18, "createLibrary");

    let library = |rig: &Rig| {
        success(&rig.stamp.create_library(
            "A",
            CreateLibraryOptions { id: Some("a".into()), ..Default::default() },
        ));
    };

    let rig = Rig::new();
    library(&rig);
    let turn = turn_counter();
    let patch = StampLibraryPatch { name: Some("B".into()), ..Default::default() };
    assert_eq!(
        reacts_on(&rig.stamp.update_library("a", patch, none()), &turn),
        21,
        "updateLibrary"
    );

    let rig = Rig::new();
    library(&rig);
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.stamp.delete_library("a", none()), &turn), 5, "deleteLibrary");

    let rig = Rig::new();
    let turn = turn_counter();
    assert_eq!(
        reacts_on(&rig.stamp.delete_library("nope", none()), &turn),
        5,
        "deleteLibrary unknown"
    );

    let rig = Rig::new();
    library(&rig);
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.stamp.export_library("a", none()), &turn), 1, "exportLibrary");

    let rig = Rig::new();
    let turn = turn_counter();
    let imported = rig.stamp.import_library(pdf(), ImportLibraryOptions::default());
    assert_eq!(reacts_on(&imported, &turn), 66, "importLibrary");

    let rig = Rig::new();
    library(&rig);
    let turn = turn_counter();
    let input = AddAssetInput {
        library_id: Some("a".into()),
        name: Some("X".into()),
        source: Some(pdf()),
        ..Default::default()
    };
    assert_eq!(reacts_on(&rig.stamp.create_asset(input, none()), &turn), 37, "createAsset");

    let rig = Rig::new();
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let id = rig.stamp.list_assets(None)[0].id.clone();
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.stamp.delete_asset(&id, none()), &turn), 18, "deleteAsset");

    let rig = Rig::new();
    let turn = turn_counter();
    assert_eq!(reacts_on(&rig.stamp.delete_asset("nope", none()), &turn), 2, "deleteAsset unknown");

    let rig = Rig::new();
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let id = rig.stamp.list_assets(None)[0].id.clone();
    let turn = turn_counter();
    let preview = rig.stamp.render_asset_preview(&id, 64.0, none());
    assert_eq!(reacts_on(&preview, &turn), 17, "renderAssetPreview");
}

/// The open the second mutation would make, held: the first `open` (the
/// setup's library) is answered at once, the next waits for `hold`.
fn held_open() -> Vec<Value> {
    vec![
        json!({ "path": "open", "times": 1, "value": { "$document": "lib" } }),
        json!({ "path": "open", "times": 1, "defer": "hold", "value": { "$document": "lib" } }),
    ]
}

/// `serial-queue.ts`: a mutation whose signal fired while it waited never
/// starts; it rejects `operation-cancelled` and opens nothing. The one ahead
/// of it finishes as it would have.
#[test]
fn a_queued_mutation_cancelled_while_it_waits_never_starts() {
    let rig = Rig::with(library(Vec::new()), held_open(), true);
    success(
        &rig.stamp.create_library(
            "A",
            CreateLibraryOptions { id: Some("a".into()), ..Default::default() },
        ),
    );
    let first = rig.stamp.update_library(
        "a",
        StampLibraryPatch { name: Some("B".into()), ..Default::default() },
        none(),
    );
    let signal = AbortSignal::new();
    let second = rig.stamp.update_library(
        "a",
        StampLibraryPatch { name: Some("C".into()), ..Default::default() },
        with_signal(&signal),
    );
    rt::run_until_stalled();
    assert_eq!(rig.calls_to("open"), 2, "the first update holds the queue");
    signal.abort(Error::new("caller gave up"));
    assert!(rig.fake.settle("hold", Ok(json!({ "$document": "lib" }))));
    assert_eq!(success(&first).name, "B");
    let refused = failure(&second);
    assert_eq!(refused.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(refused.message, "cancelled before it started");
    assert_eq!(rig.calls_to("open"), 2, "the cancelled update never opened the library");
    assert_eq!(rig.stamp.get_library("a").unwrap().name, "B");
}

/// A cancel that lands after the rewrite: the work runs and the document
/// closes, then nothing is kept. The library, its bytes and its events stay
/// as they were.
#[test]
fn a_cancel_after_the_rewrite_keeps_nothing() {
    // The create's download answers at once, the rewrite's waits for `save`.
    let downloads = vec![
        json!({ "path": "download", "times": 1, "value": bytes("%PDF-canonical") }),
        json!({ "path": "download", "times": 1, "defer": "save", "value": bytes("%PDF-renamed") }),
    ];
    let rig = Rig::with(library(downloads), Vec::new(), true);
    success(
        &rig.stamp.create_library(
            "A",
            CreateLibraryOptions { id: Some("a".into()), ..Default::default() },
        ),
    );
    let before = success(&rig.stamp.export_library("a", none()));
    let updated = Rc::new(Cell::new(0));
    let heard = updated.clone();
    let _off = rig.stamp.on_library_updated(move |_| {
        heard.set(heard.get() + 1);
        Ok(())
    });
    let signal = AbortSignal::new();
    let update = rig.stamp.update_library(
        "a",
        StampLibraryPatch { name: Some("Renamed".into()), ..Default::default() },
        with_signal(&signal),
    );
    rt::run_until_stalled();
    assert!(settled(&update).is_none(), "the rewrite waits for its download");
    signal.abort(Error::new("caller gave up"));
    assert!(rig.fake.settle("save", Ok(bytes("%PDF-renamed"))));
    let refused = failure(&update);
    assert_eq!(refused.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert_eq!(rig.stamp.get_library("a").unwrap().name, "A");
    assert_eq!(success(&rig.stamp.export_library("a", none())), before);
    assert_eq!(updated.get(), 0);
    assert_eq!(rig.calls_to("close"), 2, "the rewrite's document still closed");
}

/// One library's held rewrite doesn't hold another's: the queues are per
/// library.
#[test]
fn each_library_has_its_own_queue() {
    let opens = vec![
        json!({ "path": "open", "times": 2, "value": { "$document": "lib" } }),
        json!({ "path": "open", "times": 1, "defer": "hold", "value": { "$document": "lib" } }),
    ];
    let rig = Rig::with(library(Vec::new()), opens, true);
    for id in ["a", "b"] {
        success(&rig.stamp.create_library(
            id,
            CreateLibraryOptions { id: Some(id.into()), ..Default::default() },
        ));
    }
    let held = rig.stamp.update_library(
        "a",
        StampLibraryPatch { name: Some("A2".into()), ..Default::default() },
        none(),
    );
    let free = rig.stamp.update_library(
        "b",
        StampLibraryPatch { name: Some("B2".into()), ..Default::default() },
        none(),
    );
    assert_eq!(success(&free).name, "B2");
    assert!(settled(&held).is_none(), "library a's rewrite is still waiting for its open");
    assert!(rig.fake.settle("hold", Ok(json!({ "$document": "lib" }))));
    assert_eq!(success(&held).name, "A2");
}

/// A mutation in flight when the kernel is destroyed completes: its promise
/// resolves and the engine work it started runs to the end, while its state
/// write is dropped with an `instance-closed` report. A caller's cancel is a
/// different outcome: the same mutation rejects `operation-cancelled`.
#[test]
fn work_in_flight_at_teardown_completes_where_a_cancel_rejects() {
    let rig = Rig::with(library(Vec::new()), held_open(), true);
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let id = rig.stamp.list_assets(None)[0].id.clone();
    let removal = rig.stamp.delete_asset(&id, none());
    rt::run_until_stalled();
    success(&rig.kernel.destroy());
    assert!(rig.fake.settle("hold", Ok(json!({ "$document": "lib" }))));
    success(&removal);
    assert!(rig.calls_to("pages.delete") == 1);
    assert!(
        rig.reports.borrow().iter().any(|r| r.plugin_code() == Some(code::INSTANCE_CLOSED)),
        "the dropped state write is reported"
    );

    let rig = Rig::with(library(Vec::new()), held_open(), true);
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let id = rig.stamp.list_assets(None)[0].id.clone();
    let signal = AbortSignal::new();
    let removal = rig.stamp.delete_asset(&id, with_signal(&signal));
    rt::run_until_stalled();
    signal.abort(Error::new("caller gave up"));
    let refused = failure(&removal);
    assert_eq!(refused.plugin_code(), Some(code::OPERATION_CANCELLED));
    assert!(rig.stamp.get_asset(&id).is_some(), "a cancelled removal keeps the asset");
}

/// `services/ghosts.ts`: one render per width per asset, kept until the
/// asset goes; another width renders anew.
#[test]
fn ghost_renders_are_cached_per_width() {
    let rig = Rig::new();
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let id = rig.stamp.list_assets(None)[0].id.clone();
    let renders = rig.calls_to("page.render.image");
    let first = success(&rig.stamp.render_asset_preview(&id, 64.0, none())).unwrap();
    assert_eq!(rig.calls_to("page.render.image"), renders + 1);
    let again = success(&rig.stamp.render_asset_preview(&id, 64.0, none())).unwrap();
    assert_eq!(rig.calls_to("page.render.image"), renders + 1, "the same width is cached");
    assert_eq!(first, again);
    success(&rig.stamp.render_asset_preview(&id, 128.0, none()));
    assert_eq!(rig.calls_to("page.render.image"), renders + 2, "another width renders");
    assert_eq!(first.mime_type, Some(json!("image/png")));
}

/// A library owns its assets: deleting it drops their descriptors, bytes,
/// previews and renders, and its canonical PDF.
#[test]
fn a_deleted_library_takes_its_assets_with_it() {
    let rig = Rig::new();
    let library = success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    let ids: Vec<String> = rig.stamp.list_assets(None).iter().map(|a| a.id.clone()).collect();
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|id| rig.stamp.read_asset_bytes(id).is_some()));
    success(&rig.stamp.delete_library(&library.id, none()));
    for id in &ids {
        assert!(rig.stamp.get_asset(id).is_none());
        assert!(rig.stamp.read_asset_bytes(id).is_none());
        assert!(rig.stamp.get_asset_preview(id).is_none());
        let refused = failure(&rig.stamp.render_asset_preview(id, 64.0, none()));
        assert_eq!(refused.plugin_code(), Some(code::NOT_FOUND));
    }
    assert!(rig.stamp.list_libraries(None).is_empty());
    let refused = failure(&rig.stamp.export_library(&library.id, none()));
    assert_eq!(refused.plugin_code(), Some(code::NOT_FOUND));
}

/// Without a configured engine the kernel's is refused before any open,
/// and `canImport` turns false. Configuring a local engine re-resolves at
/// the next library operation: `canImport` is optimistic again and the
/// import goes through.
#[test]
fn a_setting_change_re_resolves_the_asset_engine() {
    let rig = Rig::with(library(Vec::new()), Vec::new(), false);
    assert!(rig.stamp.can_import());
    let refused = failure(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    assert_eq!(refused.plugin_code(), Some(code::UNSUPPORTED));
    assert_eq!(rig.calls_to("open"), 0, "refused before open");
    assert!(!rig.stamp.can_import());

    let engine: Rc<dyn epdf_kernel::engine::Engine> = Rc::new(AsEngine(rig.fake.clone()));
    let changed = Rc::new(RefCell::new(Vec::new()));
    let heard = changed.clone();
    let _off = rig.stamp.on_settings_changed(move |event| {
        heard.borrow_mut().extend(event.changed.clone());
        Ok(())
    });
    rig.stamp.update_settings(&StampConfig {
        asset_engine: Some(Some(StampAssetEngine::Engine(LocalEngine::new(engine)))),
        ..StampConfig::default()
    });
    assert_eq!(*changed.borrow(), ["assetEngine"]);
    assert!(rig.stamp.get_settings().asset_engine.is_some());
    assert!(rig.stamp.can_import(), "a changed setting makes imports possible again");
    success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    assert_eq!(rig.stamp.list_libraries(None).len(), 1);

    rig.stamp.reset_settings();
    assert!(rig.stamp.get_settings().asset_engine.is_none());
}

/// `services/asset-engine.ts:56`: with nothing configured, the kernel's
/// engine is used when it carries the local brand (`Engine::is_local`); an
/// unbranded one is refused before any open.
#[test]
fn an_unconfigured_plugin_imports_on_a_local_kernel_engine() {
    let rig = Rig::over(library(Vec::new()), Vec::new(), false, true);
    let imported = success(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    assert_eq!(rig.calls_to("open"), 1, "the import opened on the kernel's engine");
    assert_eq!(rig.stamp.list_libraries(None), vec![imported]);
    assert!(rig.stamp.can_import());

    let rig = Rig::over(library(Vec::new()), Vec::new(), false, false);
    let refused = failure(&rig.stamp.import_library(pdf(), ImportLibraryOptions::default()));
    assert_eq!(refused.plugin_code(), Some(code::UNSUPPORTED));
    assert_eq!(rig.calls_to("open"), 0, "refused before open");
    assert!(!rig.stamp.can_import());
}
