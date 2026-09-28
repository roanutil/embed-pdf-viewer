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
            "pages": [{ "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 11 },
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
    stage.set_demand(PageRef::new(11), demand, None).expect("plan");
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
    assert_eq!(cost(json!({ "width": null, "height": 5 })), 0.0);
    assert_eq!(cost(json!({ "width": true, "height": 5 })), 5.0);
    assert_eq!(cost(json!({ "width": 7 })), 1.0);
    assert!(cost(json!({ "width": "7", "height": 1 })).is_nan());
    let url = PageImageHandle::new(json!({ "source": { "kind": "url", "url": "x" } }));
    assert_eq!(url.bytes(), None);
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
fn a_handle_without_a_hook_releases_nothing() {
    // `PageImageHandle::new` keeps today's shape: no hook, nothing to call.
    let handle = PageImageHandle::new(json!({ "width": 1 }));
    let clone = handle.clone();
    assert_eq!(handle, clone);
    drop(handle);
    assert_eq!(clone.json()["width"], 1);
}
