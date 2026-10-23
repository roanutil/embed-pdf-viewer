//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector, calls its capability and view handles by name, and
//! the function table the pure suites call by their TypeScript names.
//!
//! It mirrors the shims in `tooling/plugin-conformance/src/suites/render.ts`:
//! a `renderSource`, `renderPage` or `renderThumbnail` option `signal: {"$abortSignal": name}`
//! is the signal of a controller named `name`, which the `signals` test
//! plugin's `abort(name)` aborts; the plugin's `console.warn` and
//! `console.debug` lines log as callbacks of those names. Frames are the
//! scenario clock's. On a live harness, `host: {warn, debug, releaseImage}`
//! replaces the console.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::Rect;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Promise, Token, define, is_undefined};
use plugin_conformance::js::{arg, from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::js::{dom_abort_error, number_to_string};
use crate::policy::{
    CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, PageRenderViewport, SnapOptions,
    snap_appearance_scale, snap_full_page_viewport, snap_tile_scale,
};
use crate::tiles::{self, PageSizePt, TileCoord, TileGrid};
use crate::*;

// ── a live host ───────────────────────────────────────────────────────────────

/// A live host's globals: `host: {warn, debug, releaseImage}` in the
/// config, each a `$fn` the host answers (`console.warn` and
/// `console.debug` in a browser). `releaseImage` receives the `$host` id of
/// a handle whose last Rust clone dropped, as a bare number: the handle's
/// JSON itself would decode back to the live image on the way out, id and
/// all. A handle with no `$host` id has nothing to release. Only a live
/// harness reads it; a vector's config never has one.
fn live_host(host: &mut RenderHost, config: &Value, h: &Harness) {
    if !h.is_live() {
        return;
    }
    let Some(live) = config.get("host") else { return };
    let spec = |name: &str| live.get(name).filter(|f| f.get("$fn").is_some());
    if let Some(warn) = spec("warn").map(|f| h.callback(f)) {
        host.warn = Rc::new(move |message| {
            let _ = warn.call(vec![json!(message)]);
        });
    }
    if let Some(debug) = spec("debug").map(|f| h.callback(f)) {
        host.debug = Rc::new(move |message| {
            let _ = debug.call(vec![json!(message)]);
        });
    }
    if let Some(release) = spec("releaseImage").map(|f| h.callback(f)) {
        let warn = host.warn.clone();
        host.release_image = Some(Rc::new(move |image: &Value| {
            if let Some(id) = image.get("$host").filter(|id| id.is_number())
                && let Answer::Throws(error) = release.call(vec![id.clone()])
            {
                warn(&format!("[render] releaseImage threw: {error}"));
            }
        }));
    }
}

// ── the scenario adapter ──────────────────────────────────────────────────────

/// The api object literal's member order (the harness wrapper spreads it).
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "canRender",
    "renderPage",
    "renderThumbnail",
    "renderPages",
    "getRenderPolicy",
    "getRenderEpoch",
    "invalidate",
    "onInvalidated",
    "renderSource",
    "getSourceKey",
    "conformViewport",
    "getPaintSettings",
    "getLayerRights",
    "createViewDemand",
    "renderAppearances",
    "keptAppearances",
    "getAppearanceScale",
    "onRenderCompleted",
    "onRenderFailed",
];
const VIEW_MEMBERS: &[&str] =
    &["setDemand", "getPlan", "markPainted", "markUnpainted", "release", "dispose"];

thread_local! {
    /// The case's abort controllers by name; a render build starts a case.
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// The `signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> = LazyLock::new(|| Token::new("render-test/signals"));

pub struct Render;

fn present(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn page_ref(value: &Value) -> Result<PageRef, Error> {
    PageRef::from_json(value).ok_or_else(|| Error::named("TypeError", "[render] not a PageRef"))
}

fn page_ref_arg(args: &[Value], i: usize) -> Result<PageRef, Error> {
    page_ref(present(args, i).unwrap_or(&Value::Null))
}

/// `PageRef | number`.
fn page_target(value: &Value) -> Result<PageArg, Error> {
    match value.as_f64() {
        Some(index) => Ok(PageArg::Index(index)),
        None => page_ref(value).map(PageArg::Ref),
    }
}

fn page_target_arg(args: &[Value], i: usize) -> Result<PageArg, Error> {
    page_target(present(args, i).unwrap_or(&Value::Null))
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn member<'a>(options: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    options.and_then(|o| o.get(key)).filter(|v| !v.is_null() && !is_undefined(v))
}

fn number(options: Option<&Value>, key: &str) -> Option<f64> {
    member(options, key).and_then(Value::as_f64)
}

/// `PageLayerOptions`: a boolean member as given; `null` and an absent one
/// are both left out.
fn layers(options: Option<&Value>) -> PageLayerOptions {
    PageLayerOptions {
        include_annotations: member(options, "includeAnnotations").and_then(Value::as_bool),
        include_form_fields: member(options, "includeFormFields").and_then(Value::as_bool),
    }
}

fn signal_of(options: Option<&Value>) -> Option<AbortSignal> {
    member(options, "signal")
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named)
}

fn page_options(
    options: Option<&Value>,
    signal: Option<AbortSignal>,
) -> Result<RenderPageOptions, Error> {
    let format = match member(options, "format") {
        Some(format) => Some(from_value::<RenderFormat>(format)?),
        None => None,
    };
    Ok(RenderPageOptions {
        width: number(options, "width"),
        scale: number(options, "scale"),
        layers: layers(options),
        format,
        quality: number(options, "quality"),
        signal,
    })
}

/// An event hook: subscribe a scripted callback, return its unsubscribe.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn appearance_options(options: Option<&Value>) -> Result<AppearancesOptions, Error> {
    let family = from_value::<AppearanceFamily>(member(options, "family").unwrap_or(&Value::Null))?;
    let items = member(options, "items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| AppearanceItem {
            reference: item.get("ref").cloned().unwrap_or(Value::Null),
            version: item.get("version").and_then(Value::as_f64).unwrap_or(f64::NAN),
        })
        .collect();
    Ok(AppearancesOptions {
        family,
        scale: number(options, "scale").unwrap_or(f64::NAN),
        items,
        signal: signal_of(options),
    })
}

impl Render {
    fn call_capability(
        &self,
        cap: &RenderCapability,
        method: &str,
        args: &[Value],
        h: &Harness,
    ) -> Result<Outcome, Error> {
        Ok(match method {
            "getSettings" => Outcome::Returned(to_value(&cap.get_settings())),
            "updateSettings" => {
                let changes: RenderConfig = match present(args, 0) {
                    Some(changes) => from_value(changes)?,
                    None => RenderConfig::default(),
                };
                cap.update_settings(&changes);
                Outcome::undefined()
            }
            "resetSettings" => {
                cap.reset_settings();
                Outcome::undefined()
            }
            "onSettingsChanged" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_settings_changed(f))
            }
            "canRender" => Outcome::Returned(json!(cap.can_render())),
            "renderPage" => {
                let options = present(args, 1);
                let options = page_options(options, signal_of(options))?;
                Outcome::promise(cap.render_page(page_target_arg(args, 0)?, options))
            }
            "renderThumbnail" => {
                let options = present(args, 1);
                let options = RenderThumbnailOptions {
                    max_width: number(options, "maxWidth").unwrap_or(f64::NAN),
                    layers: layers(options),
                    signal: signal_of(options),
                };
                Outcome::promise(cap.render_thumbnail(page_target_arg(args, 0)?, options))
            }
            "renderPages" => {
                let pages = present(args, 0)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(page_target)
                    .collect::<Result<Vec<_>, _>>()?;
                let options = present(args, 1);
                let options = RenderPagesOptions {
                    page: page_options(options, None)?,
                    concurrency: number(options, "concurrency"),
                };
                Outcome::promise(cap.render_pages(pages, options))
            }
            "getRenderPolicy" => Outcome::Returned(to_value(&cap.get_render_policy())),
            "getRenderEpoch" => Outcome::Returned(json!(
                cap.get_render_epoch(page_target_arg(args, 0)?, layers(present(args, 1)))
            )),
            "invalidate" => {
                let options = present(args, 0);
                let pages = match member(options, "pages").and_then(Value::as_array) {
                    Some(pages) => Some(pages.iter().map(page_target).collect::<Result<_, _>>()?),
                    None => None,
                };
                let scope = match member(options, "scope") {
                    Some(scope) => Some(from_value::<InvalidateScope>(scope)?),
                    None => None,
                };
                cap.invalidate(InvalidateOptions { pages, scope });
                Outcome::undefined()
            }
            "onInvalidated" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_invalidated(f))
            }
            "renderSource" => {
                let options = present(args, 1);
                let options = RenderSourceOptions {
                    scale: number(options, "scale").unwrap_or(f64::NAN),
                    view: member(options, "view").and_then(Value::as_str).map(str::to_string),
                    layers: layers(options),
                    signal: signal_of(options),
                };
                Outcome::promise(cap.render_source(page_ref_arg(args, 0)?, options))
            }
            "getSourceKey" => {
                let options = present(args, 1);
                let scale = number(options, "scale").unwrap_or(f64::NAN);
                Outcome::from_result(
                    cap.get_source_key(page_ref_arg(args, 0)?, scale, layers(options))
                        .map(Value::String),
                )
            }
            "conformViewport" => {
                let scale = present(args, 1).and_then(Value::as_f64).unwrap_or(f64::NAN);
                Outcome::from_result(
                    cap.conform_viewport(page_ref_arg(args, 0)?, scale).map(|v| to_value(&v)),
                )
            }
            "getPaintSettings" => Outcome::Returned(to_value(&*cap.get_paint_settings())),
            "getLayerRights" => Outcome::Returned(to_value(&cap.get_layer_rights())),
            "createViewDemand" => {
                let view = present(args, 0).and_then(Value::as_str).unwrap_or_default();
                let handle: Rc<dyn Any> = Rc::new(cap.create_view_demand(view));
                match h.try_describe(handle) {
                    Some(Ok(described)) => Outcome::Returned(described),
                    Some(Err(error)) => Outcome::Threw(error),
                    None => Outcome::Returned(json!({})),
                }
            }
            "renderAppearances" => {
                let options = appearance_options(present(args, 1))?;
                let pictures = cap.render_appearances(page_target_arg(args, 0)?, options);
                Outcome::promise(Promise::spawn(async move { Ok(to_value(&*pictures.await?)) }))
            }
            "keptAppearances" => {
                let family =
                    from_value::<AppearanceFamily>(present(args, 0).unwrap_or(&Value::Null))?;
                let item = present(args, 1).cloned().unwrap_or(Value::Null);
                Outcome::Returned(to_value(&*cap.kept_appearances(family, &item)))
            }
            "getAppearanceScale" => {
                let scale = present(args, 0).and_then(Value::as_f64).unwrap_or(f64::NAN);
                Outcome::from_result(cap.get_appearance_scale(scale).map(|s| json!(s)))
            }
            "onRenderCompleted" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_render_completed(f))
            }
            "onRenderFailed" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_render_failed(f))
            }
            other => not_a_function(other),
        })
    }

    fn call_view(&self, view: &ViewDemand, method: &str, args: &[Value]) -> Result<Outcome, Error> {
        let key = || present(args, 1).and_then(Value::as_str).unwrap_or_default().to_string();
        Ok(match method {
            "setDemand" => {
                let demand: PageViewDemand = from_value(present(args, 1).unwrap_or(&Value::Null))?;
                let page = page_ref_arg(args, 0)?;
                view.set_demand(page, demand, layers(present(args, 2)));
                Outcome::undefined()
            }
            "getPlan" => Outcome::Returned(to_value(&*view.get_plan(page_ref_arg(args, 0)?))),
            "markPainted" => {
                view.mark_painted(page_ref_arg(args, 0)?, &key());
                Outcome::undefined()
            }
            "markUnpainted" => {
                view.mark_unpainted(page_ref_arg(args, 0)?, &key());
                Outcome::undefined()
            }
            "release" => {
                view.release(page_ref_arg(args, 0)?);
                Outcome::undefined()
            }
            "dispose" => {
                view.dispose();
                Outcome::undefined()
            }
            other => not_a_function(other),
        })
    }
}

impl HarnessPlugin for Render {
    fn factories(&self) -> Vec<&'static str> {
        vec!["render", "signals"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "signals" {
            return Ok(define::<Value, Value>("render-test/signals")
                .capability(&SIGNALS, |_| Ok(Signals))
                .build());
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        let raw = config;
        let config: RenderConfig = if is_undefined(config) || config.is_null() {
            RenderConfig::default()
        } else {
            serde_json::from_value(config.clone())
                .map_err(|e| Error::new(format!("[render] bad config: {e}")))?
        };
        let (warn_h, debug_h) = (h.clone(), h.clone());
        let mut host = RenderHost {
            warn: Rc::new(move |message| warn_h.log("console.warn", vec![json!(message)])),
            debug: Rc::new(move |message| debug_h.log("console.debug", vec![json!(message)])),
            release_image: None,
        };
        live_host(&mut host, raw, h);
        Ok(render_plugin_with_host(config, host))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![RENDER.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<RenderCapability>() {
            return Some(
                self.call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw),
            );
        }
        if let Some(view) = capability.downcast_ref::<ViewDemand>() {
            return Some(self.call_view(view, method, &args).unwrap_or_else(Outcome::Threw));
        }
        capability.downcast_ref::<Signals>()?;
        Some(match method {
            "abort" => {
                let name = present(&args, 0).and_then(Value::as_str).unwrap_or_default();
                controller_named(name).abort(dom_abort_error());
                Outcome::undefined()
            }
            other => not_a_function(other),
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<RenderCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        if capability.downcast_ref::<ViewDemand>().is_some() {
            return Some(VIEW_MEMBERS.to_vec());
        }
        capability.downcast_ref::<Signals>()?;
        Some(vec!["abort"])
    }
}

// ── the function table ────────────────────────────────────────────────────────

type Out = Result<Value, Error>;

fn ok<T: serde::Serialize>(value: &T) -> Out {
    Ok(to_value(value))
}

fn tile_grid(a: &[Value]) -> Out {
    ok(&tiles::tile_grid(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))
}
fn tiles_in_rect(a: &[Value]) -> Out {
    ok(&tiles::tiles_in_rect(&arg::<TileGrid>(a, 0)?, arg(a, 1)?, arg(a, 2)?))
}
fn tile_paint_rect(a: &[Value]) -> Out {
    ok(&tiles::tile_paint_rect(&arg::<TileGrid>(a, 0)?, arg(a, 1)?, arg(a, 2)?))
}
fn bleed_rect(a: &[Value]) -> Out {
    ok(&tiles::bleed_rect(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))
}
fn region_covered(a: &[Value]) -> Out {
    let grid: TileGrid = arg(a, 0)?;
    let page: PageSizePt = arg(a, 1)?;
    let region: Rect = arg(a, 2)?;
    let painted: Vec<String> = arg(a, 3)?;
    let mut asked = Vec::new();
    let covered = tiles::region_covered(&grid, page, region, |c: TileCoord| {
        let key = format!("{},{}", number_to_string(c.ix), number_to_string(c.iy));
        let hit = painted.contains(&key);
        asked.push(key);
        hit
    });
    Ok(json!({ "covered": covered, "asked": asked }))
}
fn inflate_rect(a: &[Value]) -> Out {
    ok(&tiles::inflate_rect(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))
}
fn snap_to_pyramid(a: &[Value]) -> Out {
    ok(&tiles::snap_to_pyramid(&arg::<Vec<f64>>(a, 0)?, arg(a, 1)?))
}
fn intersect_rects(a: &[Value]) -> Out {
    ok(&tiles::intersect_rects(arg(a, 0)?, arg(a, 1)?))
}

fn resolve_render_options_fn(a: &[Value]) -> Out {
    ok(&resolve_render_options(&arg::<RenderConfig>(a, 0)?))
}
fn resolve_strategy_fn(a: &[Value]) -> Out {
    let policy: EngineRenderPolicy = arg(a, 0)?;
    ok(&resolve_strategy(&policy, &resolve_render_options(&arg(a, 1)?)))
}
fn base_ask_width_fn(a: &[Value]) -> Out {
    let policy: EngineRenderPolicy = arg(a, 0)?;
    let strategy = resolve_strategy(&policy, &resolve_render_options(&arg(a, 1)?));
    match base_ask_width(&strategy, arg(a, 2)?) {
        Some(width) => ok(&width),
        None => Ok(epdf_kernel::undefined()),
    }
}
fn default_tile_pyramid(_: &[Value]) -> Out {
    ok(&DEFAULT_TILE_PYRAMID)
}
fn empty_tile_plan_fn(_: &[Value]) -> Out {
    ok(&*empty_tile_plan())
}

fn continuous_render_policy(_: &[Value]) -> Out {
    ok(&CONTINUOUS_RENDER_POLICY)
}
fn snap_full_page_viewport_fn(a: &[Value]) -> Out {
    let policy: EngineRenderPolicy = arg(a, 0)?;
    let viewport: PageRenderViewport = arg(a, 1)?;
    let opts: Option<SnapOptions> = arg(a, 2)?;
    ok(&snap_full_page_viewport(&policy, viewport, opts)?)
}
fn snap_appearance_scale_fn(a: &[Value]) -> Out {
    ok(&snap_appearance_scale(&arg(a, 0)?, arg(a, 1)?)?)
}
fn snap_tile_scale_fn(a: &[Value]) -> Out {
    ok(&snap_tile_scale(&arg(a, 0)?, arg(a, 1)?)?)
}

fn initial_render_state_fn(_: &[Value]) -> Out {
    ok(&initial_render_state())
}
/// `{changed: next !== state, state: next}`.
fn invalidate_pages_fn(a: &[Value]) -> Out {
    // `serde_json` itself: page object numbers key the ledgers as strings.
    let state: RenderState = serde_json::from_value(a[0].clone())
        .map_err(|e| Error::new(format!("[render] bad state: {e}")))?;
    let pons: Vec<u32> = arg(a, 1)?;
    let scope: InvalidateScope = arg(a, 2)?;
    let next = invalidate_pages(&state, &pons, scope);
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}
fn pixel_change_of_fn(a: &[Value]) -> Out {
    let pons: Vec<u32> = arg(a, 1)?;
    Ok(match pixel_change_of(&a[0], || pons) {
        Some(change) => to_value(&change),
        None => Value::Null,
    })
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("tileGrid", tile_grid),
        ("tilesInRect", tiles_in_rect),
        ("tilePaintRect", tile_paint_rect),
        ("bleedRect", bleed_rect),
        ("regionCovered", region_covered),
        ("inflateRect", inflate_rect),
        ("snapToPyramid", snap_to_pyramid),
        ("intersectRects", intersect_rects),
        ("resolveRenderOptions", resolve_render_options_fn),
        ("resolveStrategy", resolve_strategy_fn),
        ("baseAskWidth", base_ask_width_fn),
        ("DEFAULT_TILE_PYRAMID", default_tile_pyramid),
        ("EMPTY_TILE_PLAN", empty_tile_plan_fn),
        ("CONTINUOUS_RENDER_POLICY", continuous_render_policy),
        ("snapFullPageViewport", snap_full_page_viewport_fn),
        ("snapAppearanceScale", snap_appearance_scale_fn),
        ("snapTileScale", snap_tile_scale_fn),
        ("initialRenderState", initial_render_state_fn),
        ("invalidatePages", invalidate_pages_fn),
        ("pixelChangeOf", pixel_change_of_fn),
    ]
}
