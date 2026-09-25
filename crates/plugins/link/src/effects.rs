//! `effects.ts`: keeps the engine-backed page cache honest in viewer-only
//! deployments. Any annotation mutation on a cached page, own or remote,
//! refetches that page's links; the cache has no other refresh path. With
//! the annotation plugin present its folded model is the source and this
//! effect never fetches.

use std::rc::Rc;

use epdf_kernel::{Ctx, EffectCtx, Error};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_annotation::value::{get, str_of};
use serde_json::Value;

use crate::source::{js_string, load_links_page};
use crate::types::{LinkAction, LinkState};

/// `refetch(pon)`. `pon in pages` compares property keys, so the event's
/// value is spelled as a key first.
fn refetch(ctx: &Ctx<LinkState, LinkAction>, pon: &Value) {
    if !matches!(ctx.try_get(&ANNOTATION), Ok(None)) {
        return; // the annotation model owns the data
    }
    let key = js_string(Some(pon));
    let cached = ctx.state().pages.keys().find(|p| p.to_string() == key).copied();
    if let Some(pon) = cached {
        // A page the engine no longer has throws out of the listener in the
        // TypeScript, which drops the refetch the same way.
        let _ = load_links_page(ctx, pon);
    }
}

/// `event[member].pageObjectNumber`; `None` where reading it throws.
fn pon_of(member: Option<&Value>) -> Option<&Value> {
    let member = member.filter(|m| !m.is_null())?;
    Some(get(member, "pageObjectNumber").unwrap_or(&Value::Null))
}

fn on_event(ctx: &Ctx<LinkState, LinkAction>, event: &Value) {
    let pon = match str_of(event, "type") {
        Some("annotation.created") => pon_of(get(event, "created")),
        Some("annotation.updated") => pon_of(get(event, "updated")),
        Some("annotation.moved") => {
            let Some(moved) = get(event, "moved").and_then(Value::as_array) else { return };
            match moved.first() {
                Some(first) => pon_of(Some(first)),
                None => return,
            }
        }
        Some("annotation.deleted") => Some(get(event, "pageObjectNumber").unwrap_or(&Value::Null)),
        _ => return,
    };
    if let Some(pon) = pon {
        refetch(ctx, pon);
    }
}

/// `registerLinkEffects(ctx)`: one document-event subscription, released
/// with the document.
pub fn register_link_effects(fx: EffectCtx<LinkState, LinkAction>) -> Result<(), Error> {
    let Some(doc) = fx.doc() else { return Ok(()) };
    let ctx = fx.ctx.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| on_event(&ctx, event)));
    fx.cleanup_fn(move || unsubscribe.call());
    Ok(())
}
