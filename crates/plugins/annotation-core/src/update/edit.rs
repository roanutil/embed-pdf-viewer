//! `update/edit.ts`: editing existing annotations with the pointer. The
//! press picks what to do (select, move, resize, rotate, drag a caption or a
//! leader), the moves preview it, the release commits it (`edit_commit.rs`).

use std::rc::Rc;

use crate::anchor::{anchor_mode_of, anchored_geom};
use crate::error::CoreResult;
use crate::geometry::{
    default_chrome_geometry, geom_drag_handle, geom_handles, geom_rotation, group_resize_anchor,
    group_resize_box,
};
use crate::group::group_members;
use crate::hit::{can_move, hit_test};
use crate::measurement::{distance_leader_length, measurement_of};
use crate::measurement_shape::move_measurement_caption;
use crate::record::{shape_of, style_of};
use crate::rect::normalize_deg;
use crate::selection::{Live, annotation_selection_frame};
use crate::snap::compute_move_snap;
use crate::types::{
    CaptionDraft, Draft, Effect, GroupDraft, HandleDraft, LeaderDraft, Model, MoveDraft, Phase,
    Point, PointerInput, RotateDraft, Target,
};
use crate::update::changes::sub;
use crate::update::edit_commit::edit_up;
use crate::update::page_bound::{
    clamp_move_delta, clamp_point_to_box, delta_on_page, edit_draft_page, fraction_on_page, view_of,
};

const RAD2DEG: f64 = 180.0 / std::f64::consts::PI;

fn angle_at(pivot: Point, point: Point) -> f64 {
    (point.y - pivot.y).atan2(point.x - pivot.x) * RAD2DEG
}

fn arc_to(from: f64, to: f64) -> f64 {
    ((to - from + 540.0) % 360.0) - 180.0
}

/// A rotate draft's live rotation, snapping applied (`rotateDraftDelta`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotateDelta {
    pub delta: f64,
    pub angle: f64,
    pub snapped: bool,
}

/// `rotateDraftDelta(model, draft)`.
pub fn rotate_draft_delta(model: &Model, draft: &RotateDraft) -> RotateDelta {
    let raw = angle_at(draft.pivot, draft.current) - angle_at(draft.pivot, draft.start);
    let one = if draft.ids.len() == 1 { model.record(&draft.ids[0]) } else { None };
    let base = one.map_or(0.0, |r| {
        geom_rotation(&anchored_geom(
            &shape_of(&r.annotation),
            anchor_mode_of(r).as_ref(),
            draft.view.as_ref(),
        ))
    });
    let angle = normalize_deg(base + raw);
    if !model.snap.rotation || draft.free == Some(true) {
        return RotateDelta { delta: raw, angle, snapped: false };
    }
    for target in &model.snap.rotation_angles {
        let adjust = arc_to(angle, normalize_deg(*target));
        if adjust.abs() <= model.snap.rotation_threshold {
            return RotateDelta {
                delta: raw + adjust,
                angle: normalize_deg(*target),
                snapped: true,
            };
        }
    }
    RotateDelta { delta: raw, angle, snapped: false }
}

fn selection_has_rotation(model: &Model, ids: &[String]) -> bool {
    ids.iter()
        .any(|id| model.record(id).is_some_and(|r| geom_rotation(&shape_of(&r.annotation)) != 0.0))
}

/// `editPointer(model, phase, input)`.
pub fn edit_pointer(
    model: &Model,
    phase: Phase,
    input: &PointerInput,
) -> CoreResult<(Model, Vec<Effect>)> {
    match phase {
        Phase::Down => Ok((edit_down(model, input), Vec::new())),
        Phase::Move => Ok((
            if model.draft.is_some() { edit_move(model, input)? } else { model.clone() },
            Vec::new(),
        )),
        Phase::Up => {
            if model.draft.is_some() {
                edit_up(model)
            } else {
                Ok((model.clone(), Vec::new()))
            }
        }
    }
}

fn with_draft(model: &Model, draft: Draft) -> Model {
    Model { draft: Some(Rc::new(draft)), ..model.clone() }
}

/// `editDown(model, input)`.
pub fn edit_down(model: &Model, input: &PointerInput) -> Model {
    let view = view_of(input);
    let chrome = input.chrome.clone().unwrap_or_else(default_chrome_geometry);
    let scale = input.scale.filter(|s| *s != 0.0 && !s.is_nan()).unwrap_or(1.0);
    let hit = hit_test(
        model,
        &input.page(),
        input.point,
        &chrome,
        model.hit_margin / scale,
        input.page_box,
        input.inert.as_ref(),
        view.as_ref(),
    );
    match hit {
        Target::Handle(h) if h.handle == "caption" => with_draft(
            model,
            Draft::Caption(CaptionDraft {
                id: h.id,
                start: input.point,
                delta: Point { x: 0.0, y: 0.0 },
                order: Default::default(),
            }),
        ),
        Target::Handle(h) if h.handle == "leader-start" || h.handle == "leader-end" => with_draft(
            model,
            Draft::Leader(LeaderDraft {
                id: h.id,
                start: input.point,
                delta: 0.0,
                order: Default::default(),
            }),
        ),
        Target::Handle(h) => {
            let record = model.record(&h.id).expect("a hit record");
            let base = anchored_geom(
                &shape_of(&record.annotation),
                anchor_mode_of(record).as_ref(),
                view.as_ref(),
            );
            with_draft(
                model,
                Draft::Handle(HandleDraft {
                    id: h.id,
                    handle: h.handle,
                    base: base.clone(),
                    current: base,
                    view,
                    order: Default::default(),
                }),
            )
        }
        Target::Rotate(r) => with_draft(
            model,
            Draft::Rotate(RotateDraft {
                ids: r.ids,
                pivot: r.pivot,
                start: input.point,
                current: input.point,
                view,
                free: None,
                order: Default::default(),
            })
            .sealed(),
        ),
        Target::GroupHandle(g) => with_draft(
            model,
            Draft::Group(GroupDraft {
                op: "resize".into(),
                anchor: group_resize_anchor(g.box_, &g.handle),
                ids: g.ids,
                handle: g.handle,
                base: g.box_,
                current: g.box_,
                view,
                order: Default::default(),
            }),
        ),
        Target::Annot(a) => {
            let group = group_members(model, &a.id);
            let in_selection = model.is_selected(&a.id);
            let selected: Vec<String> = if input.shift {
                if in_selection {
                    model.selected.iter().filter(|id| !group.contains(id)).cloned().collect()
                } else {
                    let mut next = (*model.selected).clone();
                    next.extend(group.iter().filter(|m| !model.is_selected(m)).cloned());
                    next
                }
            } else if in_selection {
                (*model.selected).clone()
            } else {
                group
            };
            let movable = !selected.is_empty() && selected.iter().all(|id| can_move(model, id));
            let draft = movable.then(|| {
                Draft::Move(MoveDraft {
                    ids: selected.clone(),
                    start: input.point,
                    delta: Point { x: 0.0, y: 0.0 },
                    guides: Vec::new(),
                    order: Default::default(),
                })
            });
            let selected = if !input.shift && in_selection {
                model.selected.clone()
            } else {
                Rc::new(selected)
            };
            Model { selected, draft: draft.map(Rc::new), ..model.clone() }
        }
        Target::Empty(_) => Model { selected: Rc::new(Vec::new()), ..model.clone() },
    }
}

/// The TypeScript's `TypeError` for a record a gesture names that the view doesn't hold.
fn missing_record() -> crate::error::CoreError {
    crate::error::CoreError {
        name: "TypeError",
        message: "Cannot read properties of undefined (reading 'annotation')".into(),
        code: None,
        details: None,
    }
}

/// `editMove(model, input)`. A record the draft names is read only where the
/// TypeScript reads it: a page-bound fraction with no page box never asks.
pub fn edit_move(model: &Model, input: &PointerInput) -> CoreResult<Model> {
    let draft = model.draft().expect("a draft").clone();
    if let Some(home) = edit_draft_page(model, &draft)
        && input.page_number() != home.object_number
    {
        return Ok(model.clone());
    }
    if let Draft::Move(d) = &draft {
        let view = view_of(input);
        let raw = clamp_move_delta(
            model,
            &d.ids,
            sub(input.point, d.start),
            input.page_box,
            view.as_ref(),
        );
        if !model.snap.alignment || input.shift {
            return Ok(with_draft(
                model,
                Draft::Move(MoveDraft { delta: raw, guides: Vec::new(), ..d.clone() }),
            ));
        }
        let scale = input.scale.filter(|s| *s != 0.0 && !s.is_nan()).unwrap_or(1.0);
        let snap = compute_move_snap(
            model,
            &d.ids,
            &input.page(),
            raw,
            model.snap.alignment_threshold / scale,
            input.page_box,
        );
        let delta = clamp_move_delta(model, &d.ids, snap.delta, input.page_box, view.as_ref());
        let guides = snap
            .guides
            .into_iter()
            .filter(
                |g| if g.axis == "x" { delta.x == snap.delta.x } else { delta.y == snap.delta.y },
            )
            .collect();
        return Ok(with_draft(model, Draft::Move(MoveDraft { delta, guides, ..d.clone() })));
    }
    let point = clamp_point_to_box(input.point, input.page_box);
    let mut missing = false;
    let next = match &draft {
        Draft::Leader(d) => {
            let Some(record) = model.record(&d.id) else { return Err(missing_record()) };
            let geometry = shape_of(&record.annotation);
            let measure = measurement_of(&record.annotation);
            let wanted = distance_leader_length(&geometry, point)
                - distance_leader_length(&geometry, d.start);
            let Some(measure) = measure.filter(|m| m.is_distance()) else {
                return Ok(with_draft(
                    model,
                    Draft::Leader(LeaderDraft { delta: wanted, ..d.clone() }),
                ));
            };
            let length = measure.leader_length();
            let fraction = fraction_on_page(input.page_box, |part| {
                let live = Live {
                    measure: Some(Some(measure.with_leader_length(length + wanted * part))),
                    ..Live::default()
                };
                annotation_selection_frame(record, None, &live).corners.to_vec()
            });
            with_draft(model, Draft::Leader(LeaderDraft { delta: wanted * fraction, ..d.clone() }))
        }
        Draft::Caption(d) => {
            let Some(record) = model.record(&d.id) else { return Err(missing_record()) };
            let geometry = shape_of(&record.annotation);
            let measure = measurement_of(&record.annotation);
            let style = style_of(&record.annotation);
            let wanted = sub(point, d.start);
            let delta = match measure {
                None => wanted,
                Some(measure) => delta_on_page(input.page_box, wanted, |moved| {
                    let (geometry, measure) =
                        move_measurement_caption(&geometry, &measure, moved, &style);
                    let live = Live {
                        geometry: Some(geometry),
                        measure: Some(Some(measure)),
                        ..Live::default()
                    };
                    annotation_selection_frame(record, None, &live).corners.to_vec()
                }),
            };
            with_draft(model, Draft::Caption(CaptionDraft { delta, ..d.clone() }))
        }
        Draft::Handle(d) => {
            let record = model.record(&d.id);
            let from = geom_handles(&d.base)
                .into_iter()
                .find(|h| h.id == d.handle)
                .map_or(point, |h| h.at);
            let dragged_by = |delta: Point| {
                geom_drag_handle(
                    &d.base,
                    &d.handle,
                    Point { x: from.x + delta.x, y: from.y + delta.y },
                )
            };
            let delta = delta_on_page(input.page_box, sub(point, from), |moved| match record {
                Some(record) => {
                    let live = Live { geometry: Some(dragged_by(moved)), ..Live::default() };
                    annotation_selection_frame(record, None, &live).corners.to_vec()
                }
                None => {
                    missing = true;
                    Vec::new()
                }
            });
            if missing {
                return Err(missing_record());
            }
            with_draft(
                model,
                Draft::Handle(HandleDraft { current: dragged_by(delta), ..d.clone() }),
            )
        }
        Draft::Rotate(d) => with_draft(
            model,
            Draft::Rotate(RotateDraft {
                current: input.point,
                free: Some(input.shift),
                ..d.clone()
            }),
        ),
        Draft::Group(d) => {
            let iso = selection_has_rotation(model, &d.ids);
            with_draft(
                model,
                Draft::Group(GroupDraft {
                    current: group_resize_box(d.base, &d.handle, point, iso),
                    ..d.clone()
                }),
            )
        }
        _ => model.clone(),
    };
    Ok(next)
}

impl Draft {
    /// The draft with its member order fixed to what it holds now, so a
    /// member set later prints last (a literal's order).
    pub fn sealed(self) -> Draft {
        match self {
            Draft::Rotate(mut d) => {
                d.seal();
                Draft::Rotate(d)
            }
            other => other,
        }
    }
}
