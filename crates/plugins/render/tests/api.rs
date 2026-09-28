//! The typed surface downstream crates use and the vectors cannot see:
//! view handle identity, the store and manager accessors, handle costs.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_render::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

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
