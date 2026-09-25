//! `handler.ts`: the five pointer handlers the plugin registers with the
//! interaction hub. Gesture state lives in the handler; the capability runs
//! the pure core.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_annotation_core::{Phase as CorePhase, Point};
use epdf_geometry::js::hypot;
use epdf_kernel::rt::{self, TimerId};
use epdf_plugin_interaction::{
    InteractionCapability, InteractionHandler, PointerKind, PointerSample, Tool, sample_point_on,
};

use crate::capability::{AnnotationCapability, HoverAt};

const MARQUEE_DRAG_THRESHOLD_PX: f64 = 4.0;

fn is_poly_tool(subtype: &str) -> bool {
    subtype == "polygon" || subtype == "polyline"
}

fn is_callout_tool(subtype: &str) -> bool {
    subtype == "free-text-callout"
}

fn rotation_of(s: &PointerSample) -> Option<f64> {
    s.page.as_ref().and_then(|p| p.rotation).map(|r| r.0)
}

/// Click-to-place for every payload-carrying tool.
pub struct PlaceHandler {
    anno: AnnotationCapability,
}

/// `createPlaceHandler(anno)`.
pub fn create_place_handler(anno: AnnotationCapability) -> PlaceHandler {
    PlaceHandler { anno }
}

impl InteractionHandler for PlaceHandler {
    fn id(&self) -> &str {
        "annotation-place"
    }
    fn priority(&self) -> f64 {
        95.0
    }
    fn enabled_for(&self, t: &Tool) -> bool {
        t.enables("annotation-place") || t.enables("annotation-stamp")
    }
    fn on_down(&self, s: &PointerSample) -> bool {
        let Some(page) = &s.page else { return false };
        self.anno.place_at(page.pon, page.point, page.rotation.map(|r| r.0))
    }
}

/// The armed tool's footprint ghost. Never captures.
pub struct GhostHandler {
    anno: AnnotationCapability,
    interaction: InteractionCapability,
}

/// `createGhostHandler(anno, interaction)`.
pub fn create_ghost_handler(
    anno: AnnotationCapability,
    interaction: InteractionCapability,
) -> GhostHandler {
    GhostHandler { anno, interaction }
}

impl InteractionHandler for GhostHandler {
    fn id(&self) -> &str {
        "annotation-ghost"
    }
    fn priority(&self) -> f64 {
        1000.0
    }
    fn enabled_for(&self, _t: &Tool) -> bool {
        true
    }
    fn on_down(&self, _s: &PointerSample) -> bool {
        self.anno.clear_ghost();
        false
    }
    fn on_hover(&self, s: &PointerSample) {
        match &s.page {
            Some(page) => self.anno.ghost_hover_at(
                &self.interaction.active_tool_id(),
                page.pon,
                page.point,
                page.rotation.map(|r| r.0),
            ),
            None => self.anno.clear_ghost(),
        }
    }
}

#[derive(Clone, Copy)]
struct EditOrigin {
    pon: u32,
    point: Point,
    down_point: Point,
    touch: bool,
}

/// Ambient editing under `annotation-edit`.
pub struct EditHandler {
    anno: AnnotationCapability,
    interaction: InteractionCapability,
    origin: RefCell<Option<EditOrigin>>,
}

/// `createEditHandler(anno, interaction)`.
pub fn create_edit_handler(
    anno: AnnotationCapability,
    interaction: InteractionCapability,
) -> EditHandler {
    EditHandler { anno, interaction, origin: RefCell::new(None) }
}

impl InteractionHandler for EditHandler {
    fn id(&self) -> &str {
        "annotation-edit"
    }
    fn priority(&self) -> f64 {
        100.0
    }
    fn enabled_for(&self, t: &Tool) -> bool {
        t.enables("annotation-edit")
    }
    fn claims_touch(&self, s: &PointerSample) -> bool {
        let Some(page) = &s.page else { return false };
        self.anno.claims_touch_at(page.pon, page.point, page.scale, rotation_of(s), page.zoom)
    }
    fn on_down(&self, s: &PointerSample) -> bool {
        let Some(page) = &s.page else { return false };
        let touch = s.pointer_type == Some(PointerKind::Touch);
        let was_editing = self.anno.current_editing().is_some();
        if was_editing {
            self.anno.end_text_edit();
        }
        let rot = rotation_of(s);
        if self.anno.hit_kind(page.pon, page.point, page.scale, rot, page.zoom, Some(touch))
            == "empty"
        {
            if !s.modifiers.shift {
                self.anno.deselect();
            }
            return was_editing;
        }
        if s.click_count.unwrap_or(1.0) >= 2.0
            && self.anno.begin_text_edit_at(page.pon, page.point, page.scale, rot, page.zoom)
        {
            return true;
        }
        self.anno.edit_pointer(
            CorePhase::Down,
            page.pon,
            page.point,
            s.modifiers.shift,
            page.scale,
            rot,
            page.zoom,
            Some(touch),
        );
        *self.origin.borrow_mut() =
            Some(EditOrigin { pon: page.pon, point: page.point, down_point: page.point, touch });
        true
    }
    fn on_move(&self, s: &PointerSample) {
        let Some(origin) = *self.origin.borrow() else { return };
        let Some(point) = sample_point_on(s, origin.pon) else { return };
        if let Some(o) = self.origin.borrow_mut().as_mut() {
            o.point = point;
        }
        self.anno.edit_pointer(
            CorePhase::Move,
            origin.pon,
            point,
            s.modifiers.shift,
            s.page.and_then(|p| p.scale),
            rotation_of(s),
            s.page.and_then(|p| p.zoom),
            Some(origin.touch),
        );
    }
    fn on_up(&self, s: &PointerSample) {
        let Some(origin) = *self.origin.borrow() else { return };
        let point = sample_point_on(s, origin.pon).unwrap_or(origin.point);
        self.anno.edit_pointer(
            CorePhase::Up,
            origin.pon,
            point,
            false,
            None,
            None,
            None,
            Some(origin.touch),
        );
        *self.origin.borrow_mut() = None;
    }
    fn on_cancel(&self, _s: &PointerSample) {
        let Some(origin) = self.origin.borrow_mut().take() else { return };
        self.anno.edit_pointer(
            CorePhase::Move,
            origin.pon,
            origin.down_point,
            false,
            None,
            None,
            None,
            Some(origin.touch),
        );
        self.anno.edit_pointer(
            CorePhase::Up,
            origin.pon,
            origin.down_point,
            false,
            None,
            None,
            None,
            Some(origin.touch),
        );
    }
    fn on_hover(&self, s: &PointerSample) {
        let cursor = s.page.as_ref().and_then(|page| {
            self.anno.cursor_at(page.pon, page.point, page.scale, rotation_of(s), page.zoom)
        });
        self.interaction.set_cursor("annotation", cursor.as_deref(), 20.0);
        self.anno.hover_at(s.page.as_ref().map(|page| HoverAt {
            pon: page.pon,
            point: page.point,
            scale: page.scale,
            rotation: rotation_of(s),
            zoom: page.zoom,
        }));
    }
}

#[derive(Clone, Copy)]
struct MarqueeAnchor {
    pon: u32,
    point: Point,
    vx: f64,
    vy: f64,
    shift: bool,
}

#[derive(Clone, Copy, Default)]
struct MarqueeView {
    scale: Option<f64>,
    rotation: Option<f64>,
    zoom: Option<f64>,
}

/// Empty-page drag selection.
pub struct MarqueeHandler {
    anno: AnnotationCapability,
    anchor: RefCell<Option<MarqueeAnchor>>,
    last: RefCell<Option<(u32, Point)>>,
    dragging: Cell<bool>,
    view: Cell<MarqueeView>,
}

/// `createMarqueeHandler(anno)`.
pub fn create_marquee_handler(anno: AnnotationCapability) -> MarqueeHandler {
    MarqueeHandler {
        anno,
        anchor: RefCell::new(None),
        last: RefCell::new(None),
        dragging: Cell::new(false),
        view: Cell::new(MarqueeView::default()),
    }
}

impl InteractionHandler for MarqueeHandler {
    fn id(&self) -> &str {
        "annotation-marquee"
    }
    fn priority(&self) -> f64 {
        50.0
    }
    fn enabled_for(&self, t: &Tool) -> bool {
        t.enables("annotation-marquee")
    }
    fn on_down(&self, s: &PointerSample) -> bool {
        let Some(page) = &s.page else { return false };
        *self.anchor.borrow_mut() = Some(MarqueeAnchor {
            pon: page.pon,
            point: page.point,
            vx: s.viewport.x,
            vy: s.viewport.y,
            shift: s.modifiers.shift,
        });
        *self.last.borrow_mut() = Some((page.pon, page.point));
        self.view.set(MarqueeView { scale: page.scale, rotation: rotation_of(s), zoom: page.zoom });
        self.dragging.set(false);
        true
    }
    fn on_move(&self, s: &PointerSample) {
        let Some(anchor) = *self.anchor.borrow() else { return };
        let Some(point) = sample_point_on(s, anchor.pon) else { return };
        *self.last.borrow_mut() = Some((anchor.pon, point));
        let view = self.view.get();
        if !self.dragging.get() {
            if hypot(s.viewport.x - anchor.vx, s.viewport.y - anchor.vy) < MARQUEE_DRAG_THRESHOLD_PX
            {
                return;
            }
            self.dragging.set(true);
            self.anno.marquee_pointer(
                CorePhase::Down,
                anchor.pon,
                anchor.point,
                anchor.shift,
                view.scale,
                view.rotation,
                view.zoom,
            );
        }
        self.anno.marquee_pointer(
            CorePhase::Move,
            anchor.pon,
            point,
            anchor.shift,
            view.scale,
            view.rotation,
            view.zoom,
        );
    }
    fn on_up(&self, _s: &PointerSample) {
        let anchor = *self.anchor.borrow();
        let last = *self.last.borrow();
        if self.dragging.get()
            && let (Some(anchor), Some((_, point))) = (anchor, last)
        {
            let view = self.view.get();
            self.anno.marquee_pointer(
                CorePhase::Up,
                anchor.pon,
                point,
                anchor.shift,
                view.scale,
                view.rotation,
                view.zoom,
            );
        }
        *self.anchor.borrow_mut() = None;
        *self.last.borrow_mut() = None;
        self.dragging.set(false);
    }
    fn on_cancel(&self, _s: &PointerSample) {
        *self.anchor.borrow_mut() = None;
        *self.last.borrow_mut() = None;
        self.dragging.set(false);
        self.anno.cancel_creation_draft();
    }
}

struct PendingInk {
    tool: String,
    pon: u32,
    timer: TimerId,
}

#[derive(Default)]
struct DrawState {
    drawing_poly: Cell<bool>,
    drawing_callout: Cell<bool>,
    origin: RefCell<Option<(u32, Point)>>,
    pending_ink: RefCell<Option<PendingInk>>,
}

/// Drawing under `annotation-draw`.
pub struct DrawHandler {
    anno: AnnotationCapability,
    interaction: InteractionCapability,
    state: Rc<DrawState>,
}

fn flush_pending_ink(state: &DrawState, anno: &AnnotationCapability) {
    let Some(pending) = state.pending_ink.borrow_mut().take() else { return };
    rt::clear_timer(pending.timer);
    anno.finish_ink_draft();
}

/// `createDrawHandler(anno, interaction)`. Registers its own tool-change
/// callback, which flushes a buffered ink group and resets the gesture.
pub fn create_draw_handler(
    anno: AnnotationCapability,
    interaction: InteractionCapability,
) -> DrawHandler {
    let state = Rc::new(DrawState::default());
    let (s, a) = (Rc::downgrade(&state), anno.clone());
    interaction.on_tool_change(move || {
        let Some(state) = s.upgrade() else { return };
        flush_pending_ink(&state, &a);
        state.drawing_poly.set(false);
        state.drawing_callout.set(false);
        *state.origin.borrow_mut() = None;
    });
    DrawHandler { anno, interaction, state }
}

impl DrawHandler {
    fn tool_id(&self) -> String {
        self.interaction.active_tool_id()
    }
}

impl InteractionHandler for DrawHandler {
    fn id(&self) -> &str {
        "annotation-draw"
    }
    fn priority(&self) -> f64 {
        55.0
    }
    fn enabled_for(&self, t: &Tool) -> bool {
        t.enables("annotation-draw")
    }
    fn on_down(&self, s: &PointerSample) -> bool {
        let Some(page) = &s.page else { return false };
        let tool = self.tool_id();
        let st = self.anno.tool_subtype(&tool);
        if st == "ink" {
            let pending = self
                .state
                .pending_ink
                .borrow()
                .as_ref()
                .map(|p| (p.tool == tool && p.pon == page.pon, p.timer));
            match pending {
                Some((true, timer)) => {
                    rt::clear_timer(timer);
                    *self.state.pending_ink.borrow_mut() = None;
                }
                Some((false, _)) => flush_pending_ink(&self.state, &self.anno),
                None => {}
            }
        }
        *self.state.origin.borrow_mut() = Some((page.pon, page.point));
        if is_poly_tool(&st) {
            let finish = s.click_count.unwrap_or(1.0) >= 2.0;
            self.anno.create_pointer(&tool, CorePhase::Down, page.pon, page.point, finish, None);
            self.state.drawing_poly.set(!finish);
            return true;
        }
        self.state.drawing_poly.set(false);
        if is_callout_tool(&st) {
            self.state.drawing_callout.set(true);
        }
        self.anno.create_pointer(
            &tool,
            CorePhase::Down,
            page.pon,
            page.point,
            false,
            rotation_of(s),
        );
        true
    }
    fn on_move(&self, s: &PointerSample) {
        let tool = self.tool_id();
        let st = self.anno.tool_subtype(&tool);
        let Some((pon, _)) = *self.state.origin.borrow() else { return };
        if is_poly_tool(&st) && !self.state.drawing_poly.get() {
            return;
        }
        let Some(point) = sample_point_on(s, pon) else { return };
        *self.state.origin.borrow_mut() = Some((pon, point));
        self.anno.create_pointer(&tool, CorePhase::Move, pon, point, false, None);
    }
    fn on_up(&self, s: &PointerSample) {
        let tool = self.tool_id();
        let st = self.anno.tool_subtype(&tool);
        let origin = *self.state.origin.borrow();
        if let Some((pon, last)) = origin
            && !is_poly_tool(&st)
        {
            let point = sample_point_on(s, pon).unwrap_or(last);
            self.anno.create_pointer(&tool, CorePhase::Up, pon, point, false, None);
            if st == "ink" {
                let group_ms = self
                    .anno
                    .tool(&tool)
                    .and_then(|t| t.ink)
                    .and_then(|i| i.group_strokes_ms)
                    .unwrap_or(0.0);
                if group_ms > 0.0 {
                    let (state, anno) = (Rc::downgrade(&self.state), self.anno.clone());
                    let timer = rt::set_timeout(group_ms, move || {
                        if let Some(state) = state.upgrade() {
                            *state.pending_ink.borrow_mut() = None;
                        }
                        anno.finish_ink_draft();
                    });
                    *self.state.pending_ink.borrow_mut() = Some(PendingInk { tool, pon, timer });
                }
            }
        }
        *self.state.origin.borrow_mut() = None;
    }
    fn on_cancel(&self, _s: &PointerSample) {
        if let Some(pending) = self.state.pending_ink.borrow_mut().take() {
            rt::clear_timer(pending.timer);
        }
        self.state.drawing_poly.set(false);
        self.state.drawing_callout.set(false);
        *self.state.origin.borrow_mut() = None;
        self.anno.cancel_creation_draft();
    }
    fn on_hover(&self, s: &PointerSample) {
        let tool = self.tool_id();
        let st = self.anno.tool_subtype(&tool);
        let Some(page) = &s.page else { return };
        if (self.state.drawing_poly.get() && is_poly_tool(&st))
            || (self.state.drawing_callout.get() && is_callout_tool(&st))
        {
            self.anno.create_pointer(&tool, CorePhase::Move, page.pon, page.point, false, None);
        }
    }
}
