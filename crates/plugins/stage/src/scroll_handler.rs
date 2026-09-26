//! `scroll-handler.ts`: pan-the-camera as an interaction handler.
//!
//! Live under the built-in `pan` tool (the `scroll` tag) and, with
//! `pan_fallback`, under every tool, where it then captures only page-gap
//! downs. It pans by the sample's viewport delta.

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_plugin_interaction::{InteractionCapability, InteractionHandler, PointerSample, Tool};

use crate::controller::StageCapability;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollHandlerOptions {
    /// Let a drag over a page gap pan whatever the active tool, with a grab
    /// cursor there. Default true.
    pub pan_fallback: Option<bool>,
}

struct ScrollHandler {
    stage: StageCapability,
    interaction: InteractionCapability,
    pan_fallback: bool,
    last: Cell<Point>,
}

impl ScrollHandler {
    fn is_pan_tool(&self) -> bool {
        self.interaction.get_active_tool().enables("scroll")
    }
}

impl InteractionHandler for ScrollHandler {
    fn id(&self) -> &str {
        "stage-scroll"
    }

    /// Below the page-aware handlers, which claim first.
    fn priority(&self) -> f64 {
        10.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("scroll") || self.pan_fallback
    }

    fn on_down(&self, s: &PointerSample) -> bool {
        // A non-pan tool pans only over a gap.
        if !self.is_pan_tool() && s.page.is_some() {
            return false;
        }
        self.last.set(s.viewport);
        self.interaction.claim_cursor("stage-grab", Some("grabbing"), 40.0);
        true
    }

    fn on_move(&self, s: &PointerSample) {
        let last = self.last.get();
        // Handler members are infallible (see epdf-plugin-interaction). A
        // pan only fails in paged flow over a document with no pages, where
        // the TypeScript's throw would reach the adapter instead.
        let _ = self.stage.pan_by(s.viewport.x - last.x, s.viewport.y - last.y);
        self.last.set(s.viewport);
    }

    fn on_up(&self, _s: &PointerSample) {
        self.interaction.claim_cursor("stage-grab", None, 0.0);
    }

    fn on_hover(&self, s: &PointerSample) {
        if self.pan_fallback && !self.is_pan_tool() {
            let cursor = if s.page.is_some() { None } else { Some("grab") };
            self.interaction.claim_cursor("stage-pan-fallback", cursor, 5.0);
        }
    }
}

/// `createScrollHandler(stage, interaction, options)`. The surface binding
/// registers it with the hub, scoped to `stage.get_lens_id()`.
pub fn create_scroll_handler(
    stage: StageCapability,
    interaction: InteractionCapability,
    options: ScrollHandlerOptions,
) -> Rc<dyn InteractionHandler> {
    Rc::new(ScrollHandler {
        stage,
        interaction,
        pan_fallback: options.pan_fallback.unwrap_or(true),
        last: Cell::new(Point { x: 0.0, y: 0.0 }),
    })
}
