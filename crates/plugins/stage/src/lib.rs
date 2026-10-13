//! `packages/plugin/stage` in Rust: the viewport lens. One camera, scene
//! and flat settings bag per document: zoom, scroll, spreads, layout,
//! motion over a host frame seam, responsive settings, PDF destinations,
//! and the pan handler for the interaction hub.
//!
//! The module split follows the TypeScript package: [`contract`],
//! [`host_contract`] and [`token`] (the lenses), [`model`], [`settings`],
//! [`motion`], [`responsive`], [`destination`] (the `/destination` entry),
//! [`controller`] with its areas (`services`, `read`, `camera`,
//! `navigation`, `settings::area`, `view`), [`sync`] (the watchers),
//! [`scroll_handler`], and the wiring in [`stage_plugin`]
//! (`stage.plugin.ts`). Names mirror the TypeScript v3 contract
//! (`getPageFrame` is [`StageCapability::get_page_frame`]). The spatial math
//! is `epdf-stage-core`.

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

pub mod contract;
pub mod controller;
pub mod destination;
pub mod host_contract;
pub mod model;
pub mod motion;
pub mod responsive;
pub mod scroll_handler;
pub mod settings;
pub mod token;

mod prelude;

/// `camera/`: the write path, motion and direct manipulation.
pub mod camera {
    pub mod animation;
    pub mod gestures;
    pub mod write;
}

/// `navigation/`: arrivals, reveals and the zoom verbs.
pub mod navigation {
    pub mod arrive;
    pub mod reveal;
    pub mod zoom;
}

/// `read/`: the page reads.
pub mod read {
    pub mod pages;
}

pub mod services;

/// `sync/`: the steady-state watchers.
pub mod sync {
    pub mod watchers;
}

/// `view/`: the view lifecycle.
pub mod view {
    pub mod lifecycle;
}

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, Controller, Error, Token, define};
use epdf_plugin_actions::{ACTIONS, ActionExecutor, ActionExecutorResult};
use serde_json::Value;
use std::rc::Rc;

pub use contract::*;
pub use controller::{StageCapability, create_stage_controller};
pub use destination::{
    DestinationReveal, PageBoxes, PageLayout, PdfDestination, destination_to_reveal,
    without_undefined,
};
pub use host_contract::GestureOptions;
pub use model::{MotionCause, StageAction, StageState, initial_stage_state, stage_reducer};
pub use responsive::{box_of, matches_query, resolve_responsive};
pub use scroll_handler::{ScrollHandlerOptions, create_scroll_handler};
pub use settings::{default_responsive, default_settings, settings_equal};
pub use sync::watchers::register_stage_watchers;
pub use token::STAGE;

/// Options for registering a lens: a document may be viewed through
/// several stages at once, each with its own id and token.
#[derive(Clone, Default)]
pub struct StagePluginOptions {
    /// Plugin id; `'stage'` for the main lens.
    pub id: Option<String>,
    /// The lens's token; [`STAGE`] by default.
    pub token: Option<Token<StageCapability>>,
    pub config: StageConfig,
}

fn resolved(result: ActionExecutorResult) -> Result<Promise<ActionExecutorResult>, Error> {
    Ok(Promise::resolved(result))
}

fn inert(reason: impl Into<String>) -> Result<Promise<ActionExecutorResult>, Error> {
    resolved(ActionExecutorResult::Inert { reason: reason.into() })
}

fn failed(error: impl Into<String>) -> Result<Promise<ActionExecutorResult>, Error> {
    resolved(ActionExecutorResult::Failed { error: error.into() })
}

fn smooth() -> Option<GoToOptions> {
    Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth))
}

/// `value[key]` as JavaScript reads it: absent, an `undefined` marker, or a
/// non-object all read as undefined (`None`).
fn js_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

/// The navigation executors the main lens gives the action engine. The
/// bodies resolve the stage at call time.
fn goto_executor(
    ctx: Controller<StageState, StageAction>,
    token: Token<StageCapability>,
) -> ActionExecutor {
    Rc::new(move |node: &Value, _| {
        if node.get("type").and_then(Value::as_str) != Some("goto") {
            return inert("not a goto node");
        }
        let stage = ctx.try_get(&token)?;
        // The TypeScript reads `node.destination.page.pageObjectNumber`
        // inside its `pages.find` callback, so a missing destination or page
        // throws there, but only when the document has a page to test.
        let has_pages = ctx
            .document()
            .is_some_and(|doc| doc.pages.as_array().is_some_and(|pages| !pages.is_empty()));
        if has_pages {
            let (missing, key) = match js_get(node, "destination") {
                Some(Value::Null) => (Some("null"), "page"),
                None => (Some("undefined"), "page"),
                Some(destination) => match js_get(destination, "page") {
                    Some(Value::Null) => (Some("null"), "pageObjectNumber"),
                    None => (Some("undefined"), "pageObjectNumber"),
                    Some(_) => (None, ""),
                },
            };
            if let Some(missing) = missing {
                return Err(Error::named(
                    "TypeError",
                    format!("Cannot read properties of {missing} (reading '{key}')"),
                ));
            }
        }
        let destination: Option<PdfDestination> =
            node.get("destination").and_then(|d| serde_json::from_value(without_undefined(d)).ok());
        let pon = node
            .get("destination")
            .and_then(|d| d.get("page"))
            .and_then(|p| p.get("pageObjectNumber"))
            .and_then(Value::as_f64);
        let layout = ctx.document().and_then(|doc| {
            doc.pages.as_array().and_then(|pages| {
                pages
                    .iter()
                    .find(|p| {
                        p.get("ref").and_then(|r| r.get("pageObjectNumber")).and_then(Value::as_f64)
                            == pon
                    })
                    .and_then(|p| serde_json::from_value::<PageLayout>(without_undefined(p)).ok())
            })
        });
        let (Some(stage), Some(layout)) = (stage, layout) else {
            return failed("no stage or destination page available");
        };
        let destination = destination.ok_or_else(|| {
            Error::named("TypeError", "[stage] a goto node without an explicit destination")
        })?;
        let reveal = destination_to_reveal(&destination, &layout, None)
            .map_err(|e| Error::new(e.to_string()))?;
        stage.reveal_index(
            reveal.page_index as isize,
            Some(RevealOptions { behavior: Some(ScrollBehaviorKind::Smooth), ..reveal.options }),
        );
        resolved(ActionExecutorResult::Executed)
    })
}

fn named_executor(
    ctx: Controller<StageState, StageAction>,
    token: Token<StageCapability>,
) -> ActionExecutor {
    Rc::new(move |node: &Value, _| {
        if node.get("type").and_then(Value::as_str) != Some("named") {
            return inert("not a named node");
        }
        let Some(stage) = ctx.try_get(&token)? else {
            return failed("no stage available");
        };
        // Page verbs only: the dispatcher owns /N Print.
        let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
        match name {
            "NextPage" => stage.next_page(smooth()),
            "PrevPage" => stage.previous_page(smooth()),
            "FirstPage" => stage.go_to_first_page(smooth()),
            "LastPage" => stage.go_to_last_page(smooth()),
            other => return inert(format!("unknown named action '{other}'")),
        }
        resolved(ActionExecutorResult::Executed)
    })
}

/// `registerExecutors(ctx, token, id)`: the main lens only, so a thumbnail
/// lens never wins the last-wins registry.
fn register_executors(
    ctx: &Controller<StageState, StageAction>,
    token: &Token<StageCapability>,
    main_lens: bool,
) -> Result<(), Error> {
    if !main_lens {
        return Ok(());
    }
    let Some(actions) = ctx.try_get(&ACTIONS)? else { return Ok(()) };
    let off = actions.register_executor("goto", goto_executor(ctx.clone(), token.clone()));
    ctx.cleanup_fn(move || off());
    let off = actions.register_executor("named", named_executor(ctx.clone(), token.clone()));
    ctx.cleanup_fn(move || off());
    Ok(())
}

/// `stagePlugin(options)`: the manifest. Document-scoped, one instance of
/// this lens per open document, with actions as an optional dependency.
/// Initial placement is not an effect: it is level-triggered inside
/// `set_viewport_size`. The watchers are steady-state.
pub fn stage_plugin(options: StagePluginOptions) -> AnyPlugin {
    let id = options.id.unwrap_or_else(|| "stage".to_string());
    let token = options.token.unwrap_or_else(|| STAGE.clone());
    let config = options.config;
    let settings = config.settings.clone();
    let main_lens = id == "stage";
    let effects_token = token.clone();
    let create_token = token.clone();
    define::<StageState, StageAction>(id)
        .document_scoped()
        .optional(ACTIONS.any())
        .initial_state(move || initial_stage_state(&settings))
        .reduce(stage_reducer)
        .effects(move |fx| register_stage_watchers(fx, effects_token.clone(), main_lens))
        .create(&token, move |cx| {
            let stage = create_stage_controller(cx.clone(), config.clone());
            let token = create_token.clone();
            Ok(controller::created(stage, move || register_executors(&cx, &token, main_lens)))
        })
        .build()
}
