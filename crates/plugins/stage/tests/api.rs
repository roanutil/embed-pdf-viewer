//! The typed surface the vectors can't reach: lens identity, motion on the
//! host clock and its cancellation at close, the current destination, the
//! order of the viewport report's events, the paged-empty camera refusal,
//! and the kept Rust-only empty scenes (M27, R5).

use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::{Point, Rect, Size};
use epdf_kernel::clock::HostClock;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Error, Kernel, KernelOptions, Unsubscribe, rt};
use epdf_plugin_stage::*;
use epdf_stage_core::Camera;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// The scenario host's clock: runtime timers, and 16 ms frames when asked.
struct FrameClock(bool);

impl HostClock for FrameClock {
    fn now(&self) -> f64 {
        rt::now()
    }
    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> Unsubscribe {
        let timer = rt::set_timeout(ms, run);
        Unsubscribe::new(move || rt::clear_timer(timer))
    }
    fn has_frames(&self) -> bool {
        self.0
    }
    fn next_frame(&self, run: Box<dyn FnOnce(f64)>) -> Unsubscribe {
        self.after(16.0, Box::new(move || run(rt::now())))
    }
}

struct Fixture {
    kernel: Kernel,
    reports: Rc<RefCell<Vec<String>>>,
}

impl Fixture {
    fn stage(&self) -> StageCapability {
        self.kernel.capability(&STAGE, Some("a")).expect("stage")
    }
}

fn page(pon: u32, index: u32) -> Value {
    json!({ "index": index, "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null, "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1 })
}

/// A kernel over `pages` (three 600x800 pages by default), with frames or
/// without, the stage registered with `settings`.
fn fixture(frames: bool, pages: Vec<Value>, settings: StageSettingsPatch) -> Fixture {
    rt::reset();
    let fake = FakeEngine::new(json!({ "documents": { "a": { "pages": pages } } }));
    let reports = Rc::new(RefCell::new(Vec::new()));
    let log = reports.clone();
    let options = KernelOptions {
        report: Some(Rc::new(move |error: &Error| log.borrow_mut().push(error.to_string()))),
        ..Default::default()
    };
    let config = StageConfig { settings, ..Default::default() };
    let kernel = Kernel::new_with_clock(
        Rc::new(AsEngine(fake)),
        vec![stage_plugin(StagePluginOptions { config, ..Default::default() })],
        options,
        Rc::new(FrameClock(frames)),
    )
    .expect("kernel");
    kernel.start();
    rt::run_until_stalled();
    kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    Fixture { kernel, reports }
}

fn three_pages() -> Vec<Value> {
    vec![page(1, 0), page(2, 1), page(3, 2)]
}

fn units() -> StageSettingsPatch {
    StageSettingsPatch { view_units_per_point: Some(1.0), ..Default::default() }
}

const VIEWPORT: Size = Size { width: 1000.0, height: 700.0 };

#[test]
fn handles_to_one_lens_are_the_same_lens() {
    let one = fixture(false, three_pages(), units());
    let a = one.stage();
    let b = one.stage();
    assert!(a.same(&b));
    assert!(a.same(&a.clone()));
    let other = fixture(false, three_pages(), units());
    assert!(!a.same(&other.stage()));
}

#[test]
fn motion_runs_on_frames_and_stops_when_the_document_closes() {
    let f = fixture(true, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    let ended = Rc::new(RefCell::new(0));
    let count = ended.clone();
    let _off = stage.on_motion_ended(move |_| {
        *count.borrow_mut() += 1;
        Ok(())
    });
    stage.go_to_page(2usize, Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth)));
    assert!(stage.is_moving(), "a smooth arrival waits for a frame");
    let start = stage.get_camera();
    rt::advance(16.0);
    assert_eq!(stage.get_camera(), start, "the first frame anchors the tween's start");
    rt::advance(16.0);
    assert!(stage.is_moving());
    assert_ne!(stage.get_camera(), start, "the next frame moved the camera");
    rt::advance(400.0);
    assert!(!stage.is_moving());
    assert_eq!(*ended.borrow(), 1, "a tween ends once");

    // A tween, a rest countdown and a fling are all pending at close.
    stage.go_to_page(0usize, Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth)));
    stage.zoom_in().expect("zoom arms the rest countdown");
    stage.fling(0.0, -3000.0).expect("fling");
    assert!(stage.is_moving());
    f.kernel.documents().close("a");
    rt::run_until_stalled();
    rt::advance(10_000.0);
    assert_eq!(rt::pending_timers(), 0, "the instance clock cancelled every frame and timer");
    assert!(
        f.reports.borrow().is_empty(),
        "no frame wrote to the closed instance: {:?}",
        f.reports.borrow()
    );
    assert_eq!(*ended.borrow(), 1, "a cancelled motion announces no end");
}

#[test]
fn without_frames_navigation_jumps_and_stop_motion_is_a_no_op() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    stage.go_to_page(2usize, Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth)));
    assert!(!stage.is_moving());
    assert_eq!(stage.get_current_page_index(), 2);
    let ended = Rc::new(RefCell::new(0));
    let count = ended.clone();
    let _off = stage.on_motion_ended(move |_| {
        *count.borrow_mut() += 1;
        Ok(())
    });
    stage.stop_motion();
    assert_eq!(*ended.borrow(), 0);
}

#[test]
fn stopping_a_tween_ends_it_where_it_is() {
    let f = fixture(true, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    let ended = Rc::new(RefCell::new(Vec::new()));
    let log = ended.clone();
    let _off = stage.on_motion_ended(move |event| {
        log.borrow_mut().push(event.camera);
        Ok(())
    });
    stage.go_to_page(2usize, Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth)));
    rt::advance(48.0);
    let mid = stage.get_camera();
    stage.stop_motion();
    assert!(!stage.is_moving());
    assert_eq!(*ended.borrow(), vec![mid]);
    rt::advance(1_000.0);
    assert_eq!(stage.get_camera(), mid, "nothing moves after the catch");
}

#[test]
fn the_current_destination_is_where_the_view_is() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    assert_eq!(stage.get_current_destination(), None, "no page is on screen before placement");
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    stage
        .scroll_by(StageScrollToOptions { top: Some(900.0), ..Default::default() })
        .expect("scroll");
    let Some(PageDestination::Xyz { page, x, y, zoom }) = stage.get_current_destination() else {
        panic!("an xyz destination");
    };
    assert_eq!(page, PageRef::new(2), "page 1 ends above the padding line");
    assert_eq!(zoom, None, "following it keeps the reader's zoom");
    let (x, y) = (x.expect("x"), y.expect("y"));
    assert!((0.0..=600.0).contains(&x) && (0.0..=800.0).contains(&y));
    assert_eq!(x, (x * 100.0).round() / 100.0, "rounded to hundredths");
    // Going there lands the same spot at the viewport's padded top-left.
    let camera = stage.get_camera();
    stage.scroll_to(StageScrollToOptions { top: Some(0.0), ..Default::default() }).expect("top");
    stage.go_to_destination(
        &PageDestination::Xyz { page, x: Some(x), y: Some(y), zoom: None },
        Some(GoToDestinationOptions { behavior: Some(ScrollBehaviorKind::Instant) }),
    );
    assert_eq!(stage.get_current_page_index(), 1);
    assert!((stage.get_camera().y - camera.y).abs() < 0.01);
}

#[test]
fn a_destination_before_placement_lands_with_the_initial_view() {
    let f = fixture(true, three_pages(), units());
    let stage = f.stage();
    let fit = PageDestination::Fit { page: PageRef::new(3) };
    stage.go_to_destination(
        &fit,
        Some(GoToDestinationOptions { behavior: Some(ScrollBehaviorKind::Smooth) }),
    );
    assert_eq!(stage.get_current_page_index(), 0, "nothing moves before a viewport");
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    assert!(!stage.is_moving(), "a pending navigation lands instantly with placement");
    assert_eq!(stage.get_current_page_index(), 2);
    assert!(stage.get_page_frame(PageRef::new(3)).is_some());
    // A destination on a page the document doesn't have is a no-op.
    let camera = stage.get_camera();
    stage.go_to_destination(&PageDestination::Fit { page: PageRef::new(99) }, None);
    assert_eq!(stage.get_camera(), camera);
    assert_eq!(stage.get_current_page_index(), 2);
}

#[test]
fn a_viewport_report_announces_the_size_before_the_rules() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    let order = Rc::new(RefCell::new(Vec::new()));
    let log = order.clone();
    let _a = stage.on_viewport_changed(move |event| {
        log.borrow_mut().push(format!("viewport {}", event.size.width));
        Ok(())
    });
    let log = order.clone();
    let _b = stage.on_settings_changed(move |event| {
        log.borrow_mut().push(format!("settings {:?}", event.changed));
        Ok(())
    });
    let log = order.clone();
    let _c = stage.on_camera_changed(move |_| {
        log.borrow_mut().push("camera".into());
        Ok(())
    });
    // Crossing into the default compact rule: the size lands, the rule's
    // padding follows, and the reframe moves the camera last.
    stage.set_viewport_size(Size { width: 500.0, height: 700.0 }).expect("compact");
    assert_eq!(*order.borrow(), vec!["viewport 500", "settings [Padding]", "camera"]);
    assert!(stage.matches_rule("compact"));
    // A report of the same size, or of no area, says nothing new.
    order.borrow_mut().clear();
    stage.set_viewport_size(Size { width: 500.0, height: 700.0 }).expect("same");
    stage.set_viewport_size(Size { width: 0.0, height: 0.0 }).expect("hidden");
    assert!(order.borrow().is_empty());
    assert_eq!(stage.get_viewport_size(), Size { width: 500.0, height: 700.0 });
}

#[test]
fn paged_flow_over_no_pages_refuses_camera_writes_as_the_typescript_does() {
    // The shipping TypeScript throws a TypeError here (scene.ts `itemRect`
    // reads `x` of the slice's missing item). Rust answers the same error,
    // checked rather than a panic, after the same earlier writes.
    let f = fixture(false, Vec::new(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    stage.place_initial();
    stage
        .update_settings(StageSettingsPatch { flow: Some(FlowMode::Paged), ..Default::default() })
        .expect("paged");
    let camera = stage.get_camera();
    let refused = |result: Result<(), Error>| {
        let error = result.expect_err("refused");
        assert_eq!(error.name, "TypeError");
        assert_eq!(error.message, "Cannot read properties of undefined (reading 'x')");
    };
    refused(stage.set_camera(Camera { x: 1.0, y: 2.0, zoom: 1.0 }));
    refused(stage.pan_by(5.0, 5.0));
    refused(stage.zoom_in());
    refused(stage.fling(0.0, 100.0));
    refused(stage.update_settings(StageSettingsPatch { padding: Some(8.0), ..Default::default() }));
    stage.begin_gesture(Some(GestureOptions { elastic: Some(true) }));
    refused(stage.pan_by(5.0, 5.0));
    refused(stage.end_gesture());
    assert_eq!(stage.get_camera(), camera, "no camera write landed");
    let state = f.kernel.state_json();
    assert_eq!(
        state["plugins"]["stage::a"]["motionCause"],
        json!("user"),
        "the cause landed before the refusal"
    );
    // The verbs the TypeScript answers succeed.
    stage.double_tap_zoom(Point { x: 5.0, y: 5.0 }).expect("double tap");
    stage.scroll_to(StageScrollToOptions { top: Some(5.0), ..Default::default() }).expect("scroll");
    assert!(stage.list_visible_pages().is_empty());
    assert!(stage.get_page_frame(0usize).is_none());
}

fn grid(columns: GridColumns, pages: Vec<Value>) -> (Fixture, StageCapability) {
    let f = fixture(false, pages, units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    stage
        .update_settings(StageSettingsPatch {
            layout: Some(LayoutKind::Grid),
            columns: Some(columns),
            ..Default::default()
        })
        .expect("grid");
    (f, stage)
}

#[test]
fn a_nan_column_count_empties_the_grid_instead_of_throwing() {
    // M27: the TypeScript throws a RangeError wherever the scene is built.
    let (_f, stage) = grid(GridColumns::Count(f64::NAN), three_pages());
    assert!(stage.list_visible_pages().is_empty());
    stage.set_camera(Camera { x: 0.0, y: 0.0, zoom: 1.0 }).expect("camera");
    stage.zoom_by(2.0, None).expect("zoom");
    stage.double_tap_zoom(Point { x: 10.0, y: 10.0 }).expect("double tap");
    stage.set_viewport_size(Size { width: 900.0, height: 700.0 }).expect("viewport");
    stage.get_viewpoint().expect("viewpoint");
    stage.go_to_page(2usize, None);
    stage.next_page(None);
    stage.fit_page().expect("fit");
    stage
        .update_settings(StageSettingsPatch {
            columns: Some(GridColumns::Count(2.0)),
            ..Default::default()
        })
        .expect("a whole count re-lays the grid");
    assert!(!stage.list_visible_pages().is_empty());
}

#[test]
fn an_unreadable_page_row_keeps_the_others_at_their_indexes() {
    // R5: page 2's row has no size. The TypeScript throws building any scene
    // over it; the scene here is empty instead, as for a NaN column count.
    let mut pages = three_pages();
    pages[1].as_object_mut().expect("row").remove("size");
    let f = fixture(false, pages, units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    assert!(stage.list_visible_pages().is_empty());
    stage.go_to_page(PageRef::new(3), None);
    assert_eq!(stage.get_current_page_index(), 2, "page 3 is still display index 2");
    stage.go_to_page(1usize, None);
    stage.next_page(None);
    stage.fit_page().expect("fit");
    stage.zoom_by(2.0, None).expect("zoom");
    stage.get_viewpoint().expect("viewpoint");
    assert!(stage.get_page_frame(PageRef::new(3)).is_none());
}

#[test]
fn a_page_argument_is_a_ref_or_a_whole_index() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    assert_eq!(stage.get_page_frame(1usize).map(|p| p.page_ref), Some(PageRef::new(2)));
    assert!(stage.get_page_frame(PageTarget::Index(1.5)).is_none());
    assert!(stage.get_page_frame(PageRef::new(9)).is_none());
    stage.go_to_page(PageTarget::Index(1.5), None);
    assert_eq!(stage.get_current_page_index(), 0, "a fractional index names no page");
    stage.go_to_page(PageTarget::Index(99.0), None);
    assert_eq!(stage.get_current_page_index(), 2, "an index past the end goes to the end");
    assert_eq!(stage.get_current_page(), Some(PageRef::new(3)));
    assert_eq!(stage.get_page_count(), 3);
}

#[test]
fn the_working_set_is_the_visible_and_the_near_pages() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    stage.set_viewport_size(Size { width: 1000.0, height: 500.0 }).expect("viewport");
    let pages = stage.list_visible_pages();
    let set = working_set_of(&pages);
    assert_eq!(set.len(), pages.len());
    let first = set[0];
    assert_eq!(first.role, WorkingSetRole::Visible);
    let visible: Rect = first.visible.expect("a visible part");
    let expected = visible.width * visible.height * pages[0].transform.render_scale.powi(2);
    assert_eq!(first.pixels, expected.round());
    for (entry, page) in set.iter().zip(pages.iter()) {
        let shown = page.visible_rect.width > 0.0 && page.visible_rect.height > 0.0;
        assert_eq!(entry.role == WorkingSetRole::Visible, shown);
        if !shown {
            assert_eq!((entry.visible, entry.pixels), (None, 0.0));
        }
    }
}

#[test]
fn closing_the_document_releases_providers_predicates_and_pending_navigation() {
    let f = fixture(false, three_pages(), units());
    let stage = f.stage();
    // Each closure holds the capability (the cycle) and a marker to count.
    let marker = Rc::new(());
    let (held, lens) = (marker.clone(), stage.clone());
    let _withdraw = stage.provide_initial_view(1.0, move || {
        let _ = (&held, &lens);
        None::<StageViewState>
    });
    let (held, lens) = (marker.clone(), stage.clone());
    stage
        .set_responsive_rules(vec![ResponsiveRule {
            name: Some("held".into()),
            when: RuleWhen::Predicate(Rc::new(move |_| {
                let _ = (&held, &lens);
                false
            })),
            settings: None,
        }])
        .expect("rules");
    stage.go_to_page(2usize, None); // pending until placement
    assert_eq!(Rc::strong_count(&marker), 3);
    f.kernel.documents().close("a");
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&marker), 1, "close let go of both closures");
}

#[test]
fn rule_predicates_debug_print_without_the_closure() {
    let rule = RuleWhen::Predicate(Rc::new(|_| true));
    assert_eq!(format!("{rule:?}"), "Predicate(fn)");
    let query = RuleWhen::Query(BoxQuery { max_width: Some(600.0), ..Default::default() });
    assert!(format!("{query:?}").starts_with("Query(BoxQuery"));
}
