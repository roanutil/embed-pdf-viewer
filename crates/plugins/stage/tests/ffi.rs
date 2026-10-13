#![cfg(feature = "uniffi")]

use epdf_ffi::{StageAlign, StageAlignValue, ZoomMode, ZoomSpec};
use epdf_plugin_stage::ffi::{
    StageArrivalAlignValue, StageGap, StageGridColumns, StageRevealOptions, StageRevealZoom,
    StageSettings, StageSettingsPatch, StageZoomTarget,
};
use epdf_plugin_stage::{PartialSettings, RevealOptions, ZoomTarget, default_settings};

#[test]
fn the_default_settings_round_trip() {
    let settings = default_settings();
    let ffi = StageSettings::try_from(settings.clone()).unwrap();
    assert_eq!(epdf_plugin_stage::StageSettings::try_from(ffi).unwrap(), settings);
}

#[test]
fn a_patch_sets_only_what_it_names() {
    let patch = StageSettingsPatch {
        padding: Some(8.0),
        gap: Some(StageGap::Px { px: 4.0 }),
        columns: Some(StageGridColumns::Count { count: 3 }),
        zoom: Some(ZoomSpec::Mode { mode: ZoomMode::FitPage }),
        arrival_align: None,
        ..Default::default()
    };
    let partial = PartialSettings::try_from(patch).unwrap();
    assert_eq!(partial.padding, Some(8.0));
    assert_eq!(partial.gap, Some(epdf_plugin_stage::Gap::Px { px: 4.0 }));
    assert_eq!(partial.columns, Some(epdf_plugin_stage::GridColumns::Count(3.0)));
    assert!(partial.flow.is_none() && partial.arrival_align.is_none());
}

#[test]
fn a_fractional_column_count_is_invalid_state_not_a_round() {
    let mut settings = default_settings();
    settings.columns = epdf_plugin_stage::GridColumns::Count(2.5);
    let error = StageSettings::try_from(settings).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

#[test]
fn keep_survives_the_arrival_alignment() {
    let keep = StageArrivalAlignValue::Keep;
    let core: epdf_plugin_stage::ArrivalAlignValue = keep.into();
    assert_eq!(core, epdf_plugin_stage::ArrivalAlignValue::KEEP);
    let named =
        StageArrivalAlignValue::Align { value: StageAlignValue::Named { align: StageAlign::End } };
    assert_eq!(
        StageArrivalAlignValue::from(epdf_plugin_stage::ArrivalAlignValue::from(named)),
        named
    );
}

#[test]
fn reveal_options_and_zoom_targets_convert() {
    let options = StageRevealOptions {
        behavior: None,
        rect: Some(epdf_ffi::Rect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }),
        zoom: Some(StageRevealZoom::Level { level: 2.0 }),
        anchor: None,
    };
    let core = RevealOptions::from(options.clone());
    assert_eq!(core.zoom, Some(epdf_plugin_stage::RevealZoom::Level { level: 2.0 }));
    assert_eq!(StageRevealOptions::from(core), options);
    assert_eq!(ZoomTarget::from(StageZoomTarget::Level { level: 1.25 }), ZoomTarget::Level(1.25));
}

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, KernelThread, PageRef, Point, Size, ViewerError};
use epdf_plugin_stage::ffi::{
    StageConfig, StageGoToOptions, StageHandle, StageInitialViewProvider, StageLayout,
    StageScrollBehavior, StageViewState, StageZoomMode,
};
use epdf_plugin_stage::{STAGE, StagePluginOptions, stage_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, TestSpec};
use serde_json::json;

const PAGE_2: PageRef = PageRef { page_object_number: 2 };

fn three_pages() -> TestSpec {
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let page = |pon: u32, index: u32| {
        json!({ "index": index, "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
            "label": null, "size": { "width": 600, "height": 800 }, "rotation": 0,
            "userUnit": 1, "boxes": { "media": box_, "crop": box_ } })
    };
    TestSpec::new(json!({ "documents": { DOCUMENT: {
        "id": DOCUMENT, "security": { "allows": "*" },
        "pages": [page(1, 0), page(2, 1), page(3, 2)]
    } } }))
}

type Reports = Arc<Mutex<Vec<String>>>;

fn handle_with(config: StageConfig) -> (KernelThread<TestKernel>, Arc<StageHandle>, Reports) {
    let thread = KernelThread::spawn(move |_| {
        let config = config.into_config()?;
        TestKernel::open(
            vec![stage_plugin(StagePluginOptions { config, ..Default::default() })],
            three_pages(),
        )
    })
    .unwrap();
    let post = thread.post();
    let reports: Reports = Arc::default();
    let sink = reports.clone();
    let handle = block_on(thread.call(move |k| {
        let report =
            Rc::new(move |e: &epdf_kernel::Error| sink.lock().unwrap().push(e.message.clone()));
        StageHandle::new(k.capability(&STAGE).unwrap(), post, report)
    }))
    .unwrap();
    (thread, handle, reports)
}

fn frameless() -> (KernelThread<TestKernel>, Arc<StageHandle>, Reports) {
    handle_with(StageConfig { frames: false, ..Default::default() })
}

fn sized(stage: &StageHandle) {
    block_on(stage.set_viewport_size(Size { width: 400.0, height: 600.0 })).unwrap();
}

fn instant() -> Option<StageGoToOptions> {
    Some(StageGoToOptions { behavior: Some(StageScrollBehavior::Instant), ..Default::default() })
}

fn drain(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|_| ())).unwrap();
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn a_zero_viewport_places_nothing_until_it_has_a_size() {
    let (_thread, stage, _) = frameless();
    block_on(stage.set_viewport_size(Size { width: 0.0, height: 0.0 })).unwrap();
    assert!(block_on(stage.visible_pages()).unwrap().is_empty());
    sized(&stage);
    let visible = block_on(stage.visible_pages()).unwrap();
    assert!(!visible.is_empty());
    assert_eq!(visible[0].page_index, 0);
    assert!(visible[0].transform.render_scale > 0.0);
}

#[test]
fn navigation_moves_the_current_page() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    assert!(block_on(stage.can_go_next()).unwrap());
    assert!(!block_on(stage.can_go_previous()).unwrap());
    block_on(stage.go_to_page(PAGE_2, instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 1);
    assert_eq!(block_on(stage.current_page()).unwrap().map(|p| p.page), Some(PAGE_2));
    block_on(stage.go_to_last_page(instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
    assert!(!block_on(stage.can_go_next()).unwrap());
}

#[test]
fn an_index_past_the_end_lands_on_the_last_page() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.go_to_page_index(99, instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
}

#[test]
fn fit_width_sets_the_mode_and_zoom_in_leaves_it() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.fit_width()).unwrap();
    assert_eq!(
        block_on(stage.zoom_mode()).unwrap(),
        StageZoomMode::Mode { mode: epdf_ffi::ZoomMode::FitWidth }
    );
    let fitted = block_on(stage.zoom_level()).unwrap();
    block_on(stage.zoom_in()).unwrap();
    assert!(block_on(stage.zoom_level()).unwrap() > fitted);
    assert_eq!(block_on(stage.zoom_mode()).unwrap(), StageZoomMode::Custom);
}

#[test]
fn page_and_viewport_points_round_trip() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    let page = PageRef { page_object_number: 1 };
    let point = Point { x: 100.0, y: 200.0 };
    let on_screen = block_on(stage.page_to_viewport(page, point)).unwrap().unwrap();
    let back = block_on(stage.viewport_to_page(page, on_screen)).unwrap().unwrap();
    assert!((back.x - point.x).abs() < 1e-6 && (back.y - point.y).abs() < 1e-6, "{back:?}");
    let hit = block_on(stage.page_at(on_screen)).unwrap().unwrap();
    assert_eq!(hit.page, page);
}

#[test]
fn settings_update_and_reset() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.set_layout(StageLayout::Horizontal)).unwrap();
    assert_eq!(block_on(stage.settings()).unwrap().layout, StageLayout::Horizontal);
    block_on(stage.reset_settings()).unwrap();
    assert_eq!(block_on(stage.settings()).unwrap().layout, StageLayout::Vertical);
}

#[test]
fn a_view_state_restores_where_it_was_taken() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.go_to_page_index(2, instant())).unwrap();
    let saved: StageViewState = block_on(stage.view_state()).unwrap();
    block_on(stage.go_to_first_page(instant())).unwrap();
    block_on(stage.apply_view_state(saved.clone())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
}

struct Restore(StageViewState);
impl StageInitialViewProvider for Restore {
    fn initial_view(&self) -> Result<Option<StageViewState>, HostError> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn an_initial_view_provider_places_the_first_view() {
    let (_thread, stage, _) = frameless();
    // A state taken from a sized stage, then handed to a fresh one.
    let (_t2, other, _) = frameless();
    sized(&other);
    block_on(other.go_to_page_index(1, instant())).unwrap();
    let saved = block_on(other.view_state()).unwrap();
    let _registration =
        block_on(stage.provide_initial_view(10.0, Arc::new(Restore(saved)))).unwrap();
    sized(&stage);
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 1);
}

#[test]
fn concurrent_pans_all_land() {
    // The same 100 one-point pans, from one thread and from four. A finger
    // moving up (negative dy) scrolls down; the camera starts at the top,
    // where a positive dy is clamped away.
    let pan = |stage: &Arc<StageHandle>, threads: usize| {
        block_on(stage.begin_gesture(None)).unwrap();
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let stage = stage.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 / threads {
                        stage.pan_by(0.0, -1.0);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        block_on(stage.end_gesture()).unwrap();
    };
    let (serial_thread, serial, _) = frameless();
    sized(&serial);
    pan(&serial, 1);
    drain(&serial_thread);
    let (thread, stage, reports) = frameless();
    sized(&stage);
    let start = block_on(stage.camera()).unwrap();
    pan(&stage, 4);
    drain(&thread);
    let end = block_on(stage.camera()).unwrap();
    assert!(reports.lock().unwrap().is_empty(), "{:?}", reports.lock().unwrap());
    assert_ne!(end, start);
    assert_eq!(end, block_on(serial.camera()).unwrap());
}

#[test]
fn motion_in_flight_at_close_stops() {
    let (thread, stage, reports) = handle_with(StageConfig::default());
    sized(&stage);
    block_on(stage.fling(0.0, -4000.0)).unwrap();
    assert!(block_on(stage.is_moving()).unwrap());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    // Let a few frames' worth of timers come due on the kernel thread.
    std::thread::sleep(std::time::Duration::from_millis(100));
    drain(&thread);
    assert!(reports.lock().unwrap().is_empty(), "{:?}", reports.lock().unwrap());
    let error = block_on(stage.camera()).unwrap_err();
    assert_eq!(code(&error), Some("instance-closed"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, stage, _) = frameless();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(stage.zoom_level()).map(|_| ()).unwrap_err(),
        block_on(stage.fit_width()).unwrap_err(),
        block_on(stage.go_to_page_index(1, None)).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A fire after the close is dropped, not a panic.
    stage.pan_by(1.0, 1.0);
    drain(&thread);
}
