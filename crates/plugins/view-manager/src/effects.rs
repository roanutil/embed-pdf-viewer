//! `effects.ts`: keep panes consistent with the document registry. Whenever
//! the open order changes, reconcile: new documents land in the focused
//! pane (a default pane is created on first open), closed ones are dropped.

use epdf_kernel::{DOCUMENTS, EffectCtx, Error};

use crate::types::{ViewManagerAction, ViewManagerState};

pub fn register_view_manager_effects(
    ctx: EffectCtx<ViewManagerState, ViewManagerAction>,
) -> Result<(), Error> {
    let documents = ctx.get(&DOCUMENTS)?;
    let select = documents.clone();
    let dispatch = ctx.ctx.clone();
    // The watch compares `order().join('|')`: two orders that join to the
    // same string are the same order to it.
    ctx.watch(
        move || select.order().join("|"),
        |a, b| a == b,
        move |_, _| {
            let prefer_view_id = dispatch.state().focused_view_id.clone();
            dispatch
                .dispatch(ViewManagerAction::Reconcile { open: documents.order(), prefer_view_id });
            Ok(())
        },
    );
    Ok(())
}
