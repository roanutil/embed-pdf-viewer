#![cfg(feature = "uniffi")]

use epdf_ffi::{PageRef, Rect};
use epdf_plugin_render::ffi::{RenderSnapshot, RenderSnapshotListener};
use futures::executor::block_on;
use std::sync::{Arc, Mutex};
mod support;
use support::{Engine, Rig, code, token};

#[derive(Default)]
struct Events(Mutex<Vec<Option<RenderSnapshot>>>, Mutex<usize>);
impl RenderSnapshotListener for Events {
    fn on_snapshot(&self, snapshot: Option<RenderSnapshot>) {
        self.0.lock().unwrap().push(snapshot);
    }
    fn on_end(&self) {
        *self.1.lock().unwrap() += 1;
    }
}

#[test]
fn metadata_and_changes_precede_pixels_and_readers_cancel_independently() {
    let rig = Rig::new(Engine::Deferred);
    assert!(block_on(rig.render.can_snapshot()).unwrap());
    assert!(block_on(rig.render.snapshot()).unwrap().is_none());
    let events = Arc::new(Events::default());
    let _sub = rig.render.subscribe_snapshots(events.clone());
    rig.capture(90.0);
    let snapshot = block_on(rig.render.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.page, PageRef { page_object_number: 4 });
    assert_eq!(snapshot.rect, Rect { x: 0.0, y: 0.0, width: 2.0, height: 1.0 });
    assert_eq!(block_on(rig.render.snapshot_box(snapshot.page)).unwrap(), Some(snapshot.rect));
    let from_event = events.0.lock().unwrap()[0].clone().unwrap();
    let cancel = token();
    let mut first = Box::pin(snapshot.image.image(cancel.clone()));
    let mut second = Box::pin(from_event.image.image(token()));
    support::start(&mut first);
    support::start(&mut second);
    rig.ping();
    cancel.cancel();
    assert_eq!(code(&block_on(first).unwrap_err()), Some("operation-cancelled"));
    assert_eq!(rig.aborts(), 0);
    block_on(rig.render.clear_snapshot()).unwrap();
    assert_eq!(code(&block_on(second).unwrap_err()), Some("operation-cancelled"));
    assert_eq!(rig.aborts(), 1);
    assert!(block_on(rig.render.snapshot()).unwrap().is_none());
    assert!(events.0.lock().unwrap()[1].is_none());
    rig.close();
    assert_eq!(*events.1.lock().unwrap(), 1);
    assert_eq!(
        code(&block_on(snapshot.image.image(token())).unwrap_err()),
        Some("instance-closed")
    );
}

#[test]
fn owned_snapshot_pixels_survive_clear_close_and_release_the_host_handle() {
    let rig = Rig::new(Engine::Deferred);
    rig.capture(90.0);
    let snapshot = block_on(rig.render.snapshot()).unwrap().unwrap();
    let other = block_on(rig.render.snapshot()).unwrap().unwrap();
    let cancelled = token();
    let mut first = Box::pin(snapshot.image.image(cancelled.clone()));
    let mut second = Box::pin(other.image.image(token()));
    support::start(&mut first);
    support::start(&mut second);
    rig.ping();
    cancelled.cancel();
    assert_eq!(code(&block_on(first).unwrap_err()), Some("operation-cancelled"));
    assert_eq!(rig.aborts(), 0);
    // Later settings do not change capture-time scale or orientation.
    block_on(rig.render.update_settings(epdf_plugin_render::ffi::RenderSettingsPatch {
        snapshot: Some(epdf_plugin_render::ffi::RenderSnapshotPatch {
            scale: Some(5.0),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    rig.resolve_pixels(2, 4);
    let pixels = block_on(second).unwrap();
    assert_eq!((pixels.width, pixels.height, pixels.scale), (2, 4, 2.0));
    assert_eq!(block_on(snapshot.image.image(token())).unwrap(), pixels);
    assert_eq!(rig.engine_log().iter().filter(|e| e["path"] == "page.render.image").count(), 1);
    assert!(rig.released().is_empty());
    block_on(rig.render.clear_snapshot()).unwrap();
    rig.ping();
    assert_eq!(rig.released(), [1], "foreign image handles must not retain host-table entries");
    rig.close();
    assert_eq!(pixels.pixels, support::pixels(2, 4, 8));
    assert_eq!(rig.released(), [1]);
}

#[test]
fn close_stops_a_pending_capture_and_ends_late_subscriptions() {
    let rig = Rig::new(Engine::Deferred);
    rig.capture(0.0);
    let snapshot = block_on(rig.render.snapshot()).unwrap().unwrap();
    let mut image = Box::pin(snapshot.image.image(token()));
    support::start(&mut image);
    rig.ping();
    rig.close();
    let error = block_on(image).unwrap_err();
    assert!(matches!(code(&error), Some("instance-closed" | "operation-cancelled")), "{error:?}");
    assert_eq!(rig.aborts(), 1);
    assert_eq!(code(&block_on(rig.render.clear_snapshot()).unwrap_err()), Some("instance-closed"));
    assert_eq!(
        code(&block_on(rig.render.snapshot_box(snapshot.page)).unwrap_err()),
        Some("instance-closed")
    );
    let events = Arc::new(Events::default());
    let _sub = rig.render.subscribe_snapshots(events.clone());
    rig.ping();
    assert_eq!(*events.1.lock().unwrap(), 1);
}

#[test]
fn capture_requires_both_render_and_copy_permission() {
    for rights in [
        serde_json::json!(["doc.render"]),
        serde_json::json!(["doc.content.copy"]),
        serde_json::json!([]),
    ] {
        let rig = Rig::permitted(rights);
        assert!(!block_on(rig.render.can_snapshot()).unwrap());
        rig.capture(0.0);
        assert!(block_on(rig.render.snapshot()).unwrap().is_none());
        assert!(rig.requests().is_empty());
    }
}

#[test]
fn snapshot_uses_geometry_at_render_admission_after_the_settlement_gate() {
    for (rotation, size) in [(0.0, (1, 2)), (90.0, (2, 1))] {
        let rig = Rig::new(Engine::Deferred);
        let events = Arc::new(Events::default());
        let _sub = rig.render.subscribe_snapshots(events.clone());
        // Both actions run in one kernel turn: metadata is published on
        // capture, then geometry changes before the settlement gate opens.
        rig.capture_before_resize(rotation, epdf_geometry::Point { x: 2.0, y: 2.0 }, Some(size));
        let snapshot = block_on(rig.render.snapshot()).unwrap().unwrap();
        let from_event = events.0.lock().unwrap()[0].clone().unwrap();
        let captured_rect = Rect { x: 0.0, y: 0.0, width: 2.0, height: 2.0 };
        assert_eq!(snapshot.rect, captured_rect);
        assert_eq!(from_event.rect, captured_rect);
        let requests: Vec<_> =
            rig.engine_log().into_iter().filter(|e| e["path"] == "page.render.image").collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["args"][0]["target"]["rect"],
            serde_json::json!({
                "x": 0.0, "y": 0.0, "width": f64::from(size.0), "height": f64::from(size.1)
            })
        );
        rig.resolve_pixels(2, 4);
        let image = block_on(snapshot.image.image(token())).unwrap();
        assert_eq!((image.width, image.height, image.scale), (2, 4, 2.0));
        assert_eq!(block_on(from_event.image.image(token())).unwrap(), image);
    }
}
