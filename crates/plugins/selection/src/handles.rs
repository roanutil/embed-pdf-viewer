//! `handles.ts`: the pure half of selection handles. A handle is the
//! boundary glyph's leading (or trailing) edge, projected by the hosting
//! view; the drag session speaks the selection's own gesture verbs.

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::{Point, TextQuad, TextQuadSide, js, text_quad_edge};
use serde::{Deserialize, Serialize};

use crate::capability::SelectionCapability;
use crate::types::PageObjectNumber;

/// A page under an overlay point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PagePoint {
    pub pon: PageObjectNumber,
    pub point: Point,
}

/// What handle geometry and drags need from the hosting view.
pub trait SelectionHandleView {
    /// Page content point to overlay px; point-exact.
    fn to_overlay(&self, pon: PageObjectNumber, pt: Point) -> Option<Point>;
    /// Overlay px to the page under it, `None` over a gap.
    fn page_at(&self, overlay: Point) -> Option<PagePoint>;
    /// Overlay px to a specific page's content space, unclamped.
    fn point_on_page(&self, pon: PageObjectNumber, overlay: Point) -> Option<Point>;
}

/// One selection boundary as the handle needs it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionHandleEndpoint {
    pub pon: PageObjectNumber,
    pub glyph_quad: TextQuad,
    /// Reading direction of its segment (1 is the frame's +x).
    pub advance: i32,
}

/// The gesture verbs a handle drag drives.
pub trait SelectionHandleTarget {
    fn begin_at(&self, pon: PageObjectNumber, point: Point) -> bool;
    fn extend_to(&self, pon: PageObjectNumber, point: Point);
    fn end(&self);
}

impl SelectionHandleTarget for SelectionCapability {
    fn begin_at(&self, pon: PageObjectNumber, point: Point) -> bool {
        SelectionCapability::begin_at(self, pon, point)
    }
    fn extend_to(&self, pon: PageObjectNumber, point: Point) {
        SelectionCapability::extend_to(self, pon, point);
    }
    fn end(&self) {
        SelectionCapability::end(self);
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
    let [upper_page, lower_page] = text_quad_edge(
        ep.glyph_quad,
        if leading { TextQuadSide::Start } else { TextQuadSide::End },
    );
    let upper = view.to_overlay(ep.pon, upper_page);
    let lower = view.to_overlay(ep.pon, lower_page);
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
    opposite_pon: PageObjectNumber,
    anchor_point: Point,
    begun: Cell<bool>,
    last_pon: Cell<PageObjectNumber>,
}

/// `createSelectionHandleDrag(selection, view, opposite, draggedPon)`.
pub fn create_selection_handle_drag(
    selection: Rc<dyn SelectionHandleTarget>,
    view: Rc<dyn SelectionHandleView>,
    opposite: &SelectionHandleEndpoint,
    dragged_pon: PageObjectNumber,
) -> SelectionHandleDragSession {
    let q = opposite.glyph_quad;
    let anchor_point =
        midpoint(midpoint(q.upper_start, q.lower_end), midpoint(q.upper_end, q.lower_start));
    SelectionHandleDragSession {
        selection,
        view,
        opposite_pon: opposite.pon,
        anchor_point,
        begun: Cell::new(false),
        last_pon: Cell::new(dragged_pon),
    }
}

impl SelectionHandleDragSession {
    /// `move(overlay)`: feed the pointer's current overlay position.
    pub fn move_to(&self, overlay: Point) {
        if !self.begun.get() {
            if !self.selection.begin_at(self.opposite_pon, self.anchor_point) {
                return;
            }
            self.begun.set(true);
        }
        match self.view.page_at(overlay) {
            Some(hit) => {
                self.last_pon.set(hit.pon);
                self.selection.extend_to(hit.pon, hit.point);
            }
            None => {
                if let Some(p) = self.view.point_on_page(self.last_pon.get(), overlay) {
                    self.selection.extend_to(self.last_pon.get(), p);
                }
            }
        }
    }

    /// The pointer released or was cancelled: settle, if the drag re-rooted.
    pub fn end(&self) {
        if self.begun.get() {
            self.selection.end();
        }
    }
}
