//! `connect.ts`: the stage's wiring to its surroundings, run once every
//! dependency exists. The refit on page-registry changes and the working
//! set (every lens) and, for the default lens only, the page-state feed and
//! the navigation executors of the actions plugin. A thumbnail lens must
//! never report page state or win the last-wins executor registry.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error};
use epdf_plugin_actions::{
    ACTIONS, ActionExecutor, ActionExecutorResult, ActionsCapability, PageStateCause,
    PageStateReport,
};
use serde_json::Value;

use crate::contract::*;
use crate::controller::StageCapability;
use crate::model::{MotionCause, StageState};
use crate::working_set::connect_working_set;

/// The id of the main lens; additional lenses register under their own id.
pub const DEFAULT_LENS_ID: &str = "stage";

/// `connectStage(ctx, stage)`.
pub(crate) fn connect_stage(
    ctx: &Controller<StageState, Value>,
    stage: &StageCapability,
) -> Result<(), Error> {
    // Rotate, move and delete change the registry revision: re-resolve the
    // zoom intent and re-place. Changes only, never the initial value, so
    // it cannot race the level-triggered initial placement.
    let revision_ctx = ctx.clone();
    let refit = stage.clone();
    ctx.watch(
        move || Ok(revision_ctx.document().map_or(0, |d| d.revision)),
        move |_, _| refit.refit(),
        |a, b| a == b,
    )?;
    connect_working_set(ctx, stage)?;

    if ctx.id() != DEFAULT_LENS_ID {
        return Ok(());
    }
    let Some(actions) = ctx.try_get(&ACTIONS)? else { return Ok(()) };
    feed_page_state(ctx, stage, &actions)?;
    register_navigation_executors(ctx, stage, &actions);
    Ok(())
}

struct Snapshot {
    current_page: Option<PageRef>,
    visible_pages: Vec<PageRef>,
    cause: MotionCause,
}

/// The page-state feed: `{currentPage, visiblePages, placed, cause}` on
/// placement and on every change of the current page or the visible page
/// set. A cause flip alone is not a change.
fn feed_page_state(
    ctx: &Controller<StageState, Value>,
    stage: &StageCapability,
    actions: &ActionsCapability,
) -> Result<(), Error> {
    let snapshot = {
        let ctx = ctx.clone();
        let stage = stage.clone();
        Rc::new(move || -> Snapshot {
            let state = ctx.state();
            Snapshot {
                current_page: stage.get_current_page(),
                visible_pages: stage.list_visible_pages().iter().map(|p| p.page_ref).collect(),
                cause: state.motion_cause,
            }
        })
    };
    let placed = {
        let ctx = ctx.clone();
        move || ctx.state().placed
    };
    let signature = snapshot.clone();
    let signature_placed = placed.clone();
    let actions = actions.clone();
    ctx.watch(
        move || {
            if !signature_placed() {
                return Ok("unplaced".to_string());
            }
            let current = signature();
            let mut numbers: Vec<u32> =
                current.visible_pages.iter().map(|p| p.page_object_number).collect();
            numbers.sort();
            let numbers: Vec<String> = numbers.iter().map(u32::to_string).collect();
            let page =
                current.current_page.map_or("-1".to_string(), |p| p.page_object_number.to_string());
            Ok(format!("{page}|{}", numbers.join(",")))
        },
        move |_, _| {
            if !placed() {
                return Ok(());
            }
            let current = snapshot();
            actions.report_page_state(PageStateReport {
                current_page: current.current_page,
                visible_pages: current.visible_pages,
                placed: true,
                cause: match current.cause {
                    MotionCause::User => PageStateCause::User,
                    MotionCause::Programmatic => PageStateCause::Programmatic,
                },
            });
            Ok(())
        },
        |a: &String, b: &String| a == b,
    )?;
    Ok(())
}

fn resolved(result: ActionExecutorResult) -> Result<Promise<ActionExecutorResult>, Error> {
    Ok(Promise::resolved(result))
}

fn inert(reason: impl Into<String>) -> Result<Promise<ActionExecutorResult>, Error> {
    resolved(ActionExecutorResult::Inert { reason: reason.into() })
}

/// `value[key]` as JavaScript reads it: `None` for an absent member or an
/// `undefined` marker.
fn js_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

fn read_of(missing: &str, key: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of {missing} (reading '{key}')"))
}

/// `ctx.getPage(page) !== null` for a page member as JSON. `undefined` and
/// `null` throw reading `kind` in `pageRefsEqual`, but only when the
/// document has a page to compare.
fn has_page(stage: &StageCapability, page: Option<&Value>) -> Result<bool, Error> {
    let rows = stage.registry_rows();
    let has_rows = rows.as_array().is_some_and(|rows| !rows.is_empty());
    match page {
        None if has_rows => Err(read_of("undefined", "kind")),
        Some(Value::Null) if has_rows => Err(read_of("null", "kind")),
        None | Some(Value::Null) => Ok(false),
        Some(page) => Ok(match page.as_f64() {
            Some(index) => stage.get_page(PageTarget::Index(index)).is_some(),
            None => PageRef::from_json(page)
                .is_some_and(|page| stage.get_page(PageTarget::Ref(page)).is_some()),
        }),
    }
}

/// The GoTo and Named page verbs for the action engine, run as deferred
/// navigation effects. They move the view by its own `scrollBehavior`
/// setting, as a link does. /N Print is the dispatcher's.
fn register_navigation_executors(
    ctx: &Controller<StageState, Value>,
    stage: &StageCapability,
    actions: &ActionsCapability,
) {
    let goto: ActionExecutor = {
        let stage = stage.clone();
        Rc::new(move |node: &Value, _| {
            if node.get("type").and_then(Value::as_str) != Some("goto") {
                return inert("not a goto node");
            }
            let destination = match js_get(node, "destination") {
                None => return Err(read_of("undefined", "page")),
                Some(Value::Null) => return Err(read_of("null", "page")),
                Some(destination) => destination,
            };
            if !has_page(&stage, js_get(destination, "page"))? {
                return resolved(ActionExecutorResult::Failed {
                    error: "the destination page is not in this document".into(),
                });
            }
            match serde_json::from_value::<PageDestination>(destination.clone()) {
                Ok(destination) => stage.go_to_destination(&destination, None),
                // A kind `revealOfDestination` does not name gives no spot:
                // a bare reveal of the page.
                Err(_) => {
                    let page = js_get(destination, "page");
                    let target = match page.and_then(Value::as_f64) {
                        Some(index) => Some(PageTarget::Index(index)),
                        None => page.and_then(PageRef::from_json).map(PageTarget::Ref),
                    };
                    if let Some(index) = target.and_then(|target| stage.index_of(target)) {
                        stage.reveal(index, Some(RevealOptions::default()));
                    }
                }
            }
            resolved(ActionExecutorResult::Executed)
        })
    };
    let off = actions.register_executor("goto", goto);
    ctx.cleanup_fn(move || off());
    let named: ActionExecutor = {
        let stage = stage.clone();
        Rc::new(move |node: &Value, _| {
            if node.get("type").and_then(Value::as_str) != Some("named") {
                return inert("not a named node");
            }
            let name = node.get("name").and_then(Value::as_str).unwrap_or("undefined");
            match name {
                "NextPage" => stage.next_page(None),
                "PrevPage" => stage.previous_page(None),
                "FirstPage" => stage.go_to_first_page(None),
                "LastPage" => stage.go_to_last_page(None),
                other => return inert(format!("unknown named action '{other}'")),
            }
            resolved(ActionExecutorResult::Executed)
        })
    };
    let off = actions.register_executor("named", named);
    ctx.cleanup_fn(move || off());
}
