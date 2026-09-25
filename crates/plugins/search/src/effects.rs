//! `effects.ts`: keeps results honest across document mutations. Every
//! document event can change what is findable and invalidates search
//! cursors, so the reaction is always to re-run the current query from
//! scratch, coalesced: a burst of events triggers one rescan.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::rt::{self, TimerId};
use epdf_kernel::{EffectCtx, Error};
use serde_json::Value;

use crate::types::{SEARCH, SearchAction, SearchState, SearchStatus};

const RERUN_DELAY_MS: f64 = 250.0;

pub fn register_search_effects(fx: EffectCtx<SearchState, SearchAction>) -> Result<(), Error> {
    let Some(doc) = fx.doc() else { return Ok(()) };

    let timer: Rc<Cell<Option<TimerId>>> = Rc::default();
    let ctx = fx.ctx.clone();
    let armed = timer.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |_event: &Value| {
        if ctx.try_state().is_none_or(|s| s.status == SearchStatus::Idle) {
            return;
        }
        if let Some(id) = armed.take() {
            rt::clear_timer(id);
        }
        let (fired, ctx) = (armed.clone(), ctx.clone());
        armed.set(Some(rt::set_timeout(RERUN_DELAY_MS, move || {
            fired.set(None);
            if let Ok(search) = ctx.get(&SEARCH) {
                search.rerun();
            }
        })));
    }));

    fx.cleanup_fn(move || {
        if let Some(id) = timer.take() {
            rt::clear_timer(id);
        }
        unsubscribe.call();
    });
    Ok(())
}
