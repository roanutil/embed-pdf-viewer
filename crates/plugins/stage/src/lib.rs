//! `packages/plugin/stage` in Rust: the viewport lens. One camera, scene
//! and flat settings bag per document: zoom, scroll, spreads, layout,
//! motion over a host frame seam, responsive settings, PDF destinations,
//! and the pan handler for the interaction hub.
//!
//! The file split follows the TypeScript package: `types`, `settings`,
//! `reducer`, `motion`, `responsive`, `destination` (the `/destination`
//! entry), `capability`, `effects`, `scroll_handler`, and the wiring in
//! [`stage_plugin`] (`stage.plugin.ts`). The spatial math is
//! `epdf-stage-core`.

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

pub mod capability;
pub mod destination;
pub mod effects;
pub mod motion;
pub mod reducer;
pub mod responsive;
pub mod scroll_handler;
pub mod settings;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, Ctx, Error, Token, define};
use epdf_plugin_actions::{ACTIONS, ActionExecutor, ActionExecutorResult};
use serde_json::Value;
use std::rc::Rc;

pub use capability::{StageCapability, create_stage_capability};
pub use destination::{
    DestinationReveal, PageBoxes, PageLayout, PdfDestination, destination_to_reveal,
    without_undefined,
};
pub use effects::register_stage_effects;
pub use reducer::{initial_stage_state, stage_reducer};
pub use responsive::{box_of, matches_query, resolve_responsive};
pub use scroll_handler::{ScrollHandlerOptions, create_scroll_handler};
pub use settings::{default_responsive, default_settings, settings_equal};
pub use types::*;

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

/// The navigation executors the main lens gives the action engine. The
/// bodies resolve the stage at call time.
fn goto_executor(
    ctx: Ctx<StageState, StageAction>,
    token: Token<StageCapability>,
) -> ActionExecutor {
    Rc::new(move |node: &Value, _| {
        if node.get("type").and_then(Value::as_str) != Some("goto") {
            return inert("not a goto node");
        }
        let stage = ctx.try_get(&token)?;
        let destination: Option<PdfDestination> =
            node.get("destination").and_then(|d| serde_json::from_value(without_undefined(d)).ok());
        let pon =
            node.get("destination").and_then(|d| d.get("pageObjectNumber")).and_then(Value::as_f64);
        let layout = ctx.document().and_then(|doc| {
            doc.pages.as_array().and_then(|pages| {
                pages
                    .iter()
                    .find(|p| p.get("pageObjectNumber").and_then(Value::as_f64) == pon)
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
        stage.reveal(
            reveal.page_index as isize,
            Some(RevealOptions { behavior: Some(ScrollBehaviorKind::Smooth), ..reveal.options }),
        );
        resolved(ActionExecutorResult::Executed)
    })
}

fn named_executor(
    ctx: Ctx<StageState, StageAction>,
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
            "NextPage" => stage.next(smooth()),
            "PrevPage" => stage.prev(smooth()),
            "FirstPage" => stage.go_to_page(0, smooth()),
            "LastPage" => {
                stage.go_to_page((stage.page_count() as isize - 1).max(0), smooth());
            }
            other => return inert(format!("unknown named action '{other}'")),
        }
        resolved(ActionExecutorResult::Executed)
    })
}

/// `stagePlugin(options)`: the manifest. Document-scoped, one instance of
/// this lens per open document, with actions as an optional dependency.
pub fn stage_plugin(options: StagePluginOptions) -> AnyPlugin {
    let id = options.id.unwrap_or_else(|| "stage".to_string());
    let token = options.token.unwrap_or_else(|| STAGE.clone());
    let config = options.config;
    let settings = config.settings.clone();
    let main_lens = id == "stage";
    let effects_token = token.clone();
    let init_token = token.clone();
    define::<StageState, StageAction>(id)
        .document_scoped()
        .optional(ACTIONS.any())
        .initial_state(move || initial_stage_state(&settings))
        .reduce(stage_reducer)
        .capability(&token, move |ctx| Ok(create_stage_capability(ctx, config.clone())))
        .effects(move |fx| register_stage_effects(fx, effects_token.clone(), main_lens))
        .init(move |ctx| {
            // Registered by the main lens only, so a thumbnail lens never
            // wins the last-wins registry.
            if !main_lens {
                return Ok(None);
            }
            let Some(actions) = ctx.try_get(&ACTIONS)? else { return Ok(None) };
            let off =
                actions.register_executor("goto", goto_executor(ctx.clone(), init_token.clone()));
            ctx.cleanup_fn(move || off());
            let off =
                actions.register_executor("named", named_executor(ctx.clone(), init_token.clone()));
            ctx.cleanup_fn(move || off());
            Ok(None)
        })
        .build()
}
