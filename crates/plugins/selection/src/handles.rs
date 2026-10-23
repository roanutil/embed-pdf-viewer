//! `handles.ts`: the pure half of selection handles. A handle is the
//! boundary glyph's leading (or trailing) edge, projected by the hosting
//! view; the drag session speaks the selection's own gesture verbs
//! (`begin_gesture_at`, `extend_to`, `end_gesture`), so a handle drag is a
//! selection gesture, re-anchored.

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::{Point, Quad, QuadSide, js, quad_edge};
use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};

use crate::contract::SelectionSnapshot;
use crate::controller::SelectionCapability;

/// A page under an overlay point, and the point on it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PagePoint {
    #[serde(rename = "ref")]
    pub page: PageRef,
    pub point: Point,
}

/// What handle geometry and drags need from the hosting view.
pub trait SelectionHandleView {
    /// Page point to overlay px. Must be point-exact; a box projector loses
    /// orientation.
    fn to_overlay(&self, page: PageRef, point: Point) -> Option<Point>;
    /// Overlay px to the page under it, `None` over a gap.
    fn page_at(&self, overlay: Point) -> Option<PagePoint>;
    /// Overlay px to a specific page's page space, unclamped.
    fn point_on_page(&self, page: PageRef, overlay: Point) -> Option<Point>;
}

/// The stage's projection, as the handles use it (`SelectionHandleStage`).
/// The stage capability satisfies it; this crate stays stage-free.
pub trait SelectionHandleStage {
    fn page_to_viewport(&self, page: PageRef, point: Point) -> Option<Point>;
    fn get_page_at(&self, point: Point) -> Option<PagePoint>;
    fn viewport_to_page(&self, page: PageRef, point: Point) -> Option<Point>;
}

/// `selectionHandleViewOf(stage)`: point-exact projection in, page lookup out.
pub struct StageHandleView<S>(pub S);

impl<S: SelectionHandleStage> SelectionHandleView for StageHandleView<S> {
    fn to_overlay(&self, page: PageRef, point: Point) -> Option<Point> {
        self.0.page_to_viewport(page, point)
    }
    fn page_at(&self, overlay: Point) -> Option<PagePoint> {
        self.0.get_page_at(overlay)
    }
    fn point_on_page(&self, page: PageRef, overlay: Point) -> Option<Point> {
        self.0.viewport_to_page(page, overlay)
    }
}

/// `selectionHandleViewOf(stage)`.
pub fn selection_handle_view_of<S: SelectionHandleStage>(stage: S) -> StageHandleView<S> {
    StageHandleView(stage)
}

/// One selection boundary as the handle needs it (a `SelectionEndpoint` slice).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionHandleEndpoint {
    pub page: PageRef,
    /// The boundary glyph's own oriented cell, in page space.
    pub glyph_quad: Quad,
    /// Reading direction of its segment (1 is the frame's +x): which side
    /// of the cell is the selection's leading edge.
    pub advance: i32,
}

/// Both ends of a selection, as the handles need them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionHandleEndpoints {
    pub start: SelectionHandleEndpoint,
    pub end: SelectionHandleEndpoint,
}

/// `selectionHandleEndpointsOf(snapshot)`: the two ends, or `None` while
/// nothing is selected.
pub fn selection_handle_endpoints_of(
    snapshot: &SelectionSnapshot,
) -> Option<SelectionHandleEndpoints> {
    let (start, end) = (snapshot.start.as_ref()?, snapshot.end.as_ref()?);
    Some(SelectionHandleEndpoints {
        start: SelectionHandleEndpoint {
            page: start.page,
            glyph_quad: start.glyph_quad,
            advance: start.advance,
        },
        end: SelectionHandleEndpoint {
            page: end.page,
            glyph_quad: end.glyph_quad,
            advance: end.advance,
        },
    })
}

/// The gesture verbs a handle drag drives.
pub trait SelectionHandleTarget {
    fn begin_gesture_at(&self, page: PageRef, point: Point) -> bool;
    fn extend_to(&self, page: PageRef, point: Point);
    fn end_gesture(&self);
}

/// The capability satisfies the target, as the TypeScript's structural type
/// does. An `extend_to` refused for want of `doc.text.select` is dropped:
/// `begin_gesture_at` already answered false without it, so a drag never
/// gets that far.
impl SelectionHandleTarget for SelectionCapability {
    fn begin_gesture_at(&self, page: PageRef, point: Point) -> bool {
        SelectionCapability::begin_gesture_at(self, page, point)
    }
    fn extend_to(&self, page: PageRef, point: Point) {
        let _ = SelectionCapability::extend_to(self, page, point);
    }
    fn end_gesture(&self) {
        SelectionCapability::end_gesture(self);
    }
}

/// px: the circle.
pub const HANDLE_HEAD: f64 = 12.0;
/// px: the caret bar.
pub const HANDLE_BAR: f64 = 2.0;
/// px: grab padding.
pub const HANDLE_PAD: f64 = 14.0;

/// Tilts within about 0.05° of upright render untransformed.
const HANDLE_ROT_EPSILON: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandleBar {
    pub from: Point,
    pub to: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionHandleGeom {
    /// The projected edge, ascent corner first, overlay px.
    pub bar: HandleBar,
    pub length: f64,
    /// Degrees clockwise; 0 when the text is upright on screen.
    pub rotation: f64,
    pub upright: bool,
    /// Centre of the grab head, overlay px.
    pub head: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandleRole {
    Start,
    End,
}

fn midpoint(a: Point, b: Point) -> Point {
    Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 }
}

/// The handle's geometry for one endpoint, or `None` when the page isn't
/// laid out or the cell is degenerate.
pub fn selection_handle_geom(
    view: &dyn SelectionHandleView,
    ep: &SelectionHandleEndpoint,
    role: HandleRole,
) -> Option<SelectionHandleGeom> {
    let leading = match role {
        HandleRole::Start => ep.advance > 0,
        HandleRole::End => ep.advance < 0,
    };
    let [upper_page, lower_page] =
        quad_edge(ep.glyph_quad, if leading { QuadSide::Left } else { QuadSide::Right });
    let upper = view.to_overlay(ep.page, upper_page);
    let lower = view.to_overlay(ep.page, lower_page);
    let (upper, lower) = (upper?, lower?);
    let dx = lower.x - upper.x;
    let dy = lower.y - upper.y;
    let length = js::hypot(dx, dy);
    if length == 0.0 {
        return None;
    }
    let rotation = (dy.atan2(dx) * 180.0) / std::f64::consts::PI - 90.0;
    let upright = rotation.abs() < HANDLE_ROT_EPSILON;
    let ux = dx / length;
    let uy = dy / length;
    let head = match role {
        HandleRole::Start => {
            Point { x: upper.x - (ux * HANDLE_HEAD) / 2.0, y: upper.y - (uy * HANDLE_HEAD) / 2.0 }
        }
        HandleRole::End => {
            Point { x: lower.x + (ux * HANDLE_HEAD) / 2.0, y: lower.y + (uy * HANDLE_HEAD) / 2.0 }
        }
    };
    Some(SelectionHandleGeom {
        bar: HandleBar { from: upper, to: lower },
        length,
        rotation,
        upright,
        head,
    })
}

/// One armed handle drag: extends from the opposite endpoint's cell centre
/// toward the pointer; over a gap it projects onto the last page hit.
pub struct SelectionHandleDragSession {
    selection: Rc<dyn SelectionHandleTarget>,
    view: Rc<dyn SelectionHandleView>,
    opposite_page: PageRef,
    anchor_point: Point,
    begun: Cell<bool>,
    last_page: Cell<PageRef>,
}

/// `createSelectionHandleDrag(selection, view, opposite, draggedPage)`.
pub fn create_selection_handle_drag(
    selection: Rc<dyn SelectionHandleTarget>,
    view: Rc<dyn SelectionHandleView>,
    opposite: &SelectionHandleEndpoint,
    dragged_page: PageRef,
) -> SelectionHandleDragSession {
    let q = opposite.glyph_quad;
    let anchor_point =
        midpoint(midpoint(q.upper_left, q.lower_right), midpoint(q.upper_right, q.lower_left));
    SelectionHandleDragSession {
        selection,
        view,
        opposite_page: opposite.page,
        anchor_point,
        begun: Cell::new(false),
        last_page: Cell::new(dragged_page),
    }
}

impl SelectionHandleDragSession {
    /// `move(overlay)`: feed the pointer's current overlay position.
    pub fn move_to(&self, overlay: Point) {
        if !self.begun.get() {
            if !self.selection.begin_gesture_at(self.opposite_page, self.anchor_point) {
                return;
            }
            self.begun.set(true);
        }
        match self.view.page_at(overlay) {
            Some(hit) => {
                self.last_page.set(hit.page);
                self.selection.extend_to(hit.page, hit.point);
            }
            None => {
                if let Some(p) = self.view.point_on_page(self.last_page.get(), overlay) {
                    self.selection.extend_to(self.last_page.get(), p);
                }
            }
        }
    }

    /// The pointer released or was cancelled: settle, if the drag re-rooted.
    pub fn end(&self) {
        if self.begun.get() {
            self.selection.end_gesture();
        }
    }
}

/// A press on a handle, armed: the point it grabbed and the drag it starts.
pub struct ArmedSelectionHandle {
    /// The point the user grabbed: the bar's midpoint, overlay px.
    pub base: Point,
    pub drag: SelectionHandleDragSession,
}

/// `armSelectionHandle(selection, view, endpoints, role)`: arm a drag of
/// the `role` handle with the view and the endpoints as they are at the
/// press; it extends the selection from the opposite end. `None` when the
/// handle's page isn't laid out.
pub fn arm_selection_handle(
    selection: Rc<dyn SelectionHandleTarget>,
    view: Rc<dyn SelectionHandleView>,
    endpoints: &SelectionHandleEndpoints,
    role: HandleRole,
) -> Option<ArmedSelectionHandle> {
    let (own, opposite) = match role {
        HandleRole::Start => (&endpoints.start, &endpoints.end),
        HandleRole::End => (&endpoints.end, &endpoints.start),
    };
    let geometry = selection_handle_geom(&*view, own, role)?;
    Some(ArmedSelectionHandle {
        base: midpoint(geometry.bar.from, geometry.bar.to),
        drag: create_selection_handle_drag(selection, view, opposite, own.page),
    })
}
