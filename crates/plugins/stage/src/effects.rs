//! `effects.ts`: the lens's two effects.
//!
//! 1. Refit on page-registry mutations: a rotate, move or delete bumps the
//!    document revision, and the lens re-resolves its zoom intent against
//!    the new footprint.
//! 2. The page-state feed (main lens only, actions present): stage is
//!    authoritative for what page the viewer is on, and reports
//!    `{currentPon, visiblePons, placed, cause}` on placement and on every
//!    change of the reportable truth.
//!
//! Both are `ctx.watch`es in the TypeScript. A selector there may throw
//! (resolving the capability while the document closes), and the throw
//! leaves the watch's previous value alone. `EffectCtx::watch` takes an
//! infallible selector, so these are written against `subscribe` with the
//! same steps.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::{EffectCtx, Error, Token};
use epdf_plugin_actions::{ACTIONS, PageStateCause, PageStateReport};

use crate::capability::{StageCapability, read_of_undefined};
use crate::types::*;

/// `ctx.watch(select, handler)` with a fallible selector and `is_equal`.
fn watch<R: Clone + 'static>(
    fx: &EffectCtx<StageState, StageAction>,
    select: impl Fn() -> Result<R, Error> + 'static,
    is_equal: impl Fn(&R, &R) -> bool + 'static,
    handler: impl Fn(&R, &R) -> Result<(), Error> + 'static,
) -> Result<(), Error> {
    let previous = RefCell::new(select()?);
    let unsubscribe = fx.ctx.ctx.subscribe(Rc::new(move || {
        let next = select()?;
        let prior = previous.borrow().clone();
        if !is_equal(&prior, &next) {
            *previous.borrow_mut() = next.clone();
            handler(&next, &prior)?;
        }
        Ok(())
    }));
    fx.ctx.ctx.cleanup_fn(move || unsubscribe.call());
    Ok(())
}

struct Snapshot {
    current_pon: Option<PageObjectNumber>,
    visible_pons: Vec<PageObjectNumber>,
    cause: MotionCause,
}

/// `registerStageEffects(ctx, token, feedActions)`.
pub fn register_stage_effects(
    fx: EffectCtx<StageState, StageAction>,
    token: Token<StageCapability>,
    feed_actions: bool,
) -> Result<(), Error> {
    let revision_ctx = fx.ctx.clone();
    let refit_ctx = fx.ctx.clone();
    let refit_token = token.clone();
    watch(
        &fx,
        move || Ok(revision_ctx.document().map_or(0, |d| d.revision)),
        |a, b| a == b,
        move |_, _| {
            refit_ctx.get(&refit_token)?.refit();
            Ok(())
        },
    )?;

    if !feed_actions {
        return Ok(());
    }
    let Some(actions) = fx.ctx.try_get(&ACTIONS)? else { return Ok(()) };

    let snapshot = {
        let ctx = fx.ctx.clone();
        Rc::new(move || -> Result<Option<Snapshot>, Error> {
            let state = ctx.try_state().ok_or_else(|| read_of_undefined("placed"))?;
            if !state.placed {
                return Ok(None);
            }
            let stage = ctx.get(&token)?;
            Ok(Some(Snapshot {
                current_pon: stage.pages().get(state.cursor).map(|p| p.pon),
                visible_pons: stage.visible_pages().iter().map(|p| p.pon).collect(),
                cause: state.motion_cause,
            }))
        })
    };
    let signature = snapshot.clone();
    watch(
        &fx,
        // The reportable truth: the current pon and the visible pon set.
        // `cause` is left out: a cause flip alone is not a change.
        move || {
            Ok(match signature()? {
                None => "unplaced".to_string(),
                Some(s) => {
                    let mut pons = s.visible_pons.clone();
                    pons.sort();
                    let pons: Vec<String> = pons.iter().map(u32::to_string).collect();
                    let current = s.current_pon.map_or("-1".to_string(), |p| p.to_string());
                    format!("{current}|{}", pons.join(","))
                }
            })
        },
        |a: &String, b: &String| a == b,
        move |_, _| {
            let Some(s) = snapshot()? else { return Ok(()) };
            actions.report_page_state(PageStateReport {
                current_pon: s.current_pon.map(f64::from),
                visible_pons: s.visible_pons.iter().map(|&p| f64::from(p)).collect(),
                placed: true,
                cause: match s.cause {
                    MotionCause::User => PageStateCause::User,
                    MotionCause::Programmatic => PageStateCause::Programmatic,
                },
            });
            Ok(())
        },
    )
}
