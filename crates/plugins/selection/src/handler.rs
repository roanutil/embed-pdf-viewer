//! `handler.ts`: the text-selection pointer handler, live under tools that
//! enable `text-select` (the built-in `pointer` does, `pan` does not).
//!
//! - single click: deselect; record an anchor, select nothing until the
//!   pointer moves past the drag threshold
//! - single click and drag: caret selection from the anchor, across pages
//! - double click: the word; triple click: the visual line
//! - down off text: deselect and don't capture
//! - hover: the I-beam only over text

use std::cell::{Cell, RefCell};

use epdf_geometry::{Point, js};
use epdf_plugin_interaction::{
    Feedback, Gesture, InteractionCapability, InteractionHandler, PointerSample, Tool,
};

use crate::capability::SelectionCapability;
use crate::types::PageObjectNumber;

const CURSOR_TOKEN: &str = "selection-text";
/// Viewport px the pointer must move before a drag-select begins.
const DRAG_THRESHOLD_PX: f64 = 4.0;

#[derive(Clone, Copy)]
struct Anchor {
    pon: PageObjectNumber,
    point: Point,
    vx: f64,
    vy: f64,
}

/// `createTextSelectHandler(selection, interaction, feedback?)`.
pub struct TextSelectHandler {
    selection: SelectionCapability,
    interaction: InteractionCapability,
    feedback: Option<Feedback>,
    anchor: RefCell<Option<Anchor>>,
    dragging: Cell<bool>,
}

pub fn create_text_select_handler(
    selection: SelectionCapability,
    interaction: InteractionCapability,
    feedback: Option<Feedback>,
) -> TextSelectHandler {
    TextSelectHandler {
        selection,
        interaction,
        feedback,
        anchor: RefCell::new(None),
        dragging: Cell::new(false),
    }
}

impl InteractionHandler for TextSelectHandler {
    fn id(&self) -> &str {
        "text-select"
    }

    fn priority(&self) -> f64 {
        60.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("text-select")
    }

    fn on_down(&self, s: &PointerSample) -> bool {
        *self.anchor.borrow_mut() = None;
        self.dragging.set(false);
        let Some(page) = s.page else { return false }; // over a gap: let a lower handler try
        let clicks = s.click_count.unwrap_or(1.0);
        if clicks >= 3.0 {
            self.selection.select_line_at(page.pon, page.point);
            return true;
        }
        if clicks == 2.0 {
            let selected = self.selection.select_word_at(page.pon, page.point);
            // The platform's selection tick: only when a long-press engaged a word.
            if selected
                && s.gesture == Some(Gesture::LongPress)
                && let Some(feedback) = &self.feedback
            {
                feedback.selection();
            }
            return true;
        }
        // Single click: clear, then anchor only over text; select nothing yet.
        self.selection.clear();
        if !self.selection.is_over_text(page.pon, page.point) {
            return false; // empty space: don't capture
        }
        *self.anchor.borrow_mut() =
            Some(Anchor { pon: page.pon, point: page.point, vx: s.viewport.x, vy: s.viewport.y });
        true
    }

    fn on_move(&self, s: &PointerSample) {
        let Some(page) = s.page else { return };
        let Some(anchor) = *self.anchor.borrow() else { return };
        if !self.dragging.get() {
            if js::hypot(s.viewport.x - anchor.vx, s.viewport.y - anchor.vy) < DRAG_THRESHOLD_PX {
                return; // still a click
            }
            self.dragging.set(true);
            self.selection.begin_at(anchor.pon, anchor.point);
        }
        self.selection.extend_to(page.pon, page.point);
    }

    fn on_up(&self, _: &PointerSample) {
        *self.anchor.borrow_mut() = None;
        self.dragging.set(false);
        self.selection.end();
    }

    /// Aborted: drop the in-flight selection instead of committing a sliver;
    /// `end` after `clear` settles `selecting` with nothing to commit.
    fn on_cancel(&self, _: &PointerSample) {
        *self.anchor.borrow_mut() = None;
        self.dragging.set(false);
        self.selection.clear();
        self.selection.end();
    }

    fn on_hover(&self, s: &PointerSample) {
        let Some(page) = s.page else {
            self.interaction.set_cursor(CURSOR_TOKEN, None, 0.0); // off the page
            return;
        };
        self.selection.ensure_page(page.pon);
        let over_text = self.selection.is_over_text(page.pon, page.point);
        self.interaction.set_cursor(CURSOR_TOKEN, over_text.then_some("text"), 10.0);
    }
}
