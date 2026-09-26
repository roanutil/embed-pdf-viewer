//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the pure suites call by their TypeScript names.
//!
//! It mirrors the shims in `tooling/plugin-conformance/src/suites/`:
//!
//! - `stage-plugin.ts`: a config with `frames: true` gets [`FrameScheduler`]
//!   (16 ms timers on the runtime clock, handles from 1); `lens: name`
//!   registers the plugin under id `name` with a token of that name; a
//!   responsive rule's `when` may be a `$fn` spec, a scripted predicate
//!   that logs the box; and the `stage-scroll` factory registers the scroll
//!   handler with the hub, scoped to the lens.
//! - `stage-pure.ts`: `resolveResponsive` rules may carry `when:
//!   {"$returns": v}`, the predicate `() => v`; `stageReducer` reports
//!   `{state, same}`; `constants` prints the registry and motion constants.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_geometry::{PageRotation, Point, Rect, Size};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, TimerId};
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_interaction::harness::{answer_truthy, truthy};
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions};
use epdf_stage_core::{Camera, ZoomSpec};
use plugin_conformance::js::{arg, from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Map, Value, json};

use crate::destination::{PageLayout, PdfDestination, destination_to_reveal};
use crate::model::{initial_stage_state, stage_reducer};
use crate::motion::*;
use crate::responsive::{box_of, eq_setting, matches_query, merge_settings, resolve_responsive};
use crate::settings::{
    SETTING_KEYS, default_responsive, default_settings, settings_effect, settings_equal,
};
use crate::*;

// ── the frame seam ────────────────────────────────────────────────────────────

/// Frames as 16 ms timers on the runtime clock, stamped with `rt::now()`:
/// the generator's `frames()` scheduler over `@sinonjs/fake-timers`.
#[derive(Default)]
pub struct FrameScheduler {
    next: Cell<u64>,
    timers: Rc<RefCell<HashMap<u64, TimerId>>>,
}

impl Scheduler for FrameScheduler {
    fn raf(&self, callback: Box<dyn FnOnce(f64)>) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        let timers = self.timers.clone();
        let timer = rt::set_timeout(16.0, move || {
            timers.borrow_mut().remove(&id);
            callback(rt::now());
        });
        self.timers.borrow_mut().insert(id, timer);
        id
    }

    fn caf(&self, handle: u64) {
        let timer = self.timers.borrow_mut().remove(&handle);
        if let Some(timer) = timer {
            rt::clear_timer(timer);
        }
    }
}

// ── decoding ──────────────────────────────────────────────────────────────────

fn present(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

/// An optional argument: absent, `undefined` and `null` are `None`.
fn opt<T: serde::de::DeserializeOwned>(args: &[Value], i: usize) -> Result<Option<T>, Error> {
    match present(args, i) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => from_value(v).map(Some),
    }
}

fn rule_from_json(value: &Value, h: Option<&Harness>) -> Result<ResponsiveRule, Error> {
    let name = value.get("name").filter(|v| !is_undefined(v)).and_then(Value::as_str);
    let when = value.get("when").cloned().unwrap_or(Value::Null);
    let when = if when.get("$fn").is_some() {
        let h = h.ok_or_else(|| Error::new("[harness] a $fn rule needs a scenario"))?;
        let callback = h.callback(&when);
        RuleWhen::Predicate(Rc::new(move |bx: &StageBox| {
            answer_truthy(callback.call(vec![to_value(bx)]))
        }))
    } else if let Some(returns) = when.get("$returns") {
        let answer = truthy(returns);
        RuleWhen::Predicate(Rc::new(move |_| answer))
    } else {
        RuleWhen::Query(from_value(&when)?)
    };
    let settings = match value.get("settings") {
        None => None,
        Some(v) if is_undefined(v) => None,
        Some(v) => Some(from_value(v)?),
    };
    Ok(ResponsiveRule { name: name.map(str::to_string), when, settings })
}

fn rules_from_json(value: &Value, h: Option<&Harness>) -> Result<Vec<ResponsiveRule>, Error> {
    value.as_array().into_iter().flatten().map(|rule| rule_from_json(rule, h)).collect()
}

fn rule_json(rule: &ResponsiveRule) -> Value {
    let mut out = Map::new();
    if let Some(name) = &rule.name {
        out.insert("name".into(), json!(name));
    }
    out.insert(
        "when".into(),
        match &rule.when {
            RuleWhen::Query(q) => to_value(q),
            RuleWhen::Predicate(_) => json!({ "$function": true }),
        },
    );
    if let Some(settings) = &rule.settings {
        out.insert("settings".into(), to_value(settings));
    }
    Value::Object(out)
}

/// The generator's `StagePluginOptions` from a vector's config.
fn stage_options(config: &Value, h: &Harness) -> Result<StagePluginOptions, Error> {
    let config = if is_undefined(config) || config.is_null() { json!({}) } else { config.clone() };
    let settings: PartialSettings = from_value(&config)?;
    let lens = config.get("lens").and_then(Value::as_str);
    let scheduler: Option<Rc<dyn Scheduler>> = (config.get("frames").and_then(Value::as_bool)
        == Some(true))
    .then(|| Rc::new(FrameScheduler::default()) as Rc<dyn Scheduler>);
    let responsive = match config.get("responsive") {
        None => None,
        Some(v) if is_undefined(v) => None,
        Some(v) => Some(rules_from_json(v, Some(h))?),
    };
    Ok(StagePluginOptions {
        id: lens.map(str::to_string),
        token: lens.map(|name| lens_token(h, name)),
        config: StageConfig { settings, scheduler, responsive },
    })
}

fn lens_token(h: &Harness, name: &str) -> Token<StageCapability> {
    Token::from_any(h.token(name, None))
}

// ── printing ──────────────────────────────────────────────────────────────────

/// A laid-out page as the TypeScript prints it: the transform's six
/// methods are functions, registered as refs in member order.
fn visible_page_json(h: &Harness, page: &VisiblePage) -> Value {
    let mut out = to_value(page);
    let transform = Rc::new(page.transform.clone());
    let point_fn = |f: fn(&epdf_geometry::PageTransform, Point) -> Point| {
        let t = transform.clone();
        h.register_fn(Rc::new(move |args: Vec<Value>| {
            Outcome::from_result(arg::<Point>(&args, 0).map(|p| to_value(&f(&t, p))))
        }))
    };
    let rect_fn = |f: fn(&epdf_geometry::PageTransform, Rect) -> Rect| {
        let t = transform.clone();
        h.register_fn(Rc::new(move |args: Vec<Value>| {
            Outcome::from_result(arg::<Rect>(&args, 0).map(|r| to_value(&f(&t, r))))
        }))
    };
    let members = [
        ("toPixels", point_fn(epdf_geometry::PageTransform::to_pixels)),
        ("fromPixels", point_fn(epdf_geometry::PageTransform::from_pixels)),
        ("contentToView", point_fn(epdf_geometry::PageTransform::content_to_view)),
        ("contentToViewRect", rect_fn(epdf_geometry::PageTransform::content_to_view_rect)),
        ("viewToContent", point_fn(epdf_geometry::PageTransform::view_to_content)),
        ("viewToContentRect", rect_fn(epdf_geometry::PageTransform::view_to_content_rect)),
    ];
    if let Some(t) = out.get_mut("transform").and_then(Value::as_object_mut) {
        for (name, value) in members {
            t.insert(name.into(), value);
        }
    }
    out
}

fn option_json<T: serde::Serialize>(value: Option<T>) -> Value {
    value.map_or(Value::Null, |v| to_value(&v))
}

fn done(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

/// The capability's member order: `composeApi` over the areas' slices, in
/// the order `controller.ts` lists them, then the six hooks.
const MEMBERS: &[&str] = &[
    // read/pages
    "getCamera",
    "getViewportSize",
    "getScrollMetrics",
    "listVisiblePages",
    "isPageVisible",
    "getCurrentPageIndex",
    "getCurrentPage",
    "listCurrentItemPages",
    "getPageFrame",
    "getPageAt",
    "viewportToPage",
    "pageToWorld",
    "pageRectToViewport",
    "worldToViewport",
    "viewportToWorld",
    "pageToViewport",
    "getViewRotation",
    "getZoomLevel",
    "getZoomMode",
    "getViewpoint",
    // camera/animation
    "fling",
    "isMoving",
    "stopMotion",
    // camera/gestures
    "setCamera",
    "panBy",
    "scrollTo",
    "scrollBy",
    "zoomAround",
    "beginGesture",
    "endGesture",
    // navigation/arrive
    "goToPageIndex",
    "goToPage",
    "goToFirstPage",
    "goToLastPage",
    "canGoNext",
    "canGoPrevious",
    "nextPage",
    "previousPage",
    "resetView",
    // navigation/reveal
    "reveal",
    "revealRect",
    "revealIndex",
    // settings/settings
    "getSettings",
    "resetSettings",
    "updateSettings",
    "setResponsiveRules",
    "matchesRule",
    "listActiveRules",
    "setFlow",
    "setLayout",
    "setSpread",
    "setSizing",
    "applyViewState",
    // navigation/zoom
    "doubleTapZoom",
    "zoomIn",
    "zoomOut",
    "zoomTo",
    "zoomBy",
    "fitWidth",
    "fitPage",
    "fitAll",
    "fitAutomatic",
    "setViewRotation",
    "rotateViewBy",
    // view/lifecycle
    "setViewportSize",
    "setDevicePixelRatio",
    "refit",
    "getViewState",
    "getLensId",
    "provideInitialView",
    "placeInitial",
    // the hooks
    "onPageChanged",
    "onZoomChanged",
    "onCameraChanged",
    "onMotionEnded",
    "onSettingsChanged",
    "onViewportChanged",
];

fn index_arg(args: &[Value], i: usize) -> Result<isize, Error> {
    let n: f64 = arg(args, i)?;
    Ok(n as isize)
}

/// A `PageRef` argument. Anything else names no page: every page verb
/// then answers as it does for a page the document lacks.
fn page_arg(args: &[Value], i: usize) -> PageRef {
    present(args, i).and_then(PageRef::from_json).unwrap_or(PageRef::new(u32::MAX))
}

/// A registry row as JavaScript reads it: `undefined` members absent.
fn page_info_json(page: Option<PageInfo>) -> Value {
    page.map_or(Value::Null, |p| without_undefined(&p))
}

/// Subscribe a scripted listener to one hook; the outcome is the
/// unsubscribe, as a ref.
fn hook<E: serde::Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| {
        let event = without_undefined(&to_value(event));
        match callback.call(vec![event]) {
            Answer::Throws(error) => Err(error),
            _ => Ok(()),
        }
    }));
    unsubscribe_ref(h, off)
}

fn unsubscribe_ref(h: &Harness, off: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn call_stage(
    cap: &StageCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let r = Outcome::Returned;
    let unit = Outcome::undefined;
    Ok(match method {
        // read/pages
        "getCamera" => r(to_value(&cap.get_camera())),
        "getViewportSize" => r(to_value(&cap.get_viewport_size())),
        "getScrollMetrics" => r(to_value(&cap.get_scroll_metrics())),
        "listVisiblePages" => r(Value::Array(
            cap.list_visible_pages().iter().map(|p| visible_page_json(h, p)).collect(),
        )),
        "isPageVisible" => r(json!(cap.is_page_visible(page_arg(args, 0)))),
        "getCurrentPageIndex" => r(json!(cap.get_current_page_index())),
        "getCurrentPage" => r(page_info_json(cap.get_current_page())),
        "listCurrentItemPages" => r(Value::Array(
            cap.list_current_item_pages().into_iter().map(|p| page_info_json(Some(p))).collect(),
        )),
        "getPageFrame" => r(cap
            .get_page_frame(page_arg(args, 0))
            .map_or(Value::Null, |p| visible_page_json(h, &p))),
        "getPageAt" => r(option_json(cap.get_page_at(arg(args, 0)?))),
        "viewportToPage" => r(option_json(cap.viewport_to_page(page_arg(args, 0), arg(args, 1)?))),
        "pageToWorld" => r(option_json(cap.page_to_world(page_arg(args, 0), arg(args, 1)?))),
        "pageRectToViewport" => {
            r(option_json(cap.page_rect_to_viewport(page_arg(args, 0), arg(args, 1)?)))
        }
        "worldToViewport" => r(to_value(&cap.world_to_viewport(arg(args, 0)?))),
        "viewportToWorld" => r(to_value(&cap.viewport_to_world(arg(args, 0)?))),
        "pageToViewport" => r(option_json(cap.page_to_viewport(page_arg(args, 0), arg(args, 1)?))),
        "getViewRotation" => r(to_value(&cap.get_view_rotation())),
        "getZoomLevel" => r(to_value(&cap.get_zoom_level())),
        "getZoomMode" => r(to_value(&cap.get_zoom_mode())),
        "getViewpoint" => r(to_value(&cap.get_viewpoint())),
        // camera/animation
        "fling" => done(cap.fling(arg(args, 0)?, arg(args, 1)?)),
        "isMoving" => r(json!(cap.is_moving())),
        "stopMotion" => {
            cap.stop_motion();
            unit()
        }
        // camera/gestures
        "setCamera" => done(cap.set_camera(arg::<Camera>(args, 0)?)),
        "panBy" => done(cap.pan_by(arg(args, 0)?, arg(args, 1)?)),
        "scrollTo" => {
            cap.scroll_to(arg(args, 0)?);
            unit()
        }
        "scrollBy" => {
            cap.scroll_by(arg(args, 0)?);
            unit()
        }
        "zoomAround" => done(cap.zoom_around(arg(args, 0)?, arg(args, 1)?)),
        "beginGesture" => {
            cap.begin_gesture(opt(args, 0)?);
            unit()
        }
        "endGesture" => done(cap.end_gesture()),
        // navigation/arrive
        "goToPageIndex" => {
            cap.go_to_page_index(index_arg(args, 0)?, opt(args, 1)?);
            unit()
        }
        "goToPage" => {
            cap.go_to_page(page_arg(args, 0), opt(args, 1)?);
            unit()
        }
        "goToFirstPage" => {
            cap.go_to_first_page(opt(args, 0)?);
            unit()
        }
        "goToLastPage" => {
            cap.go_to_last_page(opt(args, 0)?);
            unit()
        }
        "canGoNext" => r(json!(cap.can_go_next())),
        "canGoPrevious" => r(json!(cap.can_go_previous())),
        "nextPage" => {
            cap.next_page(opt(args, 0)?);
            unit()
        }
        "previousPage" => {
            cap.previous_page(opt(args, 0)?);
            unit()
        }
        "resetView" => {
            cap.reset_view();
            unit()
        }
        // navigation/reveal
        "reveal" => {
            cap.reveal(page_arg(args, 0), opt(args, 1)?);
            unit()
        }
        "revealRect" => {
            cap.reveal_rect(page_arg(args, 0), arg(args, 1)?, opt(args, 2)?);
            unit()
        }
        "revealIndex" => {
            cap.reveal_index(index_arg(args, 0)?, opt(args, 1)?);
            unit()
        }
        // settings/settings
        "getSettings" => r(to_value(&cap.get_settings())),
        "resetSettings" => done(cap.reset_settings()),
        "updateSettings" => done(cap.update_settings(arg(args, 0)?)),
        "setResponsiveRules" => done(cap.set_responsive_rules(rules_from_json(
            present(args, 0).unwrap_or(&Value::Null),
            Some(h),
        )?)),
        "matchesRule" => r(json!(cap.matches_rule(&arg::<String>(args, 0)?))),
        "listActiveRules" => r(json!(cap.list_active_rules())),
        "setFlow" => done(cap.set_flow(arg(args, 0)?)),
        "setLayout" => done(cap.set_layout(arg(args, 0)?)),
        "setSpread" => done(cap.set_spread(arg(args, 0)?)),
        "setSizing" => done(cap.set_sizing(arg(args, 0)?)),
        "applyViewState" => {
            cap.apply_view_state(arg(args, 0)?);
            unit()
        }
        // navigation/zoom
        "doubleTapZoom" => {
            cap.double_tap_zoom(arg(args, 0)?);
            unit()
        }
        "zoomIn" => done(cap.zoom_in()),
        "zoomOut" => done(cap.zoom_out()),
        "zoomTo" => done(cap.zoom_to(arg::<ZoomTarget>(args, 0)?, opt(args, 1)?)),
        "zoomBy" => done(cap.zoom_by(arg(args, 0)?, opt(args, 1)?)),
        "fitWidth" => done(cap.fit_width()),
        "fitPage" => done(cap.fit_page()),
        "fitAll" => done(cap.fit_all()),
        "fitAutomatic" => done(cap.fit_automatic()),
        "setViewRotation" => done(cap.set_view_rotation(arg::<PageRotation>(args, 0)?)),
        "rotateViewBy" => done(cap.rotate_view_by(arg(args, 0)?)),
        // view/lifecycle
        "setViewportSize" => {
            cap.set_viewport_size(arg::<Size>(args, 0)?);
            unit()
        }
        "setDevicePixelRatio" => {
            cap.set_device_pixel_ratio(arg(args, 0)?);
            unit()
        }
        "refit" => {
            cap.refit();
            unit()
        }
        "getViewState" => r(to_value(&cap.get_view_state())),
        "getLensId" => r(json!(cap.get_lens_id())),
        "provideInitialView" => {
            let priority: f64 = arg(args, 0)?;
            let callback = h.callback(present(args, 1).unwrap_or(&Value::Null));
            let off = cap.provide_initial_view(priority, move || match callback.call(Vec::new()) {
                Answer::Returns(v) if truthy(&v) => from_value(&v).ok(),
                _ => None,
            });
            unsubscribe_ref(h, off)
        }
        "placeInitial" => {
            cap.place_initial();
            unit()
        }
        // the hooks
        "onPageChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_page_changed(f))
        }
        "onZoomChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_zoom_changed(f))
        }
        "onCameraChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_camera_changed(f))
        }
        "onMotionEnded" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_motion_ended(f))
        }
        "onSettingsChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_settings_changed(f))
        }
        "onViewportChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_viewport_changed(f))
        }
        other => not_a_function(other),
    })
}

pub struct Stage;

impl HarnessPlugin for Stage {
    fn factories(&self) -> Vec<&'static str> {
        vec!["stage", "stage-scroll"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "stage-scroll" {
            let lens = config.get("lens").and_then(Value::as_str);
            let token = lens.map_or_else(|| STAGE.clone(), |name| lens_token(h, name));
            let pan_fallback = config.get("panFallback").and_then(Value::as_bool);
            let requires = token.clone();
            return Ok(define::<Value, Value>("stage-scroll")
                .document_scoped()
                .requires(requires.any())
                .requires(INTERACTION.any())
                .init(move |ctx| {
                    let stage = ctx.get(&token)?;
                    let hub = ctx.get(&INTERACTION)?;
                    let handler = create_scroll_handler(
                        stage.clone(),
                        hub.clone(),
                        ScrollHandlerOptions { pan_fallback },
                    );
                    let off = hub.register_handler(
                        handler,
                        RegisterHandlerOptions { source: Some(stage.get_lens_id()) },
                    );
                    ctx.cleanup_fn(move || off.call());
                    Ok(None)
                })
                .build());
        }
        Ok(stage_plugin(stage_options(config, h)?))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![STAGE.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<StageCapability>()?;
        Some(call_stage(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<StageCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── function suites ───────────────────────────────────────────────────────────

fn value(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or_else(undefined)
}

/// `eqSetting` over whichever setting type both values decode as.
fn eq_setting_json(a: &Value, b: &Value) -> bool {
    fn both<T: serde::de::DeserializeOwned + PartialEq>(a: &Value, b: &Value) -> Option<bool> {
        let x: T = from_value(a).ok()?;
        let y: T = from_value(b).ok()?;
        Some(eq_setting(&x, &y))
    }
    let number = |v: &Value| v.is_number() || v.get("$number").is_some();
    if number(a) && number(b) {
        return both::<f64>(a, b).unwrap_or(false);
    }
    if a.is_string() && b.is_string() {
        return a == b;
    }
    if a.is_boolean() && b.is_boolean() {
        return a == b;
    }
    if !a.is_object() || !b.is_object() {
        return false;
    }
    both::<epdf_stage_core::AlignmentValue>(a, b)
        .or_else(|| both::<ArrivalAlignment>(a, b))
        .or_else(|| both::<epdf_geometry::PageFrame>(a, b))
        .or_else(|| both::<ZoomSpec>(a, b))
        .or_else(|| both::<Gap>(a, b))
        .unwrap_or(false)
}

fn constants() -> Value {
    let effects: Map<String, Value> = SETTING_KEYS
        .iter()
        .map(|&k| {
            let key = serde_json::to_value(k).unwrap();
            (
                key.as_str().unwrap_or_default().to_string(),
                serde_json::to_value(settings_effect(k)).unwrap(),
            )
        })
        .collect();
    json!({
        "DEFAULT_SETTINGS": to_value(&default_settings()),
        "DEFAULT_RESPONSIVE": default_responsive().iter().map(rule_json).collect::<Vec<_>>(),
        "SETTINGS_EFFECT": effects,
        "SETTING_KEYS": SETTING_KEYS.iter().map(|k| serde_json::to_value(k).unwrap()).collect::<Vec<_>>(),
        "RUBBER": RUBBER,
        "FLING_DECAY": FLING_DECAY,
        "FLING_STOP": FLING_STOP,
        "SPRING_OMEGA": SPRING_OMEGA,
    })
}

/// The TypeScript names of the pure functions.
pub fn functions() -> FunctionTable {
    vec![
        ("destinationToReveal", |a| {
            let dest: PdfDestination = arg(a, 0)?;
            let layout: PageLayout = arg(a, 1)?;
            let bbox: Option<Rect> = opt(a, 2)?;
            destination_to_reveal(&dest, &layout, bbox)
                .map(|r| to_value(&r))
                .map_err(|e| Error::new(e.to_string()))
        }),
        ("settingsEqual", |a| {
            let x: StageSettings = arg(a, 0)?;
            let y: StageSettings = arg(a, 1)?;
            Ok(json!(settings_equal(&x, &y)))
        }),
        ("constants", |_| Ok(constants())),
        ("rubberOut", |a| Ok(to_value(&rubber_out(arg(a, 0)?, arg(a, 1)?)))),
        ("rubberIn", |a| Ok(to_value(&rubber_in(arg(a, 0)?, arg(a, 1)?)))),
        ("glideStep", |a| Ok(to_value(&glide_step(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?)))),
        ("springStep", |a| {
            Ok(to_value(&spring_step(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?)))
        }),
        ("easeOutCubic", |a| Ok(to_value(&ease_out_cubic(arg(a, 0)?)))),
        ("zoomLerp", |a| Ok(to_value(&zoom_lerp(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?)))),
        ("boxOf", |a| Ok(to_value(&box_of(arg(a, 0)?)))),
        ("matchesQuery", |a| {
            let q: BoxQuery = arg(a, 0)?;
            let bx: StageBox = arg(a, 1)?;
            Ok(json!(matches_query(&q, &bx)))
        }),
        ("mergeSettings", |a| {
            let into: StageSettings = arg(a, 0)?;
            let patch: PartialSettings = arg(a, 1)?;
            Ok(to_value(&merge_settings(&into, &patch)))
        }),
        ("eqSetting", |a| Ok(json!(eq_setting_json(&value(a, 0), &value(a, 1))))),
        ("resolveResponsive", |a| {
            let base: StageSettings = arg(a, 0)?;
            let rules = rules_from_json(&value(a, 1), None)?;
            let bx: StageBox = arg(a, 2)?;
            let resolved = resolve_responsive(&base, &rules, &bx);
            Ok(json!({ "effective": to_value(&resolved.effective), "active": resolved.active }))
        }),
        ("initialStageState", |a| {
            let overrides: PartialSettings = arg(a, 0)?;
            Ok(to_value(&initial_stage_state(&overrides)))
        }),
        ("stageReducer", |a| {
            let state: StageState = arg(a, 0)?;
            let action: StageAction = arg(a, 1)?;
            Ok(match stage_reducer(&state, &action) {
                Some(next) => json!({ "state": to_value(&next), "same": false }),
                None => json!({ "state": to_value(&state), "same": true }),
            })
        }),
    ]
}
