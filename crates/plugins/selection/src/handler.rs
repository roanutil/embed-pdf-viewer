//! `handler.ts`: the text-selection pointer handler, live under tools that
//! enable `text-select` (the built-in `pointer` does, `pan` does not).
//!
//! - single click: deselect; record an anchor, select nothing until the
//!   pointer moves past the drag threshold
//! - single click and drag: caret selection from the anchor, across pages
//! - double click: the word; triple click: the visual line
//! - down off text: deselect and don't capture
//! - hover: the I-beam only over text
//!
//! Without `doc.text.select` the handler declines every down, so nothing it
//! calls afterwards can be refused; the `Result`s of the selection writes are
//! dropped for that reason.

use std::cell::{Cell, RefCell};

use epdf_geometry::{Point, js};
use epdf_kernel::geometry::PageRef;
use epdf_plugin_interaction::{
    Feedback, Gesture, InteractionCapability, InteractionHandler, PointerSample, Tool,
};

use crate::controller::SelectionCapability;

const CURSOR_TOKEN: &str = "selection-text";
/// Viewport px the pointer must move before a drag-select begins.
const DEFAULT_DRAG_THRESHOLD: f64 = 4.0;

/// `TextSelectHandlerOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextSelectHandlerOptions {
    pub drag_threshold: Option<f64>,
}

#[derive(Clone, Copy)]
struct Anchor {
    page: PageRef,
    point: Point,
    vx: f64,
    vy: f64,
}

/// `createTextSelectHandler(selection, interaction, feedback?, options)`.
pub struct TextSelectHandler {
    selection: SelectionCapability,
    interaction: InteractionCapability,
    feedback: Option<Feedback>,
    drag_threshold: f64,
    anchor: RefCell<Option<Anchor>>,
    dragging: Cell<bool>,
}

pub fn create_text_select_handler(
    selection: SelectionCapability,
    interaction: InteractionCapability,
    feedback: Option<Feedback>,
    options: TextSelectHandlerOptions,
) -> TextSelectHandler {
    TextSelectHandler {
        selection,
        interaction,
        feedback,
        drag_threshold: options.drag_threshold.unwrap_or(DEFAULT_DRAG_THRESHOLD),
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
        let Some(hit) = s.page else { return false }; // over a gap: let a lower handler try
        if !self.selection.can_select() {
            return false; // nothing here can engage: stay out of the way
        }
        let (page, point) = (hit.page, hit.point);
        let clicks = s.click_count.unwrap_or(1.0);
        // The pointer is down: until it lifts, every change is the user's.
        self.selection.begin_gesture();
        if clicks >= 3.0 {
            let _ = self.selection.select_line_at(page, point);
            return true;
        }
        if clicks == 2.0 {
            let selected = self.selection.select_word_at(page, point).unwrap_or(false);
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
        if !self.selection.is_over_text(page, point) {
            self.selection.end_gesture(); // empty space: nothing to drive, don't capture
            return false;
        }
        *self.anchor.borrow_mut() =
            Some(Anchor { page, point, vx: s.viewport.x, vy: s.viewport.y });
        true
    }

    fn on_move(&self, s: &PointerSample) {
        let Some(hit) = s.page else { return };
        let Some(anchor) = *self.anchor.borrow() else { return };
        if !self.dragging.get() {
            if js::hypot(s.viewport.x - anchor.vx, s.viewport.y - anchor.vy) < self.drag_threshold {
                return; // still a click
            }
            self.dragging.set(true);
            self.selection.begin_gesture_at(anchor.page, anchor.point);
        }
        let _ = self.selection.extend_to(hit.page, hit.point);
    }

    fn on_up(&self, _: &PointerSample) {
        *self.anchor.borrow_mut() = None;
        self.dragging.set(false);
        self.selection.end_gesture();
    }

    /// Aborted: drop the in-flight selection instead of committing a sliver;
    /// `end_gesture` after `clear` settles the gesture with nothing to commit.
    fn on_cancel(&self, _: &PointerSample) {
        *self.anchor.borrow_mut() = None;
        self.dragging.set(false);
        self.selection.clear();
        self.selection.end_gesture();
    }

    fn on_hover(&self, s: &PointerSample) {
        let Some(hit) = s.page else {
            self.interaction.claim_cursor(CURSOR_TOKEN, None, 0.0); // off the page
            return;
        };
        let _ = self.selection.ensure_loaded(hit.page);
        let over_text = self.selection.is_over_text(hit.page, hit.point);
        self.interaction.claim_cursor(CURSOR_TOKEN, over_text.then_some("text"), 10.0);
    }
}
