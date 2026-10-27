//! `update/marquee.ts`: rubber-band selection over what each annotation paints.

use std::rc::Rc;

use crate::group::expand_groups;
use crate::hit::{is_selectable, painted_of};
use crate::js::IdSet;
use crate::painted::painted_touches;
use crate::plane::is_substrate_only;
use crate::rect::rect_from_points;
use crate::types::{
    Draft, Effect, Id, MarqueeDraft, Model, PageRef, Phase, PointerInput, Rect, ViewEnv,
};
use crate::update::page_bound::{clamp_point_to_box, view_of};

/// `marqueePointer(model, phase, input)`.
pub fn marquee_pointer(model: &Model, phase: Phase, input: &PointerInput) -> (Model, Vec<Effect>) {
    let point = clamp_point_to_box(input.point, input.page_box);
    if phase == Phase::Down {
        let draft = Draft::Marquee(MarqueeDraft {
            page: input.page(),
            from: point,
            to: point,
            order: Default::default(),
        });
        return (Model { draft: Some(Rc::new(draft)), ..model.clone() }, Vec::new());
    }
    let Some(Draft::Marquee(d)) = model.draft() else { return (model.clone(), Vec::new()) };
    if d.page.object_number != input.page_number() {
        return (model.clone(), Vec::new());
    }
    if phase == Phase::Move {
        let next = Draft::Marquee(MarqueeDraft { to: point, ..d.clone() });
        return (Model { draft: Some(Rc::new(next)), ..model.clone() }, Vec::new());
    }
    let hits = selection_in_box(
        model,
        &d.page,
        rect_from_points(d.from, point),
        input.inert.as_ref(),
        view_of(input).as_ref(),
    );
    let selected = if input.shift { toggle_selection(&model.selected, &hits) } else { hits };
    (Model { selected: Rc::new(selected), draft: None, ..model.clone() }, Vec::new())
}

fn toggle_selection(base: &[Id], ids: &[Id]) -> Vec<Id> {
    let mut next: Vec<Id> = Vec::new();
    for id in base {
        if !next.contains(id) {
            next.push(id.clone());
        }
    }
    for id in ids {
        if let Some(i) = next.iter().position(|x| x == id) {
            next.remove(i);
        } else {
            next.push(id.clone());
        }
    }
    next
}

/// `selectionInBox(model, page, box, inert, view)`: what a box selects, whole groups.
pub fn selection_in_box(
    model: &Model,
    page: &PageRef,
    rect: Rect,
    inert: Option<&IdSet>,
    view: Option<&ViewEnv>,
) -> Vec<Id> {
    expand_groups(model, &annots_in_box(model, page, rect, inert, view))
}

/// `annotsInBox(model, page, box, inert, view)`: the annotations whose paint the box touches.
pub fn annots_in_box(
    model: &Model,
    page: &PageRef,
    rect: Rect,
    inert: Option<&IdSet>,
    view: Option<&ViewEnv>,
) -> Vec<Id> {
    model
        .order
        .iter()
        .filter(|id| {
            let Some(record) = model.record(id) else { return false };
            if record.page_number() != Some(page.object_number)
                || inert.is_some_and(|s| s.has(id))
                || !is_selectable(model, id)
            {
                return false;
            }
            if is_substrate_only(&record.annotation) {
                return false;
            }
            painted_touches(&painted_of(record, view), rect)
        })
        .cloned()
        .collect()
}
