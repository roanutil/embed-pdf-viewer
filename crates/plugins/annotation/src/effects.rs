//! `effects.ts`: makes annotations reactive to the document event stream.
//! It kicks whole-document hydration once, hands remote `annotation.*`
//! events to the capability's hydration-aware delivery, re-hydrates on
//! `stream.desynced`, and reconciles page membership, form re-bakes and
//! redaction applies.

use std::rc::Rc;

use epdf_annotation_core::{Msg, update};
use epdf_kernel::{EffectCtx, Error};
use serde_json::Value;

use crate::types::{ANNOTATION, AnnotationAction, AnnotationState};
use crate::value::{f64_of, get, str_of};

fn pons(v: Option<&Value>) -> Vec<u32> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .map(|p| p as u32)
        .collect()
}

/// `registerAnnotationEffects(ctx)`.
pub fn register_annotation_effects(
    ctx: EffectCtx<AnnotationState, AnnotationAction>,
) -> Result<(), Error> {
    let Some(doc) = ctx.doc() else { return Ok(()) };

    let typed = ctx.ctx.clone();
    let apply = move |msg: Msg| {
        let m = typed.state().model.clone();
        let (next, _) = update(&m, &msg);
        let model = next.map_or(m, Rc::new);
        typed.dispatch(AnnotationAction::SetModel { model });
    };
    let host_ctx = ctx.ctx.clone();
    let host = move || host_ctx.get(&ANNOTATION);
    let model_ctx = ctx.ctx.clone();

    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
        let kind = str_of(event, "type").unwrap_or_default();
        if kind == "form.valueChanged" || kind == "form.effectsApplied" {
            let ids = get(event, "changedWidgets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|w| f64_of(w, "annotObjectNumber") > 0.0)
                .map(|w| {
                    format!(
                        "obj:{}",
                        epdf_geometry::js::number_to_string(f64_of(w, "annotObjectNumber"))
                    )
                })
                .collect();
            apply(Msg::BumpAp { ids });
            return;
        }
        if kind == "redaction.applied" {
            let mut affected: Vec<u32> = Vec::new();
            for r in get(event, "results").and_then(Value::as_array).into_iter().flatten() {
                if str_of(r, "status") == Some("applied") {
                    let pon = f64_of(r, "pageObjectNumber") as u32;
                    if !affected.contains(&pon) {
                        affected.push(pon);
                    }
                }
            }
            if let Ok(host) = host() {
                for pon in affected {
                    let _ = host.reload_page(pon);
                }
            }
            return;
        }
        if kind == "stream.desynced" {
            if let Ok(host) = host() {
                let _ = host.rehydrate();
            }
            return;
        }
        if kind == "pages.deleted" {
            let gone = pons(get(event, "pageObjectNumbers"));
            let m = model_ctx.state().model.clone();
            let ids: Vec<String> = m
                .order
                .iter()
                .filter(|id| m.by_id.get(*id).is_some_and(|a| gone.contains(&a.pon)))
                .cloned()
                .collect();
            if !ids.is_empty() {
                apply(Msg::Remove { ids });
            }
            return;
        }
        if kind == "pages.inserted" {
            if let Ok(host) = host() {
                for pon in pons(get(event, "insertedPageObjectNumbers")) {
                    let _ = host.reload_page(pon);
                }
            }
            return;
        }
        let remote = event
            .get("origin")
            .is_some_and(|o| o.get("kind").and_then(Value::as_str) == Some("remote"));
        if !remote {
            return;
        }
        if matches!(
            kind,
            "annotation.created" | "annotation.updated" | "annotation.moved" | "annotation.deleted"
        ) && let Ok(host) = host()
        {
            host.deliver_remote_annotation_event(event.clone());
        }
    }));
    ctx.cleanup_fn(move || unsubscribe.call());

    ctx.get(&ANNOTATION)?.ensure_hydrated();
    Ok(())
}
