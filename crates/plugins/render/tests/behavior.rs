//! Behaviour the vectors reach only in part, driven through the kernel and
//! the fake engine: what each invalidation scope repaints and what changes
//! nothing, a tile answer that lands after its tile left the want set, tile
//! work and render requests at close, the settings, the page-layer rights
//! and the appearance pictures.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_kernel::engine::{
    CallFacts, CallPriority, DocumentHandle, DocumentSecurity, Engine, EngineCall,
};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Error, Kernel, KernelOptions, rt};
use epdf_plugin_render::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

const PAGE: PageRef = PageRef { page_object_number: 11 };
const OTHER: PageRef = PageRef { page_object_number: 22 };

fn layout(pon: u32, index: u32) -> Value {
    let box_ = json!({ "left": 0, "bottom": 0, "right": 612, "top": 792 });
    json!({ "index": index, "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null, "size": { "width": 612, "height": 792 }, "rotation": 0, "userUnit": 1,
        "boxes": { "media": box_, "crop": box_ } })
}

/// Each `call_with`: the path, the page and the facts it carried.
type Facts = Rc<RefCell<Vec<(String, Option<u32>, CallFacts)>>>;

/// The fake engine, whose handles record the facts of every `call_with`
/// before answering it as the fake does.
struct Recording {
    fake: Rc<FakeEngine>,
    facts: Facts,
}

impl Engine for Recording {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let opened = AsEngine(self.fake.clone()).open(input, options);
        let facts = self.facts.clone();
        opened.convert(move |inner| {
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
        self.facts.borrow_mut().push((path.to_string(), pon, facts.clone()));
        self.inner.call_with(facts, path, pon, args)
    }
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.inner.page_index(pon)
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.inner.subscribe(listener)
    }
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    render: RenderCapability,
    warnings: Rc<RefCell<Vec<String>>>,
    facts: Facts,
    /// Handles the raster store let go of (`RenderHost::release_image`).
    released: Rc<Cell<usize>>,
}

impl Rig {
    /// One document, two pages, every engine call deferred under `r0`, `r1`,
    /// ... in call order, plus `extra` responses ahead of them.
    fn new(allows: Value, config: RenderConfig, extra: Vec<Value>) -> Rig {
        rt::reset();
        let mut responses = extra;
        responses.extend(
            (0..64).map(
                |i| json!({ "path": "page.render.image", "times": 1, "defer": format!("r{i}") }),
            ),
        );
        let fake = FakeEngine::new(json!({
            "documents": { "a": {
                "security": { "allows": allows },
                "pages": [layout(11, 0), layout(22, 1)],
                "responses": responses,
            } }
        }));
        let warnings: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = warnings.clone();
        let released = Rc::new(Cell::new(0));
        let counter = released.clone();
        let host = RenderHost {
            warn: Rc::new(move |m| sink.borrow_mut().push(m.to_string())),
            debug: Rc::new(|_| {}),
            release_image: Some(Rc::new(move |_| counter.set(counter.get() + 1))),
        };
        let facts: Facts = Rc::default();
        let kernel = Kernel::new(
            Rc::new(Recording { fake: fake.clone(), facts: facts.clone() }),
            vec![render_plugin_with_host(config, host)],
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
        let render = kernel.capability(&RENDER, Some("a")).expect("render");
        Rig { kernel, fake, render, warnings, facts, released }
    }

    fn everything() -> Rig {
        Rig::new(
            json!(["doc.render", "doc.annotate.read", "doc.forms.read"]),
            RenderConfig::default(),
            vec![],
        )
    }

    fn settle(&self, label: &str) {
        assert!(self.fake.settle(label, Ok(json!({ "width": 512, "height": 512 }))), "{label}");
        rt::run_until_stalled();
    }

    fn image_calls(&self) -> usize {
        self.fake
            .log_since(0)
            .iter()
            .filter(|entry| entry.get("path").and_then(Value::as_str) == Some("page.render.image"))
            .count()
    }

    fn emit(&self, event: Value) {
        self.fake.emit("a", &event).expect("emit");
        rt::run_until_stalled();
    }

    fn epoch(&self, page: PageRef, annotations: bool, fields: bool) -> f64 {
        self.render.get_render_epoch(
            page,
            PageLayerOptions {
                include_annotations: Some(annotations),
                include_form_fields: Some(fields),
            },
        )
    }

    fn close(&self) {
        self.kernel.documents().close("a");
        rt::run_until_stalled();
    }
}

/// Each `onInvalidated` event: its page object numbers and scope.
type Seen = Rc<RefCell<Vec<(Vec<u32>, InvalidateScope)>>>;

fn record_invalidations(render: &RenderCapability) -> Seen {
    let seen: Seen = Rc::default();
    let sink = seen.clone();
    let _ = render.on_invalidated(move |event| {
        let pages = event.pages.iter().map(|p| p.page_object_number).collect();
        sink.borrow_mut().push((pages, event.scope));
        Ok(())
    });
    seen
}

fn deep(rect: Rect) -> PageViewDemand {
    PageViewDemand { desired_device_width: 4896.0, visible_rect: Some(rect), velocity: None }
}

fn square(x: f64, y: f64, side: f64) -> Rect {
    Rect { x, y, width: side, height: side }
}

#[test]
fn each_scope_repaints_the_pictures_that_draw_it_and_nothing_else() {
    let rig = Rig::everything();
    let seen = record_invalidations(&rig.render);
    rig.render.invalidate(InvalidateOptions {
        pages: Some(vec![PAGE.into()]),
        scope: Some(InvalidateScope::Annotations),
    });
    assert_eq!((rig.epoch(PAGE, false, false), rig.epoch(PAGE, true, false)), (0.0, 1.0));
    assert_eq!(rig.epoch(PAGE, false, true), 0.0, "a fields-only picture keeps its pixels");
    rig.render.invalidate(InvalidateOptions {
        pages: Some(vec![PAGE.into()]),
        scope: Some(InvalidateScope::Fields),
    });
    assert_eq!((rig.epoch(PAGE, false, true), rig.epoch(PAGE, true, false)), (1.0, 1.0));
    assert_eq!(rig.epoch(PAGE, true, true), 2.0);
    // Content reaches every picture; `None` is content.
    rig.render.invalidate(InvalidateOptions { pages: Some(vec![PAGE.into()]), scope: None });
    assert_eq!(rig.epoch(PAGE, false, false), 1.0);
    assert_eq!(rig.epoch(PAGE, true, true), 3.0);
    // No pages, or only pages the document lacks: no bump and no event.
    let revision = rig.kernel.state_revision();
    rig.render.invalidate(InvalidateOptions { pages: Some(vec![]), scope: None });
    rig.render.invalidate(InvalidateOptions {
        pages: Some(vec![PageRef::new(99).into(), PageArg::Index(7.0)]),
        scope: Some(InvalidateScope::Fields),
    });
    assert_eq!(rig.kernel.state_revision(), revision, "the store is untouched");
    // An index names a page as a ref does; every page when none are named.
    rig.render.invalidate(InvalidateOptions {
        pages: Some(vec![PageArg::Index(1.0)]),
        scope: Some(InvalidateScope::Annotations),
    });
    rig.render.invalidate(InvalidateOptions::default());
    assert_eq!(
        *seen.borrow(),
        vec![
            (vec![11], InvalidateScope::Annotations),
            (vec![11], InvalidateScope::Fields),
            (vec![11], InvalidateScope::Content),
            (vec![22], InvalidateScope::Annotations),
            (vec![11, 22], InvalidateScope::Content),
        ]
    );
    assert_eq!(rig.render.get_render_epoch(PageRef::new(99), PageLayerOptions::default()), 0.0);
}

#[test]
fn confirmed_facts_bump_their_ledger_and_the_rest_change_nothing() {
    let rig = Rig::everything();
    let seen = record_invalidations(&rig.render);
    let page = |pon: u32| json!({ "kind": "objectNumber", "objectNumber": pon });
    rig.emit(json!({ "type": "annotations.created", "page": page(11) }));
    rig.emit(json!({ "type": "forms.valueSet",
        "meta": { "changedWidgets": [{ "page": page(22) }, { "page": null }] } }));
    rig.emit(json!({ "type": "pages.flattened",
        "results": [{ "page": page(22), "status": "applied" }, { "page": page(11), "status": "failed" }] }));
    let revision = rig.kernel.state_revision();
    for event in [
        json!({ "type": "pages.rotated", "pages": [page(11)] }),
        json!({ "type": "metadata.updated", "metadata": {} }),
        json!({ "type": "forms.valueSet", "meta": { "changedWidgets": [{ "page": null }] } }),
        json!({ "type": "redaction.applied", "results": [{ "page": page(11), "status": "skipped" }] }),
    ] {
        rig.emit(event);
    }
    assert_eq!(rig.kernel.state_revision(), revision, "no ledger moved");
    assert_eq!(
        *seen.borrow(),
        vec![
            (vec![11], InvalidateScope::Annotations),
            (vec![22], InvalidateScope::Fields),
            (vec![22], InvalidateScope::Content),
        ]
    );
    assert_eq!((rig.epoch(PAGE, true, false), rig.epoch(PAGE, false, true)), (1.0, 0.0));
    assert_eq!((rig.epoch(OTHER, false, false), rig.epoch(OTHER, false, true)), (1.0, 2.0));
    // A stream that lost events repaints every page's content.
    rig.emit(json!({ "type": "stream.desynced" }));
    assert_eq!((rig.epoch(PAGE, false, false), rig.epoch(OTHER, false, false)), (1.0, 2.0));
}

#[test]
fn a_tile_answer_that_lands_after_its_tile_left_the_want_set_changes_nothing() {
    let rig = Rig::new(
        json!("*"),
        RenderConfig {
            tiles: Some(TilesSetting::On(TilesOptions {
                prefetch: Some(PrefetchOptions { margin: Some(0.0), velocity_bias: None }),
                ..Default::default()
            })),
            ..Default::default()
        },
        vec![],
    );
    let view = rig.render.create_view_demand("stage");
    let layers = PageLayerOptions::default();
    view.set_demand(PAGE, deep(square(0.0, 0.0, 128.0)), layers);
    assert_eq!(rig.image_calls(), 4);
    assert_eq!(rig.render.tile_manager().stats("stage", 11).in_flight, 4);
    // Panning away aborts the four in flight and starts the four now in view.
    view.set_demand(PAGE, deep(square(448.0, 448.0, 128.0)), layers);
    assert_eq!(rig.image_calls(), 8);
    assert_eq!(rig.render.tile_manager().stats("stage", 11).in_flight, 4);
    let before = view.get_plan(PAGE);
    assert_eq!(before.fetching.len(), 4);
    // The old tiles' answers arrive late: the aborted store entries are gone,
    // so nothing is cached, no slot is freed twice and the plan is the same.
    for label in ["r0", "r1", "r2", "r3"] {
        rig.settle(label);
    }
    assert_eq!(rig.render.tile_manager().stats("stage", 11).in_flight, 4);
    assert_eq!(rig.render.raster_store().size(), 4, "only the wanted tiles are held");
    // A late answer to an aborted fetch never becomes a handle: the abort
    // rejected the call first, so the host has nothing of it to release.
    assert_eq!(rig.released.get(), 0);
    view.set_demand(PAGE, deep(square(448.0, 448.0, 128.0)), layers);
    assert!(Rc::ptr_eq(&before, &view.get_plan(PAGE)), "the plan did not move");
    // The wanted tiles' own answers do paint.
    for label in ["r4", "r5", "r6", "r7"] {
        rig.settle(label);
    }
    view.set_demand(PAGE, deep(square(448.0, 448.0, 128.0)), layers);
    let plan = view.get_plan(PAGE);
    assert_eq!((plan.paint.len(), plan.fetching.len()), (4, 0));
    assert!(plan.paint.iter().all(|source| source.key.contains("|w4896|")));
    // The four handles that did land are the host's to free once the store
    // and the plans let go of them: with the document closed and every
    // capability dropped, each is released once.
    assert_eq!(rig.released.get(), 0);
    let Rig { kernel, render, released, .. } = rig;
    drop((plan, before, view, render));
    kernel.documents().close("a");
    rt::run_until_stalled();
    drop(kernel);
    assert_eq!(released.get(), 4);
}

#[test]
fn closing_cancels_tile_work_the_settle_timer_and_renders_in_flight() {
    let rig = Rig::new(
        json!("*"),
        RenderConfig {
            tiles: Some(TilesSetting::On(TilesOptions {
                prefetch: Some(PrefetchOptions { margin: Some(0.0), velocity_bias: None }),
                ..Default::default()
            })),
            ..Default::default()
        },
        vec![],
    );
    let view = rig.render.create_view_demand("stage");
    let layers = PageLayerOptions::default();
    // Four tiles of one page in flight, which the close must free.
    view.set_demand(PAGE, deep(square(0.0, 0.0, 128.0)), layers);
    // Another page engages, then changes level: its four abort and the
    // 150 ms settle timer arms.
    view.set_demand(OTHER, deep(square(0.0, 0.0, 128.0)), layers);
    assert_eq!(rig.render.tile_manager().stats("stage", 22).in_flight, 8);
    view.set_demand(
        OTHER,
        PageViewDemand {
            desired_device_width: 5200.0,
            visible_rect: Some(square(0.0, 0.0, 128.0)),
            velocity: None,
        },
        layers,
    );
    assert_eq!(rig.render.tile_manager().stats("stage", 11).in_flight, 4, "the page's tiles");
    let page = rig.render.render_page(PAGE, RenderPageOptions::default());
    let source =
        rig.render.render_source(OTHER, RenderSourceOptions { scale: 0.5, ..Default::default() });
    rt::run_until_stalled();
    let calls = rig.image_calls();
    rig.close();
    rt::advance(500.0);
    rt::run_until_stalled();
    assert_eq!(rig.image_calls(), calls, "no settle fired and no tile started after close");
    assert_eq!(rig.render.tile_manager().stats("stage", 11).in_flight, 0, "every slot freed");
    for promise in [page, source] {
        let error = promise.peek().expect("settled at close").unwrap_err();
        assert_eq!(error.plugin_code(), Some("instance-closed"), "{error:?}");
    }
    // A view handle kept past the close plans from what is cached and asks
    // the engine for nothing.
    view.set_demand(PAGE, deep(square(0.0, 0.0, 128.0)), layers);
    rt::advance(500.0);
    rt::run_until_stalled();
    assert_eq!(rig.image_calls(), calls);
}

#[test]
fn settings_apply_at_once_and_reset_goes_back() {
    let rig = Rig::everything();
    let changes: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let sink = changes.clone();
    let _ = rig.render.on_settings_changed(move |event| {
        sink.borrow_mut().push(event.changed.clone());
        Ok(())
    });
    let width = |rig: &Rig| match rig.render.conform_viewport(PAGE, 8.0).unwrap() {
        PageRenderViewport::Width { width } => width,
        other => panic!("{other:?}"),
    };
    assert_eq!(width(&rig), 640.0);
    let paint = rig.render.get_paint_settings();
    rig.render.update_settings(&RenderConfig {
        full_page: Some(FullPageOptions { max_width: Some(300.0), quantize: None }),
        ..Default::default()
    });
    assert_eq!(width(&rig), 300.0, "a new budget applies at once");
    assert!(Rc::ptr_eq(&paint, &rig.render.get_paint_settings()), "the tile switch did not move");
    assert_eq!(rig.render.get_settings().full_page.and_then(|f| f.quantize), Some(Quantize::Exact));
    rig.render
        .update_settings(&RenderConfig { tiles: Some(TilesSetting::Off), ..Default::default() });
    assert!(!rig.render.get_paint_settings().tiles);
    rig.render.reset_settings();
    assert_eq!(width(&rig), 640.0);
    assert!(rig.render.get_paint_settings().tiles);
    // A change that changes nothing fires nothing.
    rig.render.update_settings(&RenderConfig { debug: Some(false), ..Default::default() });
    assert_eq!(
        *changes.borrow(),
        vec![
            vec!["fullPage".to_string()],
            vec!["tiles".to_string()],
            vec!["fullPage".to_string(), "tiles".to_string()]
        ]
    );
}

#[test]
fn a_part_the_session_may_not_read_is_left_out_or_refused() {
    let rig = Rig::new(json!(["doc.render", "doc.forms.read"]), RenderConfig::default(), vec![]);
    assert_eq!(
        rig.render.get_layer_rights(),
        PageLayerRights { annotations: false, form_fields: true }
    );
    let key = |layers| rig.render.get_source_key(PAGE, 0.5, layers).unwrap();
    assert_eq!(key(PageLayerOptions::default()), "11|w306|a0f1|e0");
    // A read counts an option the session may not read as left out.
    let annotations =
        PageLayerOptions { include_annotations: Some(true), include_form_fields: None };
    assert_eq!(key(annotations), "11|w306|a0f1|e0");
    // A door refuses it, naming the right and its own operation.
    let refused = rig
        .render
        .render_page(PAGE, RenderPageOptions { layers: annotations, ..Default::default() });
    rt::run_until_stalled();
    let error = refused.peek().expect("settled").unwrap_err();
    assert_eq!(error.plugin_code(), Some("permission-denied"));
    assert_eq!(error.message, "render.renderPage requires doc.annotate.read");
    assert_eq!(rig.image_calls(), 0);
    // Turning annotations off leaves the form fields out unless asked for.
    let off = PageLayerOptions { include_annotations: Some(false), include_form_fields: None };
    assert_eq!(key(off), "11|w306|a0f0|e0");
}

#[test]
fn appearance_asks_share_a_fetch_and_keep_their_sets() {
    let item = |n: u32| json!({ "kind": "objectNumber", "objectNumber": n });
    let picture = |n: u32, state: &str| json!({ "ref": item(n), "state": state, "width": 4 });
    let rig = Rig::new(
        json!(["doc.render", "doc.annotate.read"]),
        RenderConfig::default(),
        vec![json!({ "path": "page.annotations.renderAppearances", "times": 1, "defer": "a0" })],
    );
    let ask = |version: f64| AppearancesOptions {
        family: AppearanceFamily::Annotations,
        scale: 2.0,
        items: vec![AppearanceItem { reference: item(5), version }],
        signal: None,
    };
    let first = rig.render.render_appearances(PAGE, ask(1.0));
    let second = rig.render.render_appearances(PageArg::Index(0.0), ask(1.0));
    rt::run_until_stalled();
    let calls = |rig: &Rig| {
        rig.fake
            .log_since(0)
            .iter()
            .filter(|e| {
                e.get("path").and_then(Value::as_str) == Some("page.annotations.renderAppearances")
            })
            .count()
    };
    assert_eq!(calls(&rig), 1, "one fetch for the same page, family, scale and items");
    let answer = json!({ "appearances": [picture(5, "on"), picture(5, "off"), picture(6, "on")] });
    assert!(rig.fake.settle("a0", Ok(answer)));
    rt::run_until_stalled();
    for promise in [first, second] {
        let pictures = promise.peek().expect("settled").expect("pictures");
        assert_eq!(pictures.len(), 2, "only the asked item's pictures");
        assert!(pictures.iter().all(|p| p.version == 1.0));
        assert_eq!(serde_json::to_value(&pictures[0]).unwrap()["version"], 1.0);
    }
    // The resolved set is kept: asking again asks the engine for nothing.
    let again = rig.render.render_appearances(PAGE, ask(1.0));
    rt::run_until_stalled();
    assert_eq!(again.peek().expect("settled").expect("pictures").len(), 2);
    assert_eq!(calls(&rig), 1);
    // The item's last pictures are kept, by its ref.
    assert_eq!(rig.render.kept_appearances(AppearanceFamily::Annotations, &item(5)).len(), 2);
    assert!(rig.render.kept_appearances(AppearanceFamily::Widgets, &item(5)).is_empty());
    // Widgets need doc.forms.read.
    let widgets = rig.render.render_appearances(
        PAGE,
        AppearancesOptions { family: AppearanceFamily::Widgets, ..ask(1.0) },
    );
    rt::run_until_stalled();
    let error = widgets.peek().expect("settled").unwrap_err();
    assert_eq!(error.message, "render.renderAppearances requires doc.forms.read");
    assert_eq!(rig.render.get_appearance_scale(1.3).unwrap(), 1.3, "continuous keeps the scale");
    assert!(rig.warnings.borrow().is_empty());
}

/// Ruling13.16: every render asks the engine with `priority: 'high'`, and
/// with the view it is for when it knows one, as `controller.ts` does
/// (`ctx.doc.with({priority: 'high', view})`).
#[test]
fn every_render_carries_high_priority_and_its_view() {
    let rig = Rig::everything();
    let source =
        RenderSourceOptions { scale: 0.5, view: Some("rail".into()), ..Default::default() };
    let _ = rig.render.render_source(PAGE, source);
    let _ = rig.render.render_page(OTHER, RenderPageOptions::default());
    let view = rig.render.create_view_demand("main");
    view.set_demand(PAGE, deep(square(0.0, 0.0, 64.0)), PageLayerOptions::default());
    rt::run_until_stalled();
    let facts = rig.facts.borrow();
    let high = |view: Option<&str>| CallFacts {
        priority: Some(CallPriority::High),
        view: view.map(str::to_string),
    };
    assert_eq!(facts[0], ("page.render.image".to_string(), Some(11), high(Some("rail"))));
    assert_eq!(facts[1], ("page.render.image".to_string(), Some(22), high(None)));
    let tiles = &facts[2..];
    assert!(!tiles.is_empty());
    assert!(
        tiles.iter().all(|f| *f == ("page.render.image".to_string(), Some(11), high(Some("main"))))
    );
    // The fake records the call itself, never the facts: its `with` returns
    // the handle unchanged.
    assert_eq!(rig.image_calls(), facts.len());
}

/// A settings value this port cannot decode reads as the defaults and warns
/// once, wherever it is read: the resolved options, `get_settings`, and the
/// change event.
#[test]
fn a_malformed_setting_reads_as_the_defaults_and_warns_once() {
    let rig = Rig::everything();
    let seen: Rc<RefCell<Vec<RenderSettings>>> = Rc::default();
    let sink = seen.clone();
    let _ = rig.render.on_settings_changed(move |event| {
        sink.borrow_mut().push(event.settings.clone());
        Ok(())
    });
    let store = rig.kernel.settings_of(RENDER.any()).expect("settings");
    store.update_settings(&json!({ "fullPage": { "maxWidth": "wide" } }));
    assert_eq!(*seen.borrow(), vec![render_defaults()]);
    assert_eq!(rig.render.get_settings(), render_defaults());
    let viewport = rig.render.conform_viewport(PAGE, 8.0).unwrap();
    assert_eq!(viewport, PageRenderViewport::Width { width: 640.0 });
    assert_eq!(rig.warnings.borrow().len(), 1, "{:?}", rig.warnings.borrow());
    assert!(rig.warnings.borrow()[0].starts_with("[render] malformed settings ("));
}
