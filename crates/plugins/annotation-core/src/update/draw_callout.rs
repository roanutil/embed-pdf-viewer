//! `update/draw-callout.ts`: drawing a free-text callout: the tip, the
//! knee, then the text box it points from.

use std::rc::Rc;

use serde_json::Value;

use crate::error::CoreResult;
use crate::geometry::upright_rotation;
use crate::json::{self, Object};
use crate::placement::{GestureForm, PlacementFrame, gesture_placement};
use crate::shapes::text_box::callout_shape;
use crate::types::{
    ClickCreate, ClickPolicy, CreateCallout, Draft, Effect, Model, Phase, Placement, PointerInput,
    Rect, Shape,
};
use crate::update::changes::{draft_of, new_record, numbers_left, with_new_record};
use crate::update::session::{defaults_for, tool_annotation};

fn callout_box_policy() -> ClickPolicy {
    ClickPolicy::On(ClickCreate(json::object([
        ("width", json::num(150.0)),
        ("height", json::num(40.0)),
        ("anchor", Value::String("top-left".into())),
    ])))
}

/// `calloutUprightRot(draft)`.
pub fn callout_upright_rot(draft: &CreateCallout) -> f64 {
    match (draft.upright, draft.display_rotation) {
        (Some(true), Some(r)) if r != 0.0 && !r.is_nan() => upright_rotation(r),
        _ => 0.0,
    }
}

/// `calloutBox(draft)`: the text box rect the preview and the commit share.
pub fn callout_box(draft: &CreateCallout) -> Rect {
    let from = draft.box_from.unwrap_or(draft.current);
    let placement = gesture_placement(
        GestureForm::Box,
        from,
        draft.box_to.unwrap_or(from),
        Some(&callout_box_policy()),
        &PlacementFrame {
            page_box: draft.page_box,
            upright: draft.upright,
            display_rotation: draft.display_rotation,
        },
    );
    match placement {
        Some(Placement::Box(b)) => b.rect,
        _ => Rect { x: f64::NAN, y: f64::NAN, width: f64::NAN, height: f64::NAN },
    }
}

fn with_draft(model: &Model, draft: CreateCallout) -> Model {
    Model { draft: Some(Rc::new(Draft::CreateCallout(draft))), ..model.clone() }
}

/// `calloutPointer(model, phase, input, preset, flags)`.
pub fn callout_pointer(
    model: &Model,
    phase: Phase,
    input: &PointerInput,
    preset: &str,
    flags: Option<&Object>,
) -> CoreResult<(Model, Vec<Effect>)> {
    let current = match model.draft() {
        Some(Draft::CreateCallout(d)) => Some(d.clone()),
        _ => None,
    };
    match phase {
        Phase::Down => {
            let Some(draft) = current.filter(|d| d.page.object_number == input.page_number())
            else {
                let upright = input.upright == Some(true)
                    && input.display_rotation.is_some_and(|r| r != 0.0 && !r.is_nan());
                let mut draft = CreateCallout {
                    subtype: "free-text-callout".into(),
                    preset: Some(preset.into()),
                    page: input.page(),
                    step: "knee".into(),
                    tip: input.point,
                    current: input.point,
                    display_rotation: if upright { input.display_rotation } else { None },
                    upright: if upright { Some(true) } else { None },
                    page_box: input.page_box,
                    flags: flags.cloned(),
                    knee: None,
                    box_from: None,
                    box_to: None,
                    order: Default::default(),
                };
                draft.seal();
                return Ok((
                    Model { selected: Rc::new(Vec::new()), ..with_draft(model, draft) },
                    Vec::new(),
                ));
            };
            if draft.step == "knee" {
                return Ok((
                    with_draft(
                        model,
                        CreateCallout {
                            knee: Some(input.point),
                            step: "box".into(),
                            current: input.point,
                            ..draft
                        },
                    ),
                    Vec::new(),
                ));
            }
            Ok((
                with_draft(
                    model,
                    CreateCallout {
                        box_from: Some(input.point),
                        box_to: Some(input.point),
                        ..draft
                    },
                ),
                Vec::new(),
            ))
        }
        Phase::Move => {
            let Some(draft) = current else { return Ok((model.clone(), Vec::new())) };
            if draft.step == "box" && draft.box_from.is_some() {
                return Ok((
                    with_draft(model, CreateCallout { box_to: Some(input.point), ..draft }),
                    Vec::new(),
                ));
            }
            Ok((with_draft(model, CreateCallout { current: input.point, ..draft }), Vec::new()))
        }
        Phase::Up => {
            let Some(draft) = current.filter(|d| d.step == "box" && d.box_from.is_some()) else {
                return Ok((model.clone(), Vec::new()));
            };
            let rect = callout_box(&draft);
            let rot = callout_upright_rot(&draft);
            let tool = tool_annotation(model, "free-text-callout", draft.preset.as_deref());
            let tool_ending = if json::get_str(&tool, "subtype") == Some("free-text") {
                json::get_str(&tool, "lineEnding")
            } else {
                None
            };
            let ending = match tool_ending {
                Some(e) if !e.is_empty() && e != "none" => e.to_string(),
                _ => "open-arrow".into(),
            };
            let shape = Shape::TextBox(callout_shape(rect, rot, draft.tip, draft.knee, &ending));
            let fields = json::object([
                ("intent", Value::String("free-text-callout".into())),
                ("contents", Value::String(String::new())),
            ]);
            let defaults =
                defaults_for(model, draft.preset.as_deref().unwrap_or("free-text-callout"));
            let record = new_record(
                model,
                &draft.page,
                draft_of("free-text-callout", &defaults, &shape, &fields, draft.flags.as_ref()),
                1,
                None,
            )?;
            let id = record.record.id.clone();
            let next = with_new_record(model, record.record);
            Ok((
                Model {
                    object_numbers: numbers_left(model, 1),
                    selected: Rc::new(vec![id.clone()]),
                    draft: None,
                    editing: Some(id.clone()),
                    ..next
                },
                vec![Effect::create(&id, record.draft)],
            ))
        }
    }
}
