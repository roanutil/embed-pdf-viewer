//! `hit.ts`: paint order, selectability and the hit test.

use serde::{Deserialize, Serialize};

use crate::anchor::{anchor_mode_of, anchored_geom, anchored_stroke_width};
use crate::flags::{annot_interactive, annot_transformable, viewable};
use crate::geometry::{
    geom_handles, geom_hit, obb_from_geom, place_rotate_knob, point_in_quad, rect_handles_for,
    selection_center, selection_quad, union_rect,
};
use crate::group::group_caps;
use crate::js::IdSet;
use crate::kinds::{caps_for, is_markup};
use crate::plane::is_substrate_only;
use crate::types::{
    Annot, ChromeGeom, Cursor, Geom, Id, Model, PageObjectNumber, Point, Rect, ViewEnv,
};

/// What is under a content point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum Target {
    Handle {
        id: Id,
        handle: String,
        cursor: Cursor,
    },
    Rotate {
        ids: Vec<Id>,
        pivot: Point,
    },
    GroupHandle {
        ids: Vec<Id>,
        handle: String,
        cursor: Cursor,
        #[serde(rename = "box")]
        box_: Rect,
    },
    Annot {
        id: Id,
    },
    Empty,
}

/// Page annotation ids in paint order: markups first, then every other
/// kind, each in creation order; what `/F` hides and the substrate-only
/// annotations are culled.
pub fn paint_order(m: &Model, pon: PageObjectNumber) -> Vec<Id> {
    let mut markup = Vec::new();
    let mut other = Vec::new();
    for id in &m.order {
        let Some(a) = m.by_id.get(id) else { continue };
        if a.pon != pon {
            continue;
        }
        if !viewable(&a.flags, m.selected.contains(id)) {
            continue;
        }
        if is_substrate_only(a) {
            continue;
        }
        if is_markup(&a.subtype) { markup.push(id.clone()) } else { other.push(id.clone()) }
    }
    markup.extend(other);
    markup
}

/// Can this annotation be clicked to select?
pub fn is_selectable(m: &Model, id: &str) -> bool {
    m.by_id.get(id).is_some_and(|a| annot_interactive(a) && caps_for(&a.subtype).selectable)
}

/// An anchored kind's quad geometry is bound to text.
fn text_bound(a: &Annot) -> bool {
    caps_for(&a.subtype).anchored && matches!(a.geom, Geom::Quads { .. })
}

/// Can this annotation be dragged by its body?
pub fn can_move(m: &Model, id: &str) -> bool {
    m.by_id
        .get(id)
        .is_some_and(|a| annot_transformable(a) && caps_for(&a.subtype).movable && !text_bound(a))
}

fn has_handles(a: &Annot) -> bool {
    if !annot_transformable(a) || text_bound(a) {
        return false;
    }
    let c = caps_for(&a.subtype);
    c.resizable || c.vertex_editable
}

fn hit_geom_of(a: &Annot, view: Option<ViewEnv>) -> Geom {
    anchored_geom(&a.geom, anchor_mode_of(a), view)
}

fn hit_stroke_of(a: &Annot, view: Option<ViewEnv>) -> f64 {
    anchored_stroke_width(a.style.stroke_width, anchor_mode_of(a), view)
}

fn is_filled(a: &Annot) -> bool {
    a.style.interior_color.is_some()
        || matches!(a.geom, Geom::Quads { .. })
        || caps_for(&a.subtype).opaque_body
}

fn in_rect(b: Rect, p: Point) -> bool {
    p.x >= b.x && p.x <= b.x + b.width && p.y >= b.y && p.y <= b.y + b.height
}

fn in_bounds(a: &Annot, p: Point, view: Option<ViewEnv>) -> bool {
    point_in_quad(
        p,
        &selection_quad(&hit_geom_of(a, view), hit_stroke_of(a, view), Some(&a.style.border)),
    )
}

fn selection_union_bounds(m: &Model, pon: PageObjectNumber, view: Option<ViewEnv>) -> Option<Rect> {
    let sel: Vec<&Id> = m
        .selected
        .iter()
        .filter(|id| {
            m.by_id.get(*id).is_some_and(|a| a.pon == pon)
                && is_selectable(m, id)
                && can_move(m, id)
        })
        .collect();
    if sel.len() < 2 {
        return None;
    }
    let mut corners = Vec::new();
    for id in sel {
        let a = &m.by_id[id];
        corners.extend(selection_quad(
            &hit_geom_of(a, view),
            hit_stroke_of(a, view),
            Some(&a.style.border),
        ));
    }
    Some(union_rect(&corners))
}

/// The axis-aligned union of the selection bounds of every selected
/// annotation on a page.
pub fn group_union_bounds(m: &Model, pon: PageObjectNumber, view: Option<ViewEnv>) -> Option<Rect> {
    let mut corners = Vec::new();
    for id in &m.selected {
        let Some(a) = m.by_id.get(id) else { continue };
        if a.pon != pon {
            continue;
        }
        corners.extend(selection_quad(
            &hit_geom_of(a, view),
            hit_stroke_of(a, view),
            Some(&a.style.border),
        ));
    }
    (!corners.is_empty()).then(|| union_rect(&corners))
}

fn within(at: Point, p: Point, tol: f64) -> bool {
    (at.x - p.x).abs() <= tol && (at.y - p.y).abs() <= tol
}

fn box_corners(u: Rect) -> [Point; 4] {
    [
        Point { x: u.x, y: u.y },
        Point { x: u.x + u.width, y: u.y },
        Point { x: u.x + u.width, y: u.y + u.height },
        Point { x: u.x, y: u.y + u.height },
    ]
}

/// What is under the content point: the single selection's knob or a
/// handle, a group's knob or box handle, the topmost body, the union gap of
/// a multi-selection, else empty.
#[allow(clippy::too_many_arguments)]
pub fn hit_test(
    m: &Model,
    pon: PageObjectNumber,
    p: Point,
    geom: &ChromeGeom,
    stroke_margin: f64,
    page_box: Option<Rect>,
    inert: Option<&IdSet>,
    view: Option<ViewEnv>,
) -> Target {
    if m.selected.len() == 1 && is_selectable(m, &m.selected[0]) {
        let a = &m.by_id[&m.selected[0]];
        if a.pon == pon {
            if caps_for(&a.subtype).rotatable && annot_transformable(a) {
                let hg = hit_geom_of(a, view);
                if let Some(obb) = obb_from_geom(&hg, hit_stroke_of(a, view), Some(&a.style.border))
                {
                    let knob = place_rotate_knob(&obb.corners, geom.knob_offset, page_box);
                    if within(knob.at, p, geom.knob_tol) {
                        return Target::Rotate {
                            ids: vec![a.id.clone()],
                            pivot: selection_center(&hg, hit_stroke_of(a, view)),
                        };
                    }
                }
            }
            if has_handles(a) {
                for h in geom_handles(&hit_geom_of(a, view)) {
                    if within(h.at, p, geom.handle_tol) {
                        return Target::Handle { id: a.id.clone(), handle: h.id, cursor: h.cursor };
                    }
                }
            }
        }
    } else if m.selected.len() > 1 {
        let gc = group_caps(m, &m.selected);
        let on_page = || {
            m.selected
                .iter()
                .filter(|id| m.by_id.get(*id).is_some_and(|a| a.pon == pon))
                .cloned()
                .collect()
        };
        if gc.rotatable
            && let Some(union) = group_union_bounds(m, pon, view)
        {
            let knob = place_rotate_knob(&box_corners(union), geom.knob_offset, page_box);
            if within(knob.at, p, geom.knob_tol) {
                let pivot =
                    Point { x: union.x + union.width / 2.0, y: union.y + union.height / 2.0 };
                return Target::Rotate { ids: on_page(), pivot };
            }
        }
        if gc.resizable
            && let Some(union) = group_union_bounds(m, pon, view)
        {
            for h in rect_handles_for(union) {
                if within(h.at, p, geom.handle_tol) {
                    return Target::GroupHandle {
                        ids: on_page(),
                        handle: h.id,
                        cursor: h.cursor,
                        box_: union,
                    };
                }
            }
        }
    }
    let order = paint_order(m, pon);
    for id in order.iter().rev() {
        let Some(a) = m.by_id.get(id) else { continue };
        if inert.is_some_and(|s| s.has(id)) || !is_selectable(m, id) {
            continue;
        }
        let hit = if m.selected.contains(id) && can_move(m, id) {
            in_bounds(a, p, view)
        } else {
            geom_hit(&hit_geom_of(a, view), p, stroke_margin, is_filled(a), hit_stroke_of(a, view))
        };
        if hit {
            return Target::Annot { id: id.clone() };
        }
    }
    if let Some(union) = selection_union_bounds(m, pon, view)
        && in_rect(union, p)
    {
        for id in order.iter().rev() {
            if m.selected.contains(id) && can_move(m, id) {
                return Target::Annot { id: id.clone() };
            }
        }
    }
    Target::Empty
}

/// The cursor to show on hover.
#[allow(clippy::too_many_arguments)]
pub fn cursor_at(
    m: &Model,
    pon: PageObjectNumber,
    p: Point,
    geom: &ChromeGeom,
    stroke_margin: f64,
    page_box: Option<Rect>,
    inert: Option<&IdSet>,
    view: Option<ViewEnv>,
) -> Option<Cursor> {
    match hit_test(m, pon, p, geom, stroke_margin, page_box, inert, view) {
        Target::Handle { cursor, .. } | Target::GroupHandle { cursor, .. } => Some(cursor),
        Target::Rotate { .. } => Some("grab".into()),
        Target::Annot { id } => Some(
            if m.selected.contains(&id) && can_move(m, &id) { "move" } else { "pointer" }.into(),
        ),
        Target::Empty => None,
    }
}
