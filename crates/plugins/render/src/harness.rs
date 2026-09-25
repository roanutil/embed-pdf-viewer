//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector, calls its capability and tile handles by name, and
//! the function table the pure suites call by their TypeScript names.
//!
//! It mirrors the shims in `tooling/plugin-conformance/src/suites/render.ts`:
//! a `renderPage` option `signal: {"$abortSignal": name}` is the signal of a
//! controller named `name`, which the `signals` test plugin's `abort(name)`
//! aborts; the plugin's `console.warn` and `console.debug` lines log as
//! callbacks of those names; and the frame scheduler is the TypeScript's
//! Node fallback, `setTimeout(cb, 16)`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::Rect;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, define, is_undefined};
use plugin_conformance::js::{arg, from_value, to_value};
use plugin_conformance::{FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::js::{dom_abort_error, number_to_string};
use crate::policy::{
    CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, PageRenderViewport, SnapOptions,
    snap_appearance_scale, snap_full_page_viewport, snap_tile_scale,
};
use crate::tiles::{self, PageSizePt, TileCoord, TileGrid};
use crate::types::{InvalidateOptions, RenderAction, RenderHost, RenderPageOptions, RenderState};
use crate::*;

// ── the scenario adapter ──────────────────────────────────────────────────────

/// The capability object literal's member order.
const MEMBERS: &[&str] = &[
    "canRender",
    "renderPage",
    "renderSourceKey",
    "conformViewport",
    "paintSettings",
    "renderPolicy",
    "tilesFor",
    "renderEpoch",
    "invalidate",
];
const VIEW_MEMBERS: &[&str] = &["plan", "painted", "unpainted", "release"];

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

fn pon_arg(args: &[Value]) -> u32 {
    present(args, 0).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

/// `includeAnnotations ?? true`: absent and null are both true.
fn include_annotations(options: Option<&Value>) -> Option<bool> {
    options.and_then(|o| o.get("includeAnnotations")).and_then(Value::as_bool)
}

impl Render {
    fn call_capability(
        &self,
        cap: &RenderCapability,
        method: &str,
        args: &[Value],
        h: &Harness,
    ) -> Outcome {
        match method {
            "canRender" => Outcome::Returned(json!(cap.can_render())),
            "renderPage" => {
                let options = present(args, 1);
                let signal = options
                    .and_then(|o| o.get("signal"))
                    .and_then(|s| s.get("$abortSignal"))
                    .and_then(Value::as_str)
                    .map(controller_named);
                let options = RenderPageOptions {
                    scale: options
                        .and_then(|o| o.get("scale"))
                        .and_then(Value::as_f64)
                        .unwrap_or(f64::NAN),
                    include_annotations: include_annotations(options),
                    signal,
                };
                match cap.render_page(pon_arg(args), options) {
                    Ok(promise) => Outcome::promise(promise),
                    Err(error) => Outcome::Threw(error),
                }
            }
            "renderSourceKey" => {
                let options = present(args, 1);
                let scale = options
                    .and_then(|o| o.get("scale"))
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NAN);
                Outcome::from_result(
                    cap.render_source_key(pon_arg(args), scale, include_annotations(options))
                        .map(Value::String),
                )
            }
            "conformViewport" => {
                let scale = present(args, 1).and_then(Value::as_f64).unwrap_or(f64::NAN);
                Outcome::from_result(
                    cap.conform_viewport(pon_arg(args), scale).map(|v| to_value(&v)),
                )
            }
            "paintSettings" => Outcome::Returned(to_value(&cap.paint_settings())),
            "renderPolicy" => Outcome::Returned(to_value(&cap.render_policy())),
            "tilesFor" => {
                let view = present(args, 0).and_then(Value::as_str).unwrap_or_default();
                let handle: Rc<dyn Any> = Rc::new(cap.tiles_for(view));
                Outcome::Returned(h.describe(handle).unwrap_or_else(|| json!({})))
            }
            "renderEpoch" => {
                // A default parameter: only `undefined` is true; `null` is falsy.
                let annotations = match present(args, 1) {
                    None => true,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => false,
                };
                Outcome::from_result(cap.render_epoch(pon_arg(args), annotations).map(|e| json!(e)))
            }
            "invalidate" => {
                let options: InvalidateOptions = present(args, 0)
                    .map(|o| serde_json::from_value(o.clone()).unwrap_or_default())
                    .unwrap_or_default();
                cap.invalidate(options);
                Outcome::undefined()
            }
            other => not_a_function(other),
        }
    }

    fn call_view(&self, view: &ViewTiles, method: &str, args: &[Value]) -> Outcome {
        let pon = pon_arg(args);
        let key = || present(args, 1).and_then(Value::as_str).unwrap_or_default().to_string();
        match method {
            "plan" => {
                let demand: PageViewDemand =
                    match from_value(present(args, 1).unwrap_or(&Value::Null)) {
                        Ok(demand) => demand,
                        Err(error) => return Outcome::Threw(error),
                    };
                Outcome::from_result(
                    view.plan(pon, &demand, include_annotations(present(args, 2)))
                        .map(|plan| to_value(&*plan)),
                )
            }
            "painted" => {
                view.painted(pon, &key());
                Outcome::undefined()
            }
            "unpainted" => {
                view.unpainted(pon, &key());
                Outcome::undefined()
            }
            "release" => {
                view.release(pon);
                Outcome::undefined()
            }
            other => not_a_function(other),
        }
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
        let options: RenderPluginOptions = if is_undefined(config) || config.is_null() {
            RenderPluginOptions::default()
        } else {
            serde_json::from_value(config.clone())
                .map_err(|e| Error::new(format!("[render] bad config: {e}")))?
        };
        let (warn_h, debug_h) = (h.clone(), h.clone());
        let host = RenderHost {
            request_frame: Rc::new(|frame| {
                epdf_kernel::rt::set_timeout(16.0, frame);
            }),
            warn: Rc::new(move |message| warn_h.log("console.warn", vec![json!(message)])),
            debug: Rc::new(move |message| debug_h.log("console.debug", vec![json!(message)])),
        };
        Ok(render_plugin_with_host(options, host))
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
            return Some(self.call_capability(cap, method, &args, h));
        }
        if let Some(view) = capability.downcast_ref::<ViewTiles>() {
            return Some(self.call_view(view, method, &args));
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
        if capability.downcast_ref::<ViewTiles>().is_some() {
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
fn to_engine_rect(a: &[Value]) -> Out {
    ok(&tiles::to_engine_rect(arg(a, 0)?, arg(a, 1)?))
}
fn tile_engine_rect(a: &[Value]) -> Out {
    ok(&tiles::tile_engine_rect(&arg::<TileGrid>(a, 0)?, arg(a, 1)?, arg(a, 2)?))
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
    ok(&resolve_render_options(&arg::<RenderPluginOptions>(a, 0)?))
}
fn resolve_strategy_fn(a: &[Value]) -> Out {
    let policy: EngineRenderPolicy = arg(a, 0)?;
    ok(&resolve_strategy(&policy, &resolve_render_options(&arg(a, 1)?)))
}
fn base_ask_width_fn(a: &[Value]) -> Out {
    let policy: EngineRenderPolicy = arg(a, 0)?;
    let strategy = resolve_strategy(&policy, &resolve_render_options(&arg(a, 1)?));
    ok(&base_ask_width(&strategy, arg(a, 2)?))
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
/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn render_reducer_fn(a: &[Value]) -> Out {
    let state: RenderState = serde_json::from_value(a[0].clone())
        .map_err(|e| Error::new(format!("[render] bad state: {e}")))?;
    let next = match serde_json::from_value::<RenderAction>(a[1].clone()) {
        Ok(action) => render_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}
fn annotated_pons_fn(a: &[Value]) -> Out {
    let pons: Vec<u32> = arg(a, 1)?;
    ok(&annotated_pons(&a[0], || pons))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("tileGrid", tile_grid),
        ("tilesInRect", tiles_in_rect),
        ("tilePaintRect", tile_paint_rect),
        ("toEngineRect", to_engine_rect),
        ("tileEngineRect", tile_engine_rect),
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
        ("renderReducer", render_reducer_fn),
        ("annotatedPons", annotated_pons_fn),
    ]
}
