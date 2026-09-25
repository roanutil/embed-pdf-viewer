//! `effects.ts`: makes page rasters reactive. The effect subscribes to the
//! document event stream and bumps the touched pages' ledger for each
//! confirmed pixel-changing fact, own or remote alike. The other door is the
//! capability's `invalidate`.

use std::rc::Rc;

use epdf_kernel::{EffectCtx, Error};
use serde_json::Value;

use crate::types::{InvalidateScope, RenderAction, RenderState};

fn pon_of(value: &Value) -> Option<u32> {
    value.get("pageObjectNumber").and_then(Value::as_u64).map(|p| p as u32)
}

fn pons_of(list: Option<&Value>) -> Vec<u32> {
    list.and_then(Value::as_array).into_iter().flatten().filter_map(pon_of).collect()
}

/// `annotatedPons`: which pages' annotated raster a confirmed event
/// repaints. Everything the engine emits today is appearance-scoped;
/// `pages.*` and metadata events repaint nothing.
pub fn annotated_pons(event: &Value, all_pons: impl FnOnce() -> Vec<u32>) -> Vec<u32> {
    match event.get("type").and_then(Value::as_str).unwrap_or_default() {
        "annotation.created" | "annotation.updated" | "annotation.deleted" | "annotation.moved" => {
            pon_of(event).into_iter().collect()
        }
        "form.valueChanged" | "form.effectsApplied" => pons_of(event.get("changedWidgets")),
        "form.fieldDeleted" => pons_of(event.get("removedWidgets")),
        "form.fieldCreated"
        | "form.fieldUpdated"
        | "form.widgetAttached"
        | "form.widgetDetached" => pons_of(event.get("field").and_then(|f| f.get("widgets"))),
        "form.imported" | "form.repaired" => all_pons(),
        _ => Vec::new(),
    }
}

/// The document's page object numbers, in registry order.
pub(crate) fn all_pons_of(ctx: &epdf_kernel::plugin::PluginContext) -> Vec<u32> {
    ctx.document().map(|meta| pons_of(Some(&meta.pages))).unwrap_or_default()
}

/// `registerRenderEffects`.
pub fn register_render_effects(ctx: EffectCtx<RenderState, RenderAction>) -> Result<(), Error> {
    let Some(doc) = ctx.doc() else { return Ok(()) };
    let live = ctx.ctx.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
        // Content-scope facts: an applied redaction destroyed page content.
        if event.get("type").and_then(Value::as_str) == Some("redaction.applied") {
            let pons: Vec<u32> = event
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|r| r.get("status").and_then(Value::as_str) == Some("applied"))
                .filter_map(pon_of)
                .collect();
            if !pons.is_empty() {
                live.dispatch(RenderAction::Invalidate { scope: InvalidateScope::Content, pons });
            }
            return;
        }
        let pons = annotated_pons(event, || all_pons_of(&live.ctx));
        if !pons.is_empty() {
            live.dispatch(RenderAction::Invalidate { scope: InvalidateScope::Annotations, pons });
        }
    }));
    ctx.cleanup_fn(move || unsubscribe.call());
    Ok(())
}
