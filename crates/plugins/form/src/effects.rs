//! `effects.ts`: keeps the form reactive beyond this session's own writes.
//! Remote `form.*` events and structural events (which can move or add
//! widgets, so they also drop the cached geometry) re-read the snapshot; so
//! does `stream.desynced`, whose gap may have swallowed form events.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::{EffectCtx, Error};
use serde_json::Value;

use crate::core::model::{Msg, update};
use crate::promise::await_engine;
use crate::types::{FormAction, FormState};
use crate::value::{get, str_of};

const STRUCTURAL: &[&str] = &[
    "form.fieldCreated",
    "form.fieldUpdated",
    "form.fieldDeleted",
    "form.widgetAttached",
    "form.widgetDetached",
    "form.repaired",
];

/// `registerFormEffects(ctx)`.
pub fn register_form_effects(ctx: EffectCtx<FormState, FormAction>) -> Result<(), Error> {
    let Some(doc) = ctx.doc() else { return Ok(()) };
    let refreshing = Rc::new(Cell::new(false));
    let refresh = {
        let (ctx, doc, refreshing) = (ctx.ctx.clone(), doc.clone(), refreshing.clone());
        move || {
            let (ctx, doc, refreshing) = (ctx.clone(), doc.clone(), refreshing.clone());
            Promise::<()>::spawn(async move {
                if refreshing.get() {
                    return Ok(());
                }
                refreshing.set(true);
                let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await;
                let outcome = snapshot.map(|snapshot| {
                    let model = update(&ctx.state().model, &Msg::Snapshot { snapshot });
                    ctx.dispatch(FormAction::SetModel { model });
                });
                refreshing.set(false);
                outcome
            })
            .ignore();
        }
    };
    let clear = ctx.ctx.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
        let kind = str_of(event, "type").unwrap_or_default();
        if kind == "stream.desynced" {
            refresh();
            return;
        }
        let Some(origin) = get(event, "origin").filter(|_| kind.starts_with("form.")) else {
            return;
        };
        let structural = STRUCTURAL.contains(&kind);
        // Own non-structural writes already landed through the capability.
        if str_of(origin, "kind") != Some("remote") && !structural {
            return;
        }
        if structural {
            let model = update(&clear.state().model, &Msg::ClearGeom { page_object_number: None });
            clear.dispatch(FormAction::SetModel { model });
        }
        refresh();
    }));
    ctx.cleanup_fn(move || unsubscribe.call());
    Ok(())
}
