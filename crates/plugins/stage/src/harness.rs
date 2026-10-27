//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the pure suites call by their TypeScript names.
//!
//! It mirrors the shims in `tooling/plugin-conformance/src/suites/`:
//!
//! - `stage-plugin.ts`: `lens: name` registers the plugin under id `name`
//!   with a token of that name; a responsive rule's `when` may be a `$fn`
//!   spec, a scripted predicate that logs the box; and the `stage-scroll`
//!   factory registers the scroll handler with the hub at connect, scoped
//!   to the lens. A case's `clock: {frames: true}` gives the kernel the
//!   scenario clock's 16 ms frames (`plugin-conformance`); the plugin reads
//!   them through `ctx.clock`.
//! - `stage-pure.ts`: `resolveResponsive` rules may carry `when:
//!   {"$returns": v}`, the predicate `() => v`; the named transitions report
//!   `{state, same}`; `constants` prints the registry and motion constants.

use std::any::Any;
use std::rc::Rc;

use epdf_geometry::{PageRotation, Point, Rect, Size};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_interaction::harness::{answer_truthy, truthy};
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions};
use epdf_stage_core::ZoomSpec;
use plugin_conformance::js::{arg, from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Map, Value, json};

use crate::model::{self, CameraObject, CameraOrder, StageState};
use crate::motion::*;
use crate::responsive::{box_of, eq_setting, matches_query, merge_settings, resolve_responsive};
use crate::settings::{
    SETTING_KEYS, default_responsive, default_settings, settings_effect, settings_equal,
};
use crate::*;

// ── decoding ──

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

/// The generator's `StagePluginOptions` from a vector's config: the
/// settings, `lens`, and the rules. `frames` is the case clock's.
fn stage_options(config: &Value, h: &Harness) -> Result<StagePluginOptions, Error> {
    let config = if is_undefined(config) || config.is_null() { json!({}) } else { config.clone() };
    let settings: StageSettingsPatch = from_value(&config)?;
    let lens = config.get("lens").and_then(Value::as_str);
    let responsive = match config.get("responsive") {
        None => None,
        Some(v) if is_undefined(v) => None,
        Some(v) => Some(rules_from_json(v, Some(h))?),
    };
    Ok(StagePluginOptions {
        id: lens.map(str::to_string),
        token: lens.map(|name| lens_token(h, name)),
        config: StageConfig { settings, responsive },
    })
}

fn lens_token(h: &Harness, name: &str) -> Token<StageCapability> {
    Token::from_any(h.token(name, None))
}

/// A page argument, `PageRef | number`. An object that is not a ref names
/// no page: every page verb then answers as for a page the document lacks.
fn page_arg(args: &[Value], i: usize) -> PageTarget {
    let no_page = PageTarget::Ref(PageRef::new(u32::MAX));
    match present(args, i) {
        Some(value) if value.is_number() || value.get("$number").is_some() => {
            from_value::<f64>(value).map_or(no_page, PageTarget::Index)
        }
        Some(value) => PageRef::from_json(value).map_or(no_page, PageTarget::Ref),
        None => no_page,
    }
}

// ── printing ──

/// A laid-out page as the TypeScript prints it: the transform's six
/// methods are functions, registered as refs in member order. A live
/// harness prints the transform's data alone: its host rebuilds the six
/// methods from it (`packages/core-wasm/src/rehydrate.ts`), so refs made
/// here would only pile up, six per page on every read.
fn visible_page_json(h: &Harness, page: &VisiblePage) -> Value {
    let mut out = to_value(page);
    if h.is_live() {
        return out;
    }
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
        ("pageToView", point_fn(epdf_geometry::PageTransform::page_to_view)),
        ("pageToViewRect", rect_fn(epdf_geometry::PageTransform::page_to_view_rect)),
        ("viewToPage", point_fn(epdf_geometry::PageTransform::view_to_page)),
        ("viewToPageRect", rect_fn(epdf_geometry::PageTransform::view_to_page_rect)),
    ];
    if let Some(t) = out.get_mut("transform").and_then(Value::as_object_mut) {
        for (name, value) in members {
            t.insert(name.into(), value);
        }
        // `cssMatrix` follows the six methods in `pageTransform`'s literal
        // (`packages/core/geometry/src/index.ts`).
        if let Some(css) = t.shift_remove("cssMatrix") {
            t.insert("cssMatrix".into(), css);
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

fn returned<T: serde::Serialize>(result: Result<T, Error>) -> Outcome {
    match result {
        Ok(value) => Outcome::Returned(to_value(&value)),
        Err(error) => Outcome::Threw(error),
    }
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

/// The capability's member order: `composeApi` over the areas' slices, in
/// the order `controller.ts` lists them, then the six events.
const MEMBERS: &[&str] = &[
    // read/pages
    "getCamera",
    "getViewportSize",
    "getScrollMetrics",
    "listVisiblePages",
    "isPageVisible",
    "getCurrentPageIndex",
    "getCurrentPage",
    "getPageCount",
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
    "getCurrentDestination",
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
    "goToDestination",
    // settings/settings
    "getSettings",
    "resetSettings",
    "updateSettings",
    "setResponsiveRules",
    "matchesRule",
    "listActiveRules",
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
    // the events
    "onPageChanged",
    "onZoomChanged",
    "onCameraChanged",
    "onMotionEnded",
    "onSettingsChanged",
    "onViewportChanged",
];

/// Subscribe a scripted listener to one event; the outcome is the
/// unsubscribe, as a ref.
fn hook<E: serde::Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    unsubscribe_ref(h, off)
}

fn unsubscribe_ref(h: &Harness, off: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

/// The camera as the state holds it: its object literal's member order.
fn camera_json(cap: &StageCapability) -> Value {
    let state = cap.st();
    to_value(&state.camera_order().view(&state.camera))
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
        "getCamera" => r(camera_json(cap)),
        "getViewportSize" => r(to_value(&cap.get_viewport_size())),
        "getScrollMetrics" => r(to_value(&cap.get_scroll_metrics())),
        "listVisiblePages" => r(Value::Array(
            cap.list_visible_pages().iter().map(|p| visible_page_json(h, p)).collect(),
        )),
        "isPageVisible" => r(json!(cap.is_page_visible(page_arg(args, 0)))),
        "getCurrentPageIndex" => r(json!(cap.get_current_page_index())),
        "getCurrentPage" => r(option_json(cap.get_current_page())),
        "getPageCount" => r(json!(cap.get_page_count())),
        "listCurrentItemPages" => r(Value::Array(
            cap.list_current_item_pages()
                .iter()
                .map(crate::services::scene::without_undefined)
                .collect(),
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
        "getViewpoint" => returned(cap.get_viewpoint()),
        "getCurrentDestination" => r(option_json(cap.get_current_destination())),
        // camera/animation
        "fling" => done(cap.fling(arg(args, 0)?, arg(args, 1)?)),
        "isMoving" => r(json!(cap.is_moving())),
        "stopMotion" => {
            cap.stop_motion();
            unit()
        }
        // camera/gestures
        "setCamera" => {
            let camera = arg(args, 0)?;
            let order = CameraOrder::of_json(present(args, 0).unwrap_or(&Value::Null));
            done(cap.set_camera_object(CameraObject::new(camera, order)))
        }
        "panBy" => done(cap.pan_by(arg(args, 0)?, arg(args, 1)?)),
        "scrollTo" => done(cap.scroll_to(arg(args, 0)?)),
        "scrollBy" => done(cap.scroll_by(arg(args, 0)?)),
        "zoomAround" => done(cap.zoom_around(arg(args, 0)?, arg(args, 1)?)),
        "beginGesture" => {
            cap.begin_gesture(opt(args, 0)?);
            unit()
        }
        "endGesture" => done(cap.end_gesture()),
        // navigation/arrive
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
        "goToDestination" => {
            let destination: PageDestination = arg(args, 0)?;
            cap.go_to_destination(&destination, opt(args, 1)?);
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
        "applyViewState" => {
            cap.apply_view_state(arg::<StageViewPatch>(args, 0)?);
            unit()
        }
        // navigation/zoom
        "doubleTapZoom" => done(cap.double_tap_zoom(arg(args, 0)?)),
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
        "setViewportSize" => done(cap.set_viewport_size(arg::<Size>(args, 0)?)),
        "setDevicePixelRatio" => {
            cap.set_device_pixel_ratio(arg(args, 0)?);
            unit()
        }
        "refit" => done(cap.refit()),
        "getViewState" => returned(cap.get_view_state()),
        "getLensId" => r(json!(cap.get_lens_id())),
        "provideInitialView" => {
            let priority: f64 = arg(args, 0)?;
            let callback = h.callback(present(args, 1).unwrap_or(&Value::Null));
            let off = cap.provide_initial_view(priority, move || match callback.call(Vec::new()) {
                Answer::Returns(v) if truthy(&v) => from_value::<StageViewPatch>(&v).ok(),
                _ => None,
            });
            unsubscribe_ref(h, off)
        }
        "placeInitial" => {
            cap.place_initial();
            unit()
        }
        // the events
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

/// The `stage-scroll` factory's api: nothing.
#[derive(Clone)]
struct ScrollBinding;

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
            let binding: Token<ScrollBinding> = Token::new("stage-scroll");
            return Ok(define::<Value, Value>("stage-scroll")
                .document_scoped()
                .requires(requires.any())
                .requires(INTERACTION.any())
                .create(&binding, move |cx| {
                    let token = token.clone();
                    let connect = move || {
                        let stage = cx.get(&token)?;
                        let hub = cx.get(&INTERACTION)?;
                        let handler = create_scroll_handler(
                            stage.clone(),
                            hub.clone(),
                            ScrollHandlerOptions { pan_fallback },
                        );
                        let off = hub.register_handler(
                            handler,
                            RegisterHandlerOptions { source: Some(stage.get_lens_id()) },
                        );
                        cx.cleanup_fn(move || off.call());
                        Ok(())
                    };
                    Ok((ScrollBinding, Some(Rc::new(connect) as epdf_kernel::plugin::ConnectFn)))
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
        if capability.downcast_ref::<ScrollBinding>().is_some() {
            return Some(Vec::new());
        }
        capability.downcast_ref::<StageCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── function suites ──

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

fn state_arg(args: &[Value], i: usize) -> Result<Rc<StageState>, Error> {
    let value = present(args, i).cloned().unwrap_or(Value::Null);
    let json: model::StateJson = from_value(&value)?;
    Ok(Rc::new(StageState::from_decoded(json, CameraOrder::of_json(&value["camera"]))))
}

/// `{state, same}`, `same` being `next === state`.
fn observed(state: &Rc<StageState>, next: Rc<StageState>) -> Value {
    json!({ "state": to_value(&*next), "same": Rc::ptr_eq(state, &next) })
}

/// `revealOfDestination`'s object literal orders: `{rect, anchor, zoom}`
/// for `xyz`, `{rect, zoom, anchor}` for the rest.
fn reveal_json(destination: &PageDestination, options: &RevealOptions) -> Value {
    let mut out = Map::new();
    let order: &[&str] = if matches!(destination, PageDestination::Xyz { .. }) {
        &["rect", "anchor", "zoom"]
    } else {
        &["rect", "zoom", "anchor"]
    };
    let full = to_value(options);
    for key in order {
        if let Some(value) = full.get(*key) {
            out.insert((*key).into(), value.clone());
        }
    }
    Value::Object(out)
}

/// The TypeScript names of the pure functions.
pub fn functions() -> FunctionTable {
    vec![
        ("revealOfDestination", |a| {
            let destination: PageDestination = arg(a, 0)?;
            let size: Size = arg(a, 1)?;
            Ok(reveal_json(&destination, &reveal_of_destination(&destination, size)))
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
            let patch: StageSettingsPatch = arg(a, 1)?;
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
            let overrides: StageSettingsPatch = arg(a, 0)?;
            Ok(to_value(&initial_stage_state(&overrides)))
        }),
        ("setCamera", |a| {
            let state = state_arg(a, 0)?;
            let camera = CameraObject::new(
                arg(a, 1)?,
                CameraOrder::of_json(present(a, 1).unwrap_or(&Value::Null)),
            );
            Ok(observed(&state, model::set_camera(&state, camera)))
        }),
        ("setCameraResting", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_camera_resting(&state, arg(a, 1)?)))
        }),
        ("markPlaced", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::mark_placed(&state)))
        }),
        ("setViewport", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_viewport(&state, arg(a, 1)?)))
        }),
        ("setDpr", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_dpr(&state, arg(a, 1)?)))
        }),
        ("setCursor", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_cursor(&state, arg(a, 1)?)))
        }),
        ("setMotionCause", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_motion_cause(&state, arg(a, 1)?)))
        }),
        ("patchSettings", |a| {
            let state = state_arg(a, 0)?;
            let patch: StageSettingsPatch = arg(a, 1)?;
            Ok(observed(&state, model::patch_settings(&state, &patch)))
        }),
        ("setActiveRules", |a| {
            let state = state_arg(a, 0)?;
            Ok(observed(&state, model::set_active_rules(&state, arg(a, 1)?)))
        }),
    ]
}
