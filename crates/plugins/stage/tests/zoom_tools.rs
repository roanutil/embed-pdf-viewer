//! The zoom limits (upstream `e2ee5215f`), the marquee zoom (`a1ee5683f`)
//! and the settings locks (`4331b55a3`). The vectors record these only as
//! state defaults, so the behaviour is pinned here against the TypeScript.

use std::rc::Rc;

use epdf_geometry::{Point, Size};
use epdf_kernel::clock::HostClock;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, Unsubscribe, rt};
use epdf_plugin_interaction::{
    InteractionCapability, Modifiers, Phase, PointerKind, PointerSample, Tool,
};
use epdf_plugin_stage::*;
use epdf_stage_core::{ZoomMode, ZoomSpec};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

struct NoFrames;

impl HostClock for NoFrames {
    fn now(&self) -> f64 {
        rt::now()
    }
    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> Unsubscribe {
        let timer = rt::set_timeout(ms, run);
        Unsubscribe::new(move || rt::clear_timer(timer))
    }
    fn has_frames(&self) -> bool {
        false
    }
    fn next_frame(&self, run: Box<dyn FnOnce(f64)>) -> Unsubscribe {
        self.after(16.0, Box::new(move || run(rt::now())))
    }
}

fn page(pon: u32, index: u32) -> Value {
    json!({ "index": index, "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null, "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1 })
}

const VIEWPORT: Size = Size { width: 1000.0, height: 700.0 };

/// A placed stage over three 600x800 pages, 1 view unit per point, at 100%.
fn placed(settings: StageSettingsPatch) -> (Kernel, StageCapability) {
    rt::reset();
    let pages = vec![page(1, 0), page(2, 1), page(3, 2)];
    let fake = FakeEngine::new(json!({ "documents": { "a": { "pages": pages } } }));
    let settings = StageSettingsPatch { view_units_per_point: Some(1.0), ..settings };
    let config = StageConfig { settings, ..Default::default() };
    let kernel = Kernel::new_with_clock(
        Rc::new(AsEngine(fake)),
        vec![
            epdf_plugin_interaction::interaction_plugin(Default::default()),
            stage_plugin(StagePluginOptions { config, ..Default::default() }),
        ],
        KernelOptions::default(),
        Rc::new(NoFrames),
    )
    .expect("kernel");
    kernel.start();
    rt::run_until_stalled();
    kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    let stage: StageCapability = kernel.capability(&STAGE, Some("a")).expect("stage");
    stage.set_viewport_size(VIEWPORT).expect("viewport");
    (kernel, stage)
}

/// The stage's state as the kernel holds it: the object with a `motionCause`.
fn model(kernel: &Kernel) -> Value {
    fn find(value: &Value) -> Option<Value> {
        match value {
            Value::Object(map) if map.contains_key("motionCause") => Some(value.clone()),
            Value::Object(map) => map.values().find_map(find),
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(&kernel.state_json()).expect("a stage state")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ── zoom limits ──

#[test]
fn the_limits_default_to_the_widest_range() {
    let (kernel, stage) = placed(Default::default());
    let settings = stage.get_settings();
    assert_eq!((settings.min_zoom, settings.max_zoom), (0.05, 64.0));
    let state = model(&kernel);
    assert_eq!(state["minZoom"].as_f64(), Some(0.05));
    assert_eq!(state["maxZoom"].as_f64(), Some(64.0));
}

#[test]
fn zoom_steps_land_on_round_levels() {
    let (_k, stage) = placed(Default::default());
    assert_eq!(stage.get_zoom_level(), 1.0);
    stage.zoom_in(None).expect("in");
    assert!(close(stage.get_zoom_level(), 1.25));
    stage.zoom_in(None).expect("in");
    assert!(close(stage.get_zoom_level(), 1.5));
    stage.zoom_out(None).expect("out");
    stage.zoom_out(None).expect("out");
    stage.zoom_out(None).expect("out");
    assert!(close(stage.get_zoom_level(), 0.75));
    // Within 5% of a round level, a step goes past it: 98% zooms in to 125%.
    stage.zoom_to(ZoomTarget::Level(0.98), None).expect("level");
    stage.zoom_in(None).expect("in");
    assert!(close(stage.get_zoom_level(), 1.25));
}

#[test]
fn a_step_stops_at_the_limit_and_says_so() {
    let (_k, stage) = placed(StageSettingsPatch {
        min_zoom: Some(0.5),
        max_zoom: Some(1.4),
        ..Default::default()
    });
    assert!(stage.can_zoom_in() && stage.can_zoom_out());
    stage.zoom_in(None).expect("in");
    stage.zoom_in(None).expect("in");
    assert!(close(stage.get_zoom_level(), 1.4), "the next level, 150%, is past maxZoom");
    assert!(!stage.can_zoom_in());
    stage.set_camera(epdf_stage_core::Camera { x: 0.0, y: 0.0, zoom: 0.01 }).expect("camera");
    assert!(close(stage.get_zoom_level(), 0.5), "setCamera clamps into the limits");
    assert!(!stage.can_zoom_out());
}

#[test]
fn a_fit_or_a_pinch_outside_the_limits_stops_at_the_nearest_one() {
    let (_k, stage) = placed(StageSettingsPatch { max_zoom: Some(1.2), ..Default::default() });
    stage.fit_width().expect("fit");
    // (1000 − 2·24) / 600 is 1.59, past maxZoom.
    assert!(close(stage.get_zoom_level(), 1.2));
    stage.zoom_around(Point { x: 10.0, y: 10.0 }, 10.0).expect("pinch");
    assert!(close(stage.get_zoom_level(), 1.2));
    stage
        .update_settings(StageSettingsPatch { max_zoom: Some(1.0), ..Default::default() })
        .expect("limits");
    assert!(close(stage.get_zoom_level(), 1.0), "a new limit re-resolves the zoom");
}

fn limited() -> (Kernel, StageCapability) {
    placed(StageSettingsPatch { max_zoom: Some(1.2), ..Default::default() })
}

#[test]
fn a_reveal_of_a_tiny_rect_stops_at_the_limit() {
    let (_k, stage) = limited();
    stage.reveal(
        1usize,
        Some(RevealOptions {
            behavior: Some(ScrollBehaviorKind::Instant),
            rect: Some(epdf_geometry::Rect { x: 10.0, y: 10.0, width: 2.0, height: 2.0 }),
            zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
            ..Default::default()
        }),
    );
    assert!(stage.get_zoom_level() <= 1.2, "zoom {}", stage.get_zoom_level());
}

#[test]
fn the_double_tap_ladder_stops_at_the_limit() {
    let (_k, stage) = limited();
    for _ in 0..3 {
        stage.double_tap_zoom(Point { x: 500.0, y: 350.0 }).expect("tap");
        assert!(stage.get_zoom_level() <= 1.2 + 1e-9, "zoom {}", stage.get_zoom_level());
    }
}

#[test]
fn an_arrival_under_fit_width_stops_at_the_limit() {
    let (_k, stage) = limited();
    stage.fit_width().expect("fit");
    stage.go_to_page(1usize, Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)));
    assert!(stage.get_zoom_level() <= 1.2 + 1e-9, "zoom {}", stage.get_zoom_level());
}

// ── marquee zoom ──

#[test]
fn the_zoom_box_look_defaults_to_the_accent_and_a_patch_names_only_its_colors() {
    let (kernel, stage) = placed(Default::default());
    assert_eq!(stage.get_settings().zoom_box, ZoomBoxLook { fill: None, stroke: None });
    stage
        .update_settings(StageSettingsPatch {
            zoom_box: Some(PartialZoomBoxLook { fill: Some("red".into()), stroke: None }),
            ..Default::default()
        })
        .expect("look");
    assert_eq!(
        stage.get_settings().zoom_box,
        ZoomBoxLook { fill: Some("red".into()), stroke: None }
    );
    let state = model(&kernel);
    assert_eq!(state["zoomBox"], json!({ "fill": "red", "stroke": null }));
    assert_eq!(state["zoomDrag"], Value::Null);
}

#[test]
fn a_short_drag_shows_no_box_and_a_long_one_does() {
    let (kernel, stage) = placed(Default::default());
    let start = Point { x: 300.0, y: 200.0 };
    stage.begin_zoom_drag(start);
    let drag = model(&kernel)["zoomDrag"].clone();
    assert_eq!(drag["page"], json!({ "kind": "objectNumber", "objectNumber": 1 }));
    assert_eq!(drag["from"], drag["to"]);
    assert_eq!(stage.get_zoom_box(), None, "a press is a click until it moves");
    stage.move_zoom_drag(Point { x: 303.0, y: 202.0 });
    assert_eq!(stage.get_zoom_box(), None, "under 4 px on both axes is still a click");
    stage.move_zoom_drag(Point { x: 304.0, y: 200.0 });
    assert!(stage.get_zoom_box().is_some(), "4 px on one axis is a box");
    stage.move_zoom_drag(Point { x: 400.0, y: 250.0 });
    let zoom_box = stage.get_zoom_box().expect("a box");
    assert!(close(zoom_box.x, 300.0) && close(zoom_box.y, 200.0));
    assert!(close(zoom_box.width, 100.0) && close(zoom_box.height, 50.0));
    stage.cancel_zoom_drag();
    assert_eq!(model(&kernel)["zoomDrag"], Value::Null);
    assert_eq!(stage.get_zoom_box(), None);
}

#[test]
fn releasing_a_box_zooms_so_it_fills_the_padded_view() {
    let (kernel, stage) = placed(StageSettingsPatch {
        scroll_behavior: Some(ScrollBehaviorKind::Instant),
        ..Default::default()
    });
    stage.begin_zoom_drag(Point { x: 300.0, y: 200.0 });
    stage.move_zoom_drag(Point { x: 400.0, y: 250.0 });
    stage.end_zoom_drag(None).expect("zoom");
    // min((1000 − 48) / 100, (700 − 48) / 50) at 100%.
    let fit = f64::min(952.0 / 100.0, 652.0 / 50.0);
    assert!(close(stage.get_zoom_level(), fit));
    assert_eq!(stage.get_settings().zoom, ZoomSpec::Level { level: fit });
    assert_eq!(model(&kernel)["zoomDrag"], Value::Null);
}

#[test]
fn a_click_zooms_one_step_around_it_and_alt_zooms_out() {
    let (_k, stage) = travelling();
    let at = Point { x: 520.0, y: 330.0 };
    let world = stage.viewport_to_world(at);
    stage.begin_zoom_drag(at);
    stage.end_zoom_drag(None).expect("click");
    assert!(close(stage.get_zoom_level(), 3.0));
    let after = stage.viewport_to_world(at);
    assert!(close(after.x, world.x) && close(after.y, world.y), "the clicked point holds");
    stage.begin_zoom_drag(at);
    stage.end_zoom_drag(Some(EndZoomDragOptions { zoom_out: Some(true) })).expect("click");
    assert!(close(stage.get_zoom_level(), 2.0));
    // Without a drag, ending does nothing.
    stage.end_zoom_drag(None).expect("nothing");
    assert!(close(stage.get_zoom_level(), 2.0));
}

/// At 200%, scrolled into the middle page, so both axes have room to
/// travel and a zoom around a point near the middle holds it.
fn travelling() -> (Kernel, StageCapability) {
    let (kernel, stage) = placed(Default::default());
    stage.zoom_to(ZoomTarget::Level(2.0), None).expect("level");
    stage
        .scroll_to(StageScrollToOptions {
            top: Some(1800.0),
            left: Some(124.0),
            ..Default::default()
        })
        .expect("scroll");
    (kernel, stage)
}

#[test]
fn zoom_in_holds_the_point_it_is_given() {
    let (_k, stage) = travelling();
    let at = Point { x: 480.0, y: 380.0 };
    let world = stage.viewport_to_world(at);
    stage.zoom_in(Some(ZoomToOptions { around: Some(at) })).expect("in");
    let after = stage.viewport_to_world(at);
    assert!(close(after.x, world.x) && close(after.y, world.y));
}

fn sample(source: Option<String>, alt: bool) -> PointerSample {
    let mut sample = PointerSample::new(Phase::Down, Point { x: 300.0, y: 200.0 }, None);
    sample.modifiers = Modifiers { alt, ..Default::default() };
    sample.pointer_type = Some(PointerKind::Mouse);
    sample.source = source;
    sample
}

#[test]
fn the_zoom_handler_takes_its_own_lens_presses_under_the_zoom_box_tag() {
    let (kernel, stage) = placed(Default::default());
    let interaction: InteractionCapability =
        kernel.capability(&epdf_plugin_interaction::INTERACTION, Some("a")).expect("interaction");
    let handler = create_zoom_drag_handler(stage.clone(), interaction);
    assert_eq!(handler.id(), "stage-zoom-drag");
    assert_eq!(handler.priority(), 10.0);
    let zoom = Tool::new("zoom", "zoom-in", ["zoom-box"]).with_gap_cursor("zoom-in");
    assert!(handler.enabled_for(&zoom));
    assert!(!handler.enabled_for(&Tool::new("pointer", "default", [])));
    assert!(!handler.on_down(&sample(Some("elsewhere".into()), false)));
    assert_eq!(model(&kernel)["zoomDrag"], Value::Null);
    assert!(handler.on_down(&sample(Some(stage.get_lens_id()), false)));
    assert_ne!(model(&kernel)["zoomDrag"], Value::Null);
    handler.on_up(&sample(Some(stage.get_lens_id()), true));
    assert!(close(stage.get_zoom_level(), 0.75), "Alt turns the click into a step out");
}

#[test]
fn the_pan_fallback_leaves_a_tool_that_acts_over_the_gaps() {
    let (kernel, stage) = placed(Default::default());
    let interaction: InteractionCapability =
        kernel.capability(&epdf_plugin_interaction::INTERACTION, Some("a")).expect("interaction");
    let handler = create_scroll_handler(stage, interaction, ScrollHandlerOptions::default());
    assert!(handler.enabled_for(&Tool::new("pointer", "default", [])));
    assert!(handler.enabled_for(&Tool::new("pan", "grab", ["scroll"]).with_gap_cursor("grab")));
    assert!(
        !handler
            .enabled_for(&Tool::new("zoom", "zoom-in", ["zoom-box"]).with_gap_cursor("zoom-in"))
    );
}

#[test]
fn asking_again_for_a_fit_lands_after_a_step_left_it() {
    let (_k, stage) = placed(Default::default());
    stage.fit_width().expect("fit");
    stage.zoom_in(None).expect("in");
    assert!(matches!(stage.get_settings().zoom, ZoomSpec::Level { .. }));
    stage.fit_width().expect("fit again");
    assert_eq!(stage.get_settings().zoom, ZoomSpec::Mode { mode: ZoomMode::FitWidth });
}

// ── locks ──

fn paged_lock(by: &str) -> StageSettingsLock {
    StageSettingsLock {
        by: by.into(),
        reason_key: Some("x.reason".into()),
        settings: StageSettingsPatch { flow: Some(FlowMode::Paged), ..Default::default() },
    }
}

#[test]
fn a_lock_holds_a_setting_over_every_writer_until_it_is_released() {
    let (kernel, stage) = placed(Default::default());
    let release = stage.lock_settings(paged_lock("slides")).expect("lock");
    assert_eq!(stage.get_settings().flow, FlowMode::Paged);
    assert_eq!(
        stage.get_lock(SettingKey::Flow),
        Some(SettingLock { by: "slides".into(), reason_key: Some("x.reason".into()) })
    );
    assert_eq!(stage.get_lock(SettingKey::Layout), None);
    let state = model(&kernel);
    assert_eq!(state["locks"], json!({ "flow": { "by": "slides", "reasonKey": "x.reason" } }));
    stage
        .update_settings(StageSettingsPatch {
            flow: Some(FlowMode::Continuous),
            ..Default::default()
        })
        .expect("write under the lock");
    assert_eq!(stage.get_settings().flow, FlowMode::Paged, "the lock wins");
    assert_eq!(
        stage.get_view_state().expect("view").settings.flow,
        FlowMode::Continuous,
        "a saved view keeps what is under the lock"
    );
    // A restored view, a reset and a rule write underneath too.
    let mut view = StageViewPatch::from(stage.get_view_state().expect("view"));
    view.settings.flow = Some(FlowMode::Continuous);
    stage.apply_view_state(view);
    assert_eq!(stage.get_settings().flow, FlowMode::Paged, "a restored view stays under the lock");
    stage.reset_settings().expect("reset");
    assert_eq!(stage.get_settings().flow, FlowMode::Paged, "a reset stays under the lock");
    stage
        .set_responsive_rules(vec![ResponsiveRule {
            name: Some("always".into()),
            when: RuleWhen::Query(BoxQuery::default()),
            settings: Some(StageSettingsPatch {
                flow: Some(FlowMode::Continuous),
                ..Default::default()
            }),
        }])
        .expect("rules");
    assert_eq!(stage.get_settings().flow, FlowMode::Paged, "a rule stays under the lock");
    release.call();
    assert_eq!(stage.get_settings().flow, FlowMode::Continuous, "the user's choice is back");
    assert_eq!(stage.get_lock(SettingKey::Flow), None);
    release.call();
}

#[test]
fn equal_locks_stack_and_conflicting_or_zoom_locks_are_refused() {
    let (_k, stage) = placed(Default::default());
    let first = stage.lock_settings(paged_lock("a")).expect("lock");
    let second = stage.lock_settings(paged_lock("b")).expect("an equal lock stacks");
    let Err(conflict) = stage.lock_settings(StageSettingsLock {
        by: "c".into(),
        reason_key: None,
        settings: StageSettingsPatch { flow: Some(FlowMode::Continuous), ..Default::default() },
    }) else {
        panic!("a different value is refused");
    };
    assert_eq!(conflict.plugin_code(), Some("conflict"));
    assert_eq!(conflict.message, "\"flow\" is locked by a at another value");
    let Err(zoom) = stage.lock_settings(StageSettingsLock {
        by: "c".into(),
        reason_key: None,
        settings: StageSettingsPatch::zoom(ZoomSpec::Level { level: 2.0 }),
    }) else {
        panic!("the zoom is not lockable");
    };
    assert_eq!(zoom.plugin_code(), Some("invalid-input"));
    first.call();
    assert_eq!(stage.get_settings().flow, FlowMode::Paged, "the second lock still holds");
    assert_eq!(stage.get_lock(SettingKey::Flow).map(|l| l.by), Some("b".into()));
    second.call();
    assert_eq!(stage.get_settings().flow, FlowMode::Continuous);
}
