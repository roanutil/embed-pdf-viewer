//! `connect.ts`: the selection plugin's wiring to its siblings, run from
//! the controller's `connect`.

use std::rc::Rc;

use epdf_kernel::{Controller, Error};
use epdf_plugin_interaction::{FEEDBACK, INTERACTION, RegisterHandlerOptions};
use serde_json::Value;

use crate::controller::SelectionCapability;
use crate::handler::{TextSelectHandlerOptions, create_text_select_handler};
use crate::model::SelectionState;

/// `connectSelection(ctx, selection)`: register the text-select pointer
/// handler with the interaction hub, which owns the pointer stream and
/// arbitration. Platform feedback (haptics) is optional. The handler reads
/// the drag threshold from the settings when a press would become a drag,
/// so a settings change applies to the next gesture.
pub(crate) fn connect_selection(
    cx: &Controller<SelectionState, Value>,
    selection: SelectionCapability,
) -> Result<(), Error> {
    let interaction = cx.get(&INTERACTION)?;
    let feedback = cx.try_get(&FEEDBACK)?;
    let threshold = selection.clone();
    let unregister = interaction.register_handler(
        Rc::new(create_text_select_handler(
            selection,
            interaction.clone(),
            feedback,
            TextSelectHandlerOptions {
                drag_threshold: Some(Rc::new(move || threshold.drag_threshold())),
            },
        )),
        RegisterHandlerOptions::default(),
    );
    cx.defer(Box::new(move || {
        unregister.call();
        Ok(None)
    }));
    Ok(())
}
