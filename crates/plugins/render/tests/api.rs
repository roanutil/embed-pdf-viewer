//! The typed surface downstream crates use and the vectors cannot see:
//! view handle identity, the store and manager accessors, handle costs,
//! and the release hook a live host hears when a handle's last clone drops.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_render::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 612, "top": 792 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "security": { "allows": ["doc.render"] },
            "pages": [{ "index": 0, "ref": { "kind": "objectNumber", "objectNumber": 11 },
                "label": null,
                "size": { "width": 612, "height": 792 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ } }],
            "responses": [{ "path": "page.render.image",
                "value": { "source": { "kind": "bytes", "bytes": { "$bytes": "AQID" } } } }]
        } }
    }));
    let host = RenderHost { warn: Rc::new(|_| {}), debug: Rc::new(|_| {}), ..Default::default() };
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![render_plugin_with_host(RenderConfig::default(), host)],
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
    kernel
}

#[test]
fn view_handles_are_stable_per_view() {
    let kernel = kernel();
    let render = kernel.capability(&RENDER, Some("a")).expect("render");
    let stage = render.create_view_demand("stage");
    assert!(stage.same(&render.create_view_demand("stage")));
    assert!(!stage.same(&render.create_view_demand("thumbs")));
    assert_eq!(stage.view_id(), "stage");
    assert_eq!(render.create_view_demand("thumbs").view_id(), "thumbs");
    // With every handle dropped, the next one is new but shares the view's
    // reference count and demands.
    drop(stage);
    let again = render.create_view_demand("stage");
    assert_eq!(again.view_id(), "stage");
}

#[test]
fn the_store_and_manager_are_the_capability_s() {
    let kernel = kernel();
    let render = kernel.capability(&RENDER, Some("a")).expect("render");
    assert_eq!(render.raster_store().size(), 0);
    let page = render
        .render_source(PageRef::new(11), RenderSourceOptions { scale: 0.5, ..Default::default() });
    assert_eq!(render.raster_store().size(), 1);
    rt::run_until_stalled();
    let handle = page.peek().expect("settled").expect("resolved");
    assert_eq!(handle.bytes().map(|b| b.0), Some(vec![1, 2, 3]));
    assert_eq!(handle.json()["source"]["kind"], "bytes");
    assert_eq!(render.raster_store().cost_used(), 3.0);

    let demand = PageViewDemand {
        desired_device_width: 4896.0,
        visible_rect: Some(epdf_geometry::Rect { x: 0.0, y: 0.0, width: 128.0, height: 128.0 }),
        velocity: None,
    };
    let stage = render.create_view_demand("stage");
    stage.set_demand(PageRef::new(11), demand, PageLayerOptions::default());
    let stats = render.tile_manager().stats("stage", 11);
    assert_eq!(stats, TileStats { entries: 4, in_flight: 4 });
    rt::run_until_stalled();
    assert_eq!(render.tile_manager().stats("stage", 11).in_flight, 0);
    assert_eq!(render.tile_manager().stats("thumbs", 11).entries, 0);
}

#[test]
fn handle_costs() {
    let cost = |v: serde_json::Value| PageImageHandle::new(v).cost();
    assert_eq!(
        cost(json!({ "source": { "kind": "bytes", "bytes": { "$bytes": "AQIDBA==" } } })),
        4.0
    );
    assert_eq!(
        cost(json!({ "source": { "kind": "bytes", "bytes": { "$byteLength": 4096 } },
            "width": 10, "height": 10 })),
        4096.0
    );
    assert_eq!(
        cost(json!({ "source": { "kind": "url", "url": "x" }, "width": 3, "height": 5 })),
        15.0
    );
    // A native host's raw pixels, held host side: the byte count, which is
    // four times the pixel count (and more, with row padding).
    assert_eq!(
        cost(json!({ "source": { "kind": "pixels", "$byteLength": 64 },
            "width": 3, "height": 5, "stride": 16 })),
        64.0
    );
    // Bytes with neither an inline payload nor a byte count: the pixel
    // estimate, never NaN (which would stop the LRU evicting for good).
    assert_eq!(
        cost(json!({ "source": { "kind": "bytes", "bytes": {} }, "width": 3, "height": 5 })),
        15.0
    );
    assert_eq!(cost(json!({ "source": { "kind": "bytes", "bytes": {} } })), 1.0);
    assert_eq!(cost(json!({ "width": null, "height": 5 })), 0.0);
    assert_eq!(cost(json!({ "width": true, "height": 5 })), 5.0);
    assert_eq!(cost(json!({ "width": 7 })), 1.0);
    assert!(cost(json!({ "width": "7", "height": 1 })).is_nan());
    let url = PageImageHandle::new(json!({ "source": { "kind": "url", "url": "x" } }));
    assert_eq!(url.bytes(), None);
}

#[test]
fn a_lengthless_bytes_handle_keeps_the_lru_working() {
    rt::reset();
    // A budget of one ten-byte image.
    let store = RasterStore::new(10.0);
    let fetch = |id: &str, value: Value| {
        let image =
            store.acquire(id, move |_| Ok(Promise::resolved(value)), None).expect("acquire");
        rt::run_until_stalled();
        assert!(image.peek().expect("settled").is_ok());
    };
    fetch("a", json!({ "source": { "kind": "bytes", "bytes": {} }, "width": 2, "height": 4 }));
    assert_eq!(store.cost_used(), 8.0, "the pixel estimate, not NaN");
    fetch("b", json!({ "source": { "kind": "bytes", "bytes": { "$byteLength": 10 } } }));
    assert_eq!(store.size(), 1, "over budget, a was evicted");
    assert!(store.peek("a").is_none());
    assert_eq!(store.cost_used(), 10.0);
}

#[test]
fn quantize_names_only_exact() {
    assert_eq!(serde_json::from_value::<Quantize>(json!("exact")).unwrap(), Quantize::Exact);
    assert!(serde_json::from_value::<Quantize>(json!("fast")).is_err());
    assert_eq!(
        serde_json::from_value::<Quantize>(json!([1, 2])).unwrap(),
        Quantize::Ladder(vec![1.0, 2.0])
    );
}

#[test]
fn a_store_releases_a_handle_once_its_last_clone_drops() {
    rt::reset();
    let seen: Rc<RefCell<Vec<Value>>> = Rc::default();
    let record = seen.clone();
    // A budget of one ten-byte image.
    let store = RasterStore::with_release(
        10.0,
        Rc::new(move |v: &Value| record.borrow_mut().push(v.clone())),
    );
    let image = |id: &str| json!({ "id": id, "source": { "kind": "bytes", "bytes": { "$byteLength": 10 } } });
    let fetch = |id: &str| {
        let value = image(id);
        store.acquire(id, move |_| Ok(Promise::resolved(value)), None).expect("acquire")
    };

    // Each consumer promise holds a clone of its handle; drop it once read.
    let a = fetch("a");
    rt::run_until_stalled();
    assert!(a.peek().expect("settled").is_ok());
    drop(a);
    assert!(seen.borrow().is_empty(), "a is still in the store");

    let b = fetch("b");
    rt::run_until_stalled();
    assert!(b.peek().expect("settled").is_ok());
    drop(b);
    assert_eq!(*seen.borrow(), vec![image("a")], "evicting a released it, and only it");
    assert_eq!(store.size(), 1);

    drop(store);
    assert_eq!(*seen.borrow(), vec![image("a"), image("b")], "dropping the store released b");
}

#[test]
fn a_clone_held_outside_the_store_defers_release_until_it_drops() {
    rt::reset();
    let seen: Rc<RefCell<Vec<Value>>> = Rc::default();
    let record = seen.clone();
    // A budget of one ten-byte image.
    let store = RasterStore::with_release(
        10.0,
        Rc::new(move |v: &Value| record.borrow_mut().push(v.clone())),
    );
    let image = |id: &str| json!({ "id": id, "source": { "kind": "bytes", "bytes": { "$byteLength": 10 } } });
    let fetch = |id: &str| {
        let value = image(id);
        store.acquire(id, move |_| Ok(Promise::resolved(value)), None).expect("acquire")
    };

    let a = fetch("a");
    rt::run_until_stalled();
    // A tile plan or paint promise holding its own clone, outside the store.
    let held = store.peek("a").expect("resolved");
    drop(a);

    // Fill the one-image budget with "b": the store evicts "a" from its own
    // map, but `held` still keeps its last clone alive.
    let b = fetch("b");
    rt::run_until_stalled();
    drop(b);
    assert_eq!(store.size(), 1, "a was evicted from the store");
    assert!(seen.borrow().is_empty(), "a's clone outside the store is still live");

    drop(held);
    assert_eq!(*seen.borrow(), vec![image("a")], "the last clone dropping released a");
}

#[test]
fn a_settled_fetch_lets_go_of_its_abort_listeners() {
    // The controller's fetch hangs the engine task's abort on the store's
    // signal, and that listener retains the task and its answer. Once the
    // fetch settles the store must drop it, or every cached raster keeps a
    // second, uncosted copy of its image alive.
    rt::reset();
    let store = RasterStore::default();
    let task_answer = Rc::new(());
    let held = task_answer.clone();
    let (task, resolver) = Promise::pending();
    let image = store
        .acquire(
            "a",
            move |signal| {
                signal.on_abort(Rc::new(move |_| {
                    let _ = &held;
                }));
                Ok(task)
            },
            None,
        )
        .expect("acquire");
    assert_eq!(Rc::strong_count(&task_answer), 2, "in flight, the listener is live");
    resolver.resolve(json!({ "width": 1, "height": 1 }));
    rt::run_until_stalled();
    assert!(image.peek().expect("settled").is_ok());
    assert!(store.peek("a").is_some(), "the raster is still cached");
    assert_eq!(Rc::strong_count(&task_answer), 1, "the listener went with the fetch");
}

#[test]
fn a_handle_without_a_hook_releases_nothing() {
    // `PageImageHandle::new` keeps today's shape: no hook, nothing to call.
    let handle = PageImageHandle::new(json!({ "width": 1 }));
    let clone = handle.clone();
    assert_eq!(handle, clone);
    drop(handle);
    assert_eq!(clone.json()["width"], 1);
}

/// A manager over one 612x792 page whose `fetch_tile` refuses synchronously
/// while `refuse` is set, and otherwise answers a promise that never settles.
fn refusing_manager(
    refuse: Rc<std::cell::Cell<bool>>,
    calls: Rc<std::cell::Cell<u32>>,
    failures: Rc<std::cell::Cell<u32>>,
) -> TileManager {
    let options = Rc::new(resolve_render_options(&RenderConfig {
        tiles: Some(TilesSetting::On(TilesOptions { settle_ms: Some(0.0), ..Default::default() })),
        ..Default::default()
    }));
    TileManager::new(TileManagerDeps {
        store: RasterStore::default(),
        get_options: Rc::new(move || options.clone()),
        get_policy: Rc::new(|| CONTINUOUS_RENDER_POLICY),
        get_page_size: Rc::new(|_| Some(PageSizePt { width: 612.0, height: 792.0 })),
        get_epoch: Rc::new(|_, _| 0.0),
        after: Rc::new(|_, _| epdf_kernel::Unsubscribe::noop()),
        fetch_tile: Rc::new(move |_, _, _, _, _, _| {
            calls.set(calls.get() + 1);
            if refuse.get() {
                Err(epdf_kernel::Error::new("page is not in this document"))
            } else {
                Ok(Promise::pending().0)
            }
        }),
        on_advance: Rc::new(move |_| failures.set(failures.get() + 1)),
        debug: None,
        warn: Rc::new(|_| {}),
    })
}

/// M25: a fetch refused synchronously frees its transit slot and counts as a
/// failure at its level, so refusals past `MAX_IN_FLIGHT` (8) never stop
/// tile fetching. The TypeScript's `fetchTile` is `async` and cannot refuse
/// this way; a synchronous throw there would strand the entry and the slot.
#[test]
fn a_synchronous_tile_refusal_frees_its_slot() {
    rt::reset();
    let refuse = Rc::new(std::cell::Cell::new(true));
    let calls = Rc::new(std::cell::Cell::new(0));
    let failures = Rc::new(std::cell::Cell::new(0));
    let tiles = refusing_manager(refuse.clone(), calls.clone(), failures.clone());
    let demand = |width: f64| PageViewDemand {
        desired_device_width: width,
        visible_rect: Some(epdf_geometry::Rect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 }),
        velocity: None,
    };
    let layers = PageRenderLayers::default();
    // 8x over a 612pt page in 512px tiles is 10x13 visible tiles, more than
    // the transit bound: every one is tried in this plan, none strands.
    let plan = tiles.plan("v", 11, &demand(4896.0), layers);
    assert_eq!(calls.get(), 130, "each visible tile tried once");
    assert_eq!(failures.get(), 130, "each refusal woke the layers");
    assert_eq!(tiles.stats("v", 11), TileStats { entries: 0, in_flight: 0 }, "nothing stranded");
    assert!(plan.engaged && plan.fetching.is_empty() && plan.paint.is_empty());
    // At the same level a failed key is not retried.
    tiles.plan("v", 11, &demand(4896.0), layers);
    assert_eq!(calls.get(), 130);
    // The engine accepts again: a new level clears the failures and fetching
    // resumes up to the transit bound.
    refuse.set(false);
    tiles.plan("v", 11, &demand(5200.0), layers);
    assert_eq!(tiles.stats("v", 11).in_flight, 8);
    assert_eq!(calls.get(), 138);
}

#[test]
fn a_zero_tile_span_yields_no_tiles_instead_of_looping() {
    use epdf_geometry::Rect;
    use epdf_plugin_render::tiles::{region_covered, tile_grid, tiles_in_rect};
    let page = PageSizePt { width: 600.0, height: 800.0 };
    let rect = |x, y| Rect { x, y, width: 100.0, height: 100.0 };
    // A tile size of 0 and an infinite scale both make the span 0. The
    // TypeScript loops here until it runs out of memory.
    for grid in [tile_grid(page, 1.0, 0.0), tile_grid(page, f64::INFINITY, 256.0)] {
        for at in [rect(0.0, 0.0), rect(10.0, 0.0), rect(0.0, 10.0), rect(10.0, 10.0)] {
            assert!(tiles_in_rect(&grid, page, at).is_empty());
            assert!(region_covered(&grid, page, at, |_| false));
        }
    }
    // A normal grid is untouched: 256 px tiles over 600x800 points at 1x.
    let grid = tile_grid(page, 1.0, 256.0);
    let tiles = tiles_in_rect(&grid, page, Rect { x: 0.0, y: 0.0, width: 600.0, height: 800.0 });
    assert_eq!(tiles.len(), 3 * 4);
    assert_eq!(tiles[0], TileCoord { ix: 0.0, iy: 0.0 });
    assert_eq!(tiles[11], TileCoord { ix: 2.0, iy: 3.0 });
}
