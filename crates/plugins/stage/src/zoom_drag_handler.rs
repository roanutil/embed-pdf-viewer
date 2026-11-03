//! `zoom-drag-handler.ts`: the `zoom` tool as an interaction handler.
//!
//! Live under the tool's `zoom-box` tag, over the pages and the space
//! between them. It forwards the pointer in viewport px to the stage's zoom
//! drag, which decides what a box or a click zooms to. A sample from
//! another surface (a page view stamps no lens) is in another element's px,
//! so the handler takes only its own lens's presses.
//!
//! Cursors: the tool's own `zoom-in`, and `zoom-out` while Alt (Option) is
//! held, which turns a click into a step out.

use std::rc::Rc;

use epdf_plugin_interaction::{InteractionCapability, InteractionHandler, PointerSample, Tool};

use crate::controller::StageCapability;

struct ZoomDragHandler {
    stage: StageCapability,
    interaction: InteractionCapability,
}

impl InteractionHandler for ZoomDragHandler {
    fn id(&self) -> &str {
        "stage-zoom-drag"
    }

    /// The stage's own band, as the scroll handler.
    fn priority(&self) -> f64 {
        10.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("zoom-box")
    }

    fn on_down(&self, s: &PointerSample) -> bool {
        if s.source.as_deref() != Some(self.stage.get_lens_id().as_str()) {
            return false;
        }
        self.stage.begin_zoom_drag(s.viewport);
        true
    }

    fn on_move(&self, s: &PointerSample) {
        self.stage.move_zoom_drag(s.viewport);
    }

    fn on_up(&self, s: &PointerSample) {
        // Handler members are infallible (see epdf-plugin-interaction). A
        // zoom fails only where the camera write does, in paged flow over a
        // document with no pages, where the TypeScript's throw would reach
        // the adapter instead.
        let _ = self
            .stage
            .end_zoom_drag(Some(crate::EndZoomDragOptions { zoom_out: Some(s.modifiers.alt) }));
    }

    fn on_cancel(&self, _s: &PointerSample) {
        self.stage.cancel_zoom_drag();
    }

    fn on_hover(&self, s: &PointerSample) {
        let cursor = if s.modifiers.alt { Some("zoom-out") } else { None };
        self.interaction.claim_cursor("stage-zoom-out", cursor, 0.0);
    }
}

/// `createZoomDragHandler(stage, interaction)`. The surface binding
/// registers it with the hub, scoped to `stage.get_lens_id()`.
pub fn create_zoom_drag_handler(
    stage: StageCapability,
    interaction: InteractionCapability,
) -> Rc<dyn InteractionHandler> {
    Rc::new(ZoomDragHandler { stage, interaction })
}
