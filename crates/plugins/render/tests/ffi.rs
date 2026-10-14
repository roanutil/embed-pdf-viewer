#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::PageRef;
use epdf_plugin_render::ffi::{
    RenderEvent, RenderEventListener, RenderInvalidateScope, RenderPageOptions, RenderPixelFormat,
};
use futures::executor::block_on;
use serde_json::json;

mod support;

use support::{BLUE, Engine, RED, Rig, code, token};

const SMALL: PageRef = PageRef { page_object_number: 3 };
const LARGE: PageRef = PageRef { page_object_number: 4 };

fn scale(scale: f64) -> RenderPageOptions {
    RenderPageOptions { scale: Some(scale), ..Default::default() }
}

#[derive(Default)]
struct Events(Mutex<Vec<RenderEvent>>, AtomicUsize);

impl RenderEventListener for Events {
    fn on_event(&self, event: RenderEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

impl Events {
    fn taken(&self) -> Vec<RenderEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
    fn ends(&self) -> usize {
        self.1.load(Ordering::SeqCst)
    }
}

#[test]
fn a_page_renders_to_owned_pixels() {
    let rig = Rig::pixels();
    let image = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap();
    assert_eq!((image.width, image.height, image.stride), (2, 1, 8));
    assert_eq!(image.scale, 1.0);
    assert_eq!(image.format, RenderPixelFormat::Rgba8);
    assert_eq!(image.pixels, [RED, BLUE].concat());
}

#[test]
fn the_request_carries_an_exact_width_and_the_annotations_flag() {
    let rig = Rig::pixels();
    let image = block_on(rig.render.render_page(LARGE, scale(2.5), token())).unwrap();
    // 2.5 device px a point across a 4pt page.
    assert_eq!((image.width, image.height, image.stride), (10, 5, 40));
    assert_eq!(image.scale, 2.5);
    assert_eq!(image.pixels.len(), 200);
    let options =
        RenderPageOptions { width: Some(6.0), scale: Some(9.0), include_annotations: Some(false) };
    let image = block_on(rig.render.render_page(LARGE, options, token())).unwrap();
    // `width` wins over `scale`.
    assert_eq!((image.width, image.height, image.scale), (6, 3, 1.5));
    assert_eq!(
        rig.requests(),
        vec![
            (
                4,
                json!({ "viewport": { "kind": "width", "width": 10.0 }, "includeAnnotations": true })
            ),
            (
                4,
                json!({ "viewport": { "kind": "width", "width": 6.0 }, "includeAnnotations": false })
            ),
        ]
    );
}

#[test]
fn a_repeated_render_is_served_from_the_raster_store() {
    let rig = Rig::pixels();
    let first = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap();
    let second = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap();
    assert_eq!(first, second);
    assert_eq!(rig.requests().len(), 1);
}

#[test]
fn a_bad_scale_is_refused() {
    let rig = Rig::pixels();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let error = block_on(rig.render.render_page(SMALL, scale(bad), token())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
        let width = RenderPageOptions { width: Some(bad), ..Default::default() };
        let error = block_on(rig.render.render_page(SMALL, width, token())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
        let error = block_on(rig.render.render_thumbnail(SMALL, bad, None, token())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
    }
    assert!(rig.requests().is_empty());
}

#[test]
fn a_page_the_document_lacks_is_not_found() {
    let rig = Rig::pixels();
    let missing = PageRef { page_object_number: 99 };
    let error = block_on(rig.render.render_page(missing, scale(1.0), token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_thumbnail_renders_at_its_width() {
    let rig = Rig::pixels();
    let image = block_on(rig.render.render_thumbnail(LARGE, 2.0, Some(false), token())).unwrap();
    assert_eq!((image.width, image.height, image.scale), (2, 1, 0.5));
    assert_eq!(rig.requests()[0].1["includeAnnotations"], json!(false));
}

#[test]
fn a_batch_renders_in_order_and_reports_each_failure() {
    let rig = Rig::pixels();
    let missing = PageRef { page_object_number: 99 };
    let batch = block_on(rig.render.render_pages(
        vec![LARGE, missing, SMALL],
        scale(1.0),
        Some(1),
        token(),
    ))
    .unwrap();
    let applied: Vec<_> = batch.applied.iter().map(|e| (e.page, e.image.width)).collect();
    assert_eq!(applied, vec![(LARGE, 4), (SMALL, 2)]);
    assert_eq!(batch.failed.len(), 1);
    assert_eq!(batch.failed[0].page, missing);
    assert_eq!(batch.failed[0].error.code, "not-found");
    assert_eq!(batch.failed[0].error.capability, "render");
}

#[test]
fn a_batch_refuses_a_bad_scale_before_it_starts() {
    let rig = Rig::pixels();
    let error =
        block_on(rig.render.render_pages(vec![SMALL], scale(0.0), None, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(rig.requests().is_empty());
}

#[test]
fn an_answer_that_is_not_raw_pixels_is_invalid_state() {
    let encoded = json!({
        "width": 2, "height": 1,
        "source": { "kind": "bytes", "bytes": { "$byteLength": 10 } },
        "$host": 1
    });
    let rig = Rig::new(Engine::Value(encoded));
    let error = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn pixels_the_host_does_not_hold_are_invalid_state() {
    let raster = json!({
        "width": 2, "height": 1, "stride": 8, "color": "rgba8", "premultipliedAlpha": false,
        "source": { "kind": "pixels", "$byteLength": 8 },
        "$host": 77
    });
    let rig = Rig::new(Engine::Value(raster));
    let error = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn an_unknown_pixel_format_is_invalid_state() {
    for (color, premultiplied) in [("bgra8", false), ("rgba8", true)] {
        let raster = json!({
            "width": 2, "height": 1, "stride": 8,
            "color": color, "premultipliedAlpha": premultiplied,
            "source": { "kind": "pixels", "$byteLength": 8 },
            "$host": 1
        });
        let rig = Rig::new(Engine::Value(raster));
        let error = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-state"), "{color}: {error:?}");
    }
}

#[test]
fn dropping_a_render_aborts_the_engine_call() {
    let rig = Rig::new(Engine::Deferred);
    let mut future = Box::pin(rig.render.render_page(SMALL, scale(1.0), token()));
    support::start(&mut future);
    rig.ping();
    assert_eq!(rig.aborts(), 0);
    drop(future);
    rig.ping();
    assert_eq!(rig.aborts(), 1);
}

#[test]
fn cancelling_a_render_rejects_operation_cancelled() {
    let rig = Rig::new(Engine::Deferred);
    let cancel = token();
    let mut future = Box::pin(rig.render.render_page(SMALL, scale(1.0), cancel.clone()));
    support::start(&mut future);
    rig.ping();
    cancel.cancel();
    let error = block_on(future).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert_eq!(rig.aborts(), 1);
}

#[test]
fn cancelling_a_batch_rejects_operation_cancelled() {
    let rig = Rig::new(Engine::Deferred);
    let cancel = token();
    let mut future =
        Box::pin(rig.render.render_pages(vec![SMALL, LARGE], scale(1.0), None, cancel.clone()));
    support::start(&mut future);
    rig.ping();
    cancel.cancel();
    let error = block_on(future).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert_eq!(rig.aborts(), 2);
}

#[test]
fn invalidating_bumps_the_epoch_and_is_observed() {
    let rig = Rig::pixels();
    let events = Arc::new(Events::default());
    let _subscription = rig.render.subscribe(events.clone());
    assert_eq!(block_on(rig.render.render_epoch(SMALL, None)).unwrap(), 0);
    block_on(rig.render.invalidate(Some(vec![SMALL]), None)).unwrap();
    assert_eq!(block_on(rig.render.render_epoch(SMALL, None)).unwrap(), 1);
    assert_eq!(block_on(rig.render.render_epoch(SMALL, Some(false))).unwrap(), 1);
    assert_eq!(block_on(rig.render.render_epoch(LARGE, None)).unwrap(), 0);
    block_on(rig.render.invalidate(None, Some(RenderInvalidateScope::Annotations))).unwrap();
    // Annotations only: the bare raster is unchanged.
    assert_eq!(block_on(rig.render.render_epoch(LARGE, Some(false))).unwrap(), 0);
    assert_eq!(block_on(rig.render.render_epoch(LARGE, Some(true))).unwrap(), 1);
    assert_eq!(
        events.taken(),
        vec![
            RenderEvent::Invalidated { pages: vec![SMALL], scope: RenderInvalidateScope::Content },
            RenderEvent::Invalidated {
                pages: vec![SMALL, LARGE],
                scope: RenderInvalidateScope::Annotations
            },
        ]
    );
}

#[test]
fn a_confirmed_annotation_change_is_observed() {
    let rig = Rig::pixels();
    let events = Arc::new(Events::default());
    let _subscription = rig.render.subscribe(events.clone());
    rig.ping();
    rig.emit(json!({ "type": "annotation.updated", "page": { "pageObjectNumber": 4 } }));
    assert_eq!(
        events.taken(),
        vec![RenderEvent::Invalidated {
            pages: vec![LARGE],
            scope: RenderInvalidateScope::Annotations
        }]
    );
}

#[test]
fn a_completed_render_is_observed() {
    let rig = Rig::pixels();
    let events = Arc::new(Events::default());
    let _subscription = rig.render.subscribe(events.clone());
    rig.ping();
    block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap();
    let taken = events.taken();
    let [RenderEvent::Completed { page, key }] = &taken[..] else { panic!("{taken:?}") };
    assert_eq!(*page, SMALL);
    assert!(key.starts_with("3|w2|"), "{key}");
}

#[test]
fn a_failed_render_is_observed() {
    let rig = Rig::new(Engine::Error(json!({ "name": "RuntimeUnavailable", "message": "boom" })));
    let events = Arc::new(Events::default());
    let _subscription = rig.render.subscribe(events.clone());
    rig.ping();
    assert!(block_on(rig.render.render_page(SMALL, scale(1.0), token())).is_err());
    let taken = events.taken();
    let [RenderEvent::Failed { page, error, .. }] = &taken[..] else { panic!("{taken:?}") };
    assert_eq!(*page, SMALL);
    assert_eq!(error.capability, "render");
    assert!(error.message.contains("boom"), "{error:?}");
}

#[test]
fn the_pixels_outlive_the_release() {
    let rig = Rig::releasing_closed_sessions();
    let image = block_on(rig.render.render_page(SMALL, scale(1.0), token())).unwrap();
    assert!(rig.released().is_empty(), "the raster store still holds the image");
    rig.close();
    // The handle still holds the capability, and with it the store.
    rig.ping();
    assert!(rig.released().is_empty());
    let released = rig.released_log();
    let Rig { thread, render, .. } = rig;
    drop(render);
    // Dropping the last handle posts the slab removal; wait for it.
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(*released.lock().unwrap(), vec![1]);
    assert_eq!(image.pixels, [RED, BLUE].concat());
}

#[test]
fn can_render_answers_the_documents_permission() {
    let rig = Rig::pixels();
    assert!(block_on(rig.render.can_render()).unwrap());
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let rig = Rig::pixels();
    rig.close();
    let render = &rig.render;
    let errors = [
        block_on(render.can_render()).map(|_| ()).unwrap_err(),
        block_on(render.render_page(SMALL, scale(1.0), token())).map(|_| ()).unwrap_err(),
        block_on(render.render_thumbnail(SMALL, 2.0, None, token())).map(|_| ()).unwrap_err(),
        block_on(render.render_pages(vec![SMALL], scale(1.0), None, token()))
            .map(|_| ())
            .unwrap_err(),
        block_on(render.render_epoch(SMALL, None)).map(|_| ()).unwrap_err(),
        block_on(render.invalidate(None, None)).unwrap_err(),
    ];
    for error in errors {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    assert!(rig.requests().is_empty());
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let rig = Rig::pixels();
    let events = Arc::new(Events::default());
    let subscription = rig.render.subscribe(events.clone());
    rig.ping();
    assert_eq!(events.ends(), 0);
    rig.close();
    assert_eq!(events.ends(), 1);
    subscription.cancel();
    rig.ping();
    assert_eq!(events.ends(), 1);
}

#[test]
fn subscribing_after_the_close_ends_at_once() {
    let rig = Rig::pixels();
    rig.close();
    let events = Arc::new(Events::default());
    let _subscription = rig.render.subscribe(events.clone());
    rig.ping();
    assert_eq!(events.ends(), 1);
}

use epdf_plugin_render::ffi::{RenderPageViewDemand, RenderPolicy, RenderViewport};

#[test]
fn the_default_policy_is_continuous_and_a_viewport_conforms_to_a_width() {
    let rig = Rig::pixels();
    assert_eq!(block_on(rig.render.render_policy()).unwrap(), RenderPolicy::Continuous);
    let viewport = block_on(rig.render.conform_viewport(LARGE, 2.0)).unwrap();
    assert_eq!(viewport, RenderViewport::Width { width: 8.0 });
}

#[test]
fn a_source_key_names_the_raster_render_source_produces() {
    let rig = Rig::pixels();
    let a = block_on(rig.render.source_key(LARGE, 2.0, None)).unwrap();
    let b = block_on(rig.render.source_key(LARGE, 2.0, Some(true))).unwrap();
    let c = block_on(rig.render.source_key(LARGE, 3.0, None)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
    let image = block_on(rig.render.render_source(LARGE, 2.0, None, token())).unwrap();
    assert_eq!((image.width, image.height), (8, 4));
}

#[test]
fn a_page_not_in_the_document_is_not_found() {
    let rig = Rig::pixels();
    let missing = PageRef { page_object_number: 99 };
    let error = block_on(rig.render.source_key(missing, 1.0, None)).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_view_demand_plans_and_disposes() {
    let rig = Rig::pixels();
    let paint = block_on(rig.render.paint_settings()).unwrap();
    let view = block_on(rig.render.create_view_demand("main".into())).unwrap();
    let demand = RenderPageViewDemand {
        desired_device_width: 8.0,
        visible_rect: None,
        velocity_x: None,
        velocity_y: None,
    };
    block_on(view.set_demand(LARGE, demand, None)).unwrap();
    let plan = block_on(view.plan(LARGE)).unwrap();
    // The full-page raster already supplies 8 device pixels across, so the
    // tile plane stays disengaged whether or not tiles are on.
    assert!(!plan.engaged && plan.paint.is_empty(), "{plan:?} (tiles {})", paint.tiles);
    assert!(block_on(view.tile_image(LARGE, "no-such-key".into())).unwrap().is_none());
    block_on(view.release(LARGE)).unwrap();
    block_on(view.dispose()).unwrap();
}

#[test]
fn a_closed_documents_view_demand_refuses() {
    let rig = Rig::pixels();
    let view = block_on(rig.render.create_view_demand("main".into())).unwrap();
    rig.close();
    let error = block_on(view.plan(LARGE)).unwrap_err();
    assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
}
