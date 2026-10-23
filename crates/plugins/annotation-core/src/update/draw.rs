//! `update/draw.ts`: drawing new annotations with the pointer: shapes,
//! lines, ink and polygons. A finished drawing becomes a new record and a
//! `create` effect.

use std::rc::Rc;

use serde_json::Value;

use crate::error::CoreResult;
use crate::ink::straighten_ink_stroke;
use crate::json::{self, Object};
use crate::measurement::{MeasurementAppearance, measurement_draft_fields};
use crate::measurement_shape::shape_measurement_readout;
use crate::placement::{GestureForm, MIN_DRAG, PlacementFrame, gesture_placement, placed_shape};
use crate::record::writable_target;
use crate::rect::union_rect;
use crate::types::{
    CapturedEffect, CreateInk, CreateLine, CreatePointerMessage, CreatePoly, CreateRect, Draft,
    Effect, InkShape, Model, Phase, Placement, PointerInput, PolyShape, Rect, Shape,
};
use crate::update::changes::{
    draft_of, is_poly_subtype, new_record, numbers_left, with_new_record,
};
use crate::update::draw_callout::callout_pointer;
use crate::update::draw_distance::distance_pointer;
use crate::update::page_bound::clamp_point_to_box;
use crate::update::session::{defaults_for, line_endings_of, tool_annotation};

/// A box or a line being drawn (`ShapeDraft`).
pub enum ShapeDraft<'a> {
    Rect(&'a CreateRect),
    Line(&'a CreateLine),
}

/// `draftPlacement(draft, pageBox)`.
pub fn draft_placement(draft: ShapeDraft, page_box: Option<Rect>) -> Option<Placement> {
    match draft {
        ShapeDraft::Rect(d) => gesture_placement(
            GestureForm::Box,
            d.from,
            d.to,
            d.click_create.as_ref(),
            &PlacementFrame { page_box, upright: d.upright, display_rotation: d.display_rotation },
        ),
        ShapeDraft::Line(d) => gesture_placement(
            GestureForm::Segment,
            d.from,
            d.to,
            d.click_create.as_ref(),
            &PlacementFrame { page_box, upright: d.upright, display_rotation: d.display_rotation },
        ),
    }
}

fn with_draft(model: &Model, draft: Draft, clear_selection: bool) -> Model {
    Model {
        draft: Some(Rc::new(draft)),
        selected: if clear_selection { Rc::new(Vec::new()) } else { model.selected.clone() },
        ..model.clone()
    }
}

/// The model after a create: the record added, selected, the draft gone, one number used.
fn created(model: &Model, record: crate::types::ModelAnnotation, editing: Option<String>) -> Model {
    let id = record.id.clone();
    let next = with_new_record(model, record);
    Model {
        object_numbers: numbers_left(model, 1),
        selected: Rc::new(vec![id]),
        draft: None,
        editing,
        ..next
    }
}

/// `createPointer(model, phase, subtype, input, ...)`.
pub fn create_pointer(
    model: &Model,
    message: &CreatePointerMessage,
) -> CoreResult<(Model, Vec<Effect>)> {
    let phase = message.phase;
    let subtype = message.subtype.as_str();
    let preset = message.preset.clone().unwrap_or_else(|| subtype.to_string());
    let mut input = message.input.clone();
    if phase != Phase::Down
        && let Some(page) = model.draft().and_then(Draft::page)
        && page.object_number != input.page_number()
    {
        return Ok((model.clone(), Vec::new()));
    }
    if input.page_box.is_some() {
        input.point = clamp_point_to_box(input.point, input.page_box);
    }
    let measure = message.measure.clone().map(MeasurementAppearance);
    let distance_measure = measure.clone().filter(|m| m.is_distance());
    if matches!(model.draft(), Some(Draft::CreateDistance(_)))
        || (distance_measure.is_some() && message.capture.as_deref().is_none_or(str::is_empty))
    {
        return distance_pointer(
            model,
            phase,
            &input,
            &preset,
            distance_measure.as_ref(),
            message.flags.as_ref(),
        );
    }
    if subtype == "free-text-callout" {
        return callout_pointer(model, phase, &input, &preset, message.flags.as_ref());
    }
    match phase {
        Phase::Down => {
            if is_poly_subtype(subtype) {
                if input.finish == Some(true) {
                    return finish_poly_create(model);
                }
                if let Some(Draft::CreatePoly(d)) = model.draft()
                    && d.subtype == subtype
                    && d.preset.as_deref() == Some(preset.as_str())
                    && d.page.object_number == input.page_number()
                {
                    let mut points = d.points.clone();
                    points.push(input.point);
                    let next =
                        Draft::CreatePoly(CreatePoly { points, current: input.point, ..d.clone() });
                    return Ok((Model { draft: Some(Rc::new(next)), ..model.clone() }, Vec::new()));
                }
                let draft = Draft::CreatePoly(CreatePoly {
                    subtype: subtype.into(),
                    preset: Some(preset),
                    page: input.page(),
                    points: vec![input.point],
                    current: input.point,
                    closed: subtype == "polygon",
                    measure: measure.filter(|m| !m.is_distance()).map(|m| m.0),
                    flags: message.flags.clone(),
                    order: Default::default(),
                });
                return Ok((with_draft(model, draft, true), Vec::new()));
            }
            let upright = input.upright == Some(true)
                && input.display_rotation.is_some_and(|r| r != 0.0 && !r.is_nan());
            let draft = match subtype {
                "line" => Some(Draft::CreateLine(CreateLine {
                    measure: message.measure.clone(),
                    capture: message.capture.clone(),
                    subtype: subtype.into(),
                    preset: Some(preset),
                    page: input.page(),
                    from: input.point,
                    to: input.point,
                    display_rotation: if upright { input.display_rotation } else { None },
                    upright: if upright { Some(true) } else { None },
                    click_create: message.click_create.clone(),
                    flags: message.flags.clone(),
                    order: Default::default(),
                })),
                "ink" => match model.draft() {
                    Some(Draft::CreateInk(d))
                        if d.subtype == subtype
                            && d.preset.as_deref() == Some(preset.as_str())
                            && d.page.object_number == input.page_number() =>
                    {
                        let mut strokes = d.strokes.clone();
                        strokes.push(vec![input.point]);
                        Some(Draft::CreateInk(CreateInk { strokes, ..d.clone() }))
                    }
                    _ => Some(Draft::CreateInk(CreateInk {
                        subtype: subtype.into(),
                        preset: Some(preset),
                        page: input.page(),
                        strokes: vec![vec![input.point]],
                        intent: message.intent.clone(),
                        flags: message.flags.clone(),
                        order: Default::default(),
                    })),
                },
                "square" | "circle" | "free-text" | "redact" | "link" => {
                    Some(Draft::CreateRect(CreateRect {
                        subtype: subtype.into(),
                        preset: Some(preset),
                        page: input.page(),
                        from: input.point,
                        to: input.point,
                        display_rotation: if upright { input.display_rotation } else { None },
                        upright: if upright { Some(true) } else { None },
                        click_create: message.click_create.clone(),
                        flags: message.flags.clone(),
                        order: Default::default(),
                    }))
                }
                _ => None,
            };
            Ok(match draft {
                Some(d) => (with_draft(model, d, true), Vec::new()),
                None => (model.clone(), Vec::new()),
            })
        }
        Phase::Move => {
            let next = match model.draft() {
                Some(Draft::CreatePoly(d)) => {
                    Draft::CreatePoly(CreatePoly { current: input.point, ..d.clone() })
                }
                Some(Draft::CreateRect(d)) => {
                    Draft::CreateRect(CreateRect { to: input.point, ..d.clone() })
                }
                Some(Draft::CreateLine(d)) => {
                    Draft::CreateLine(CreateLine { to: input.point, ..d.clone() })
                }
                Some(Draft::CreateInk(d)) => {
                    let mut strokes = d.strokes.clone();
                    if let Some(last) = strokes.last_mut() {
                        last.push(input.point);
                    }
                    Draft::CreateInk(CreateInk { strokes, ..d.clone() })
                }
                _ => return Ok((model.clone(), Vec::new())),
            };
            Ok((Model { draft: Some(Rc::new(next)), ..model.clone() }, Vec::new()))
        }
        Phase::Up => {
            let active = model.draft().cloned();
            match active {
                Some(Draft::CreateInk(d)) => {
                    let mut next = model.clone();
                    if let (Some(options), false) = (&message.straighten_ink, d.strokes.is_empty())
                    {
                        let mut strokes = d.strokes.clone();
                        let last = strokes.len() - 1;
                        strokes[last] = straighten_ink_stroke(&strokes[last], options);
                        next = Model {
                            draft: Some(Rc::new(Draft::CreateInk(CreateInk {
                                strokes,
                                ..d.clone()
                            }))),
                            ..model.clone()
                        };
                    }
                    if message.defer_ink_commit == Some(true) {
                        Ok((next, Vec::new()))
                    } else {
                        finish_ink_create(&next)
                    }
                }
                Some(Draft::CreateRect(ref d)) => commit_shape(model, ShapeDraft::Rect(d), &input),
                Some(Draft::CreateLine(ref d)) => commit_shape(model, ShapeDraft::Line(d), &input),
                _ => Ok((model.clone(), Vec::new())),
            }
        }
    }
}

fn commit_shape(
    model: &Model,
    draft: ShapeDraft,
    input: &PointerInput,
) -> CoreResult<(Model, Vec<Effect>)> {
    let (subtype, preset, page, flags, capture, measure) = match &draft {
        ShapeDraft::Rect(d) => {
            (d.subtype.clone(), d.preset.clone(), d.page.clone(), d.flags.clone(), None, None)
        }
        ShapeDraft::Line(d) => (
            d.subtype.clone(),
            d.preset.clone(),
            d.page.clone(),
            d.flags.clone(),
            d.capture.clone(),
            d.measure.clone(),
        ),
    };
    let tool = tool_annotation(model, &subtype, preset.as_deref());
    let placement = draft_placement(draft, input.page_box);
    let geometry = placement.and_then(|p| placed_shape(&tool, &p));
    let Some(geometry) = geometry else {
        return Ok((Model { draft: None, ..model.clone() }, Vec::new()));
    };
    if let Some(capture) = capture.filter(|c| !c.is_empty()) {
        let effect = Effect::Captured(CapturedEffect {
            tool: capture,
            page,
            geometry,
            order: Default::default(),
        });
        return Ok((Model { draft: None, ..model.clone() }, vec![effect]));
    }
    let mut fields = Object::new();
    if let Some(m) = measure.map(MeasurementAppearance) {
        for (k, v) in measurement_draft_fields(&m) {
            fields.insert(k, v);
        }
    }
    if matches!(geometry, Shape::TextBox(_)) {
        fields.insert("intent".into(), Value::String("free-text".into()));
        fields.insert("contents".into(), Value::String(String::new()));
    }
    if subtype == "link" {
        let target =
            if json::get_str(&tool, "subtype") == Some("link") { tool.get("target") } else { None };
        fields.insert("target".into(), writable_target(target));
    }
    let defaults = defaults_for(model, preset.as_deref().unwrap_or(&subtype));
    let created_record = new_record(
        model,
        &page,
        draft_of(&subtype, &defaults, &geometry, &fields, flags.as_ref()),
        1,
        None,
    )?;
    let id = created_record.record.id.clone();
    let editing = if matches!(geometry, Shape::TextBox(_)) {
        Some(id.clone())
    } else {
        model.editing.clone()
    };
    let next = created(model, created_record.record, editing);
    Ok((next, vec![Effect::create(&id, created_record.draft)]))
}

/// `finishInkCreate(model)`.
pub fn finish_ink_create(model: &Model) -> CoreResult<(Model, Vec<Effect>)> {
    let Some(Draft::CreateInk(d)) = model.draft() else { return Ok((model.clone(), Vec::new())) };
    let points: Vec<_> = d.strokes.concat();
    if !d.strokes.iter().any(|s| s.len() >= 2) || points.is_empty() {
        return Ok((Model { draft: None, ..model.clone() }, Vec::new()));
    }
    let bounds = union_rect(&points);
    if crate::js::max(bounds.width, bounds.height) < MIN_DRAG {
        return Ok((Model { draft: None, ..model.clone() }, Vec::new()));
    }
    let shape = Shape::Ink(InkShape {
        ink_list: d.strokes.clone(),
        rotation: 0.0,
        order: Default::default(),
    });
    let mut fields = Object::new();
    if d.intent.as_deref() == Some("ink-highlight") {
        fields.insert("intent".into(), Value::String("ink-highlight".into()));
    }
    let defaults = defaults_for(model, d.preset.as_deref().unwrap_or(&d.subtype));
    let record = new_record(
        model,
        &d.page,
        draft_of(&d.subtype, &defaults, &shape, &fields, d.flags.as_ref()),
        1,
        None,
    )?;
    let id = record.record.id.clone();
    let next = created(model, record.record, model.editing.clone());
    Ok((next, vec![Effect::create(&id, record.draft)]))
}

/// `finishPolyCreate(model)`.
pub fn finish_poly_create(model: &Model) -> CoreResult<(Model, Vec<Effect>)> {
    let Some(Draft::CreatePoly(d)) = model.draft() else { return Ok((model.clone(), Vec::new())) };
    let min_points = if d.closed { 3 } else { 2 };
    if d.points.len() < min_points {
        return Ok((Model { draft: None, ..model.clone() }, Vec::new()));
    }
    let tool = tool_annotation(model, &d.subtype, d.preset.as_deref());
    let geometry = Shape::Poly(PolyShape {
        vertices: d.points.clone(),
        closed: d.closed,
        line_endings: if d.closed { None } else { Some(line_endings_of(&tool)) },
        caption_center: None,
        rotation: 0.0,
        order: Default::default(),
    });
    let measure = d.measure.clone().map(MeasurementAppearance);
    if let Some(m) = &measure
        && shape_measurement_readout(&geometry, m).unavailable().is_some()
    {
        return Ok((model.clone(), Vec::new()));
    }
    let fields = measure.as_ref().map(measurement_draft_fields).unwrap_or_default();
    let defaults = defaults_for(model, d.preset.as_deref().unwrap_or(&d.subtype));
    let record = new_record(
        model,
        &d.page,
        draft_of(&d.subtype, &defaults, &geometry, &fields, d.flags.as_ref()),
        1,
        None,
    )?;
    let id = record.record.id.clone();
    let next = created(model, record.record, model.editing.clone());
    Ok((next, vec![Effect::create(&id, record.draft)]))
}
