//! `update/draw-distance.ts`: drawing a distance measurement: the measured
//! line, then the offset of its dimension line.

use std::rc::Rc;

use serde_json::Value;

use crate::error::CoreResult;
use crate::js;
use crate::json::Object;
use crate::measurement::{MeasurementAppearance, distance_leader_length, measurement_draft_fields};
use crate::measurement_shape::measurement_selection_quad;
use crate::placement::MIN_DRAG;
use crate::record::style_of;
use crate::types::{
    CreateDistance, Draft, Effect, LinePoints, LineShape, Model, Phase, PointerInput, Shape,
};
use crate::update::changes::{draft_of, new_record, numbers_left, with_new_record};
use crate::update::page_bound::fraction_on_page;
use crate::update::session::{defaults_for, line_endings_of, tool_annotation};

fn with_draft(model: &Model, draft: Draft) -> Model {
    Model { draft: Some(Rc::new(draft)), ..model.clone() }
}

/// `distancePointer(model, phase, input, preset, measure, flags)`.
pub fn distance_pointer(
    model: &Model,
    phase: Phase,
    input: &PointerInput,
    preset: &str,
    measure: Option<&MeasurementAppearance>,
    flags: Option<&Object>,
) -> CoreResult<(Model, Vec<Effect>)> {
    let Some(Draft::CreateDistance(draft)) = model.draft() else {
        let (Phase::Down, Some(measure)) = (phase, measure) else {
            return Ok((model.clone(), Vec::new()));
        };
        let draft = Draft::CreateDistance(CreateDistance {
            step: "endpoints".into(),
            subtype: "line".into(),
            preset: preset.into(),
            page: input.page(),
            from: input.point,
            to: input.point,
            measure: measure.with_leader_length(0.0).0,
            flags: flags.cloned(),
            order: Default::default(),
        });
        return Ok((
            Model { selected: Rc::new(Vec::new()), ..with_draft(model, draft) },
            Vec::new(),
        ));
    };
    if input.page_number() != draft.page.object_number {
        return Ok((model.clone(), Vec::new()));
    }
    if draft.step == "endpoints" {
        if phase == Phase::Down {
            return Ok((model.clone(), Vec::new()));
        }
        let next = CreateDistance { to: input.point, ..draft.clone() };
        if phase == Phase::Move {
            return Ok((with_draft(model, Draft::CreateDistance(next)), Vec::new()));
        }
        let length = js::hypot(next.to.x - next.from.x, next.to.y - next.from.y);
        if length < MIN_DRAG {
            return Ok((Model { draft: None, ..model.clone() }, Vec::new()));
        }
        return Ok((
            with_draft(
                model,
                Draft::CreateDistance(CreateDistance { step: "offset".into(), ..next }),
            ),
            Vec::new(),
        ));
    }
    if phase == Phase::Up {
        return Ok((model.clone(), Vec::new()));
    }
    let tool = tool_annotation(model, &draft.subtype, Some(&draft.preset));
    let geometry = Shape::Line(LineShape {
        line_points: LinePoints::new(draft.from, draft.to),
        line_endings: Some(line_endings_of(&tool)),
        rotation: 0.0,
        order: Default::default(),
    });
    let style = style_of(&tool);
    let appearance = MeasurementAppearance(draft.measure.clone());
    let wanted = distance_leader_length(&geometry, input.point);
    let fraction = fraction_on_page(input.page_box, |part| {
        measurement_selection_quad(&geometry, &appearance.with_leader_length(wanted * part), &style)
            .to_vec()
    });
    let appearance = appearance.with_leader_length(wanted * fraction);
    if phase == Phase::Move {
        return Ok((
            with_draft(
                model,
                Draft::CreateDistance(CreateDistance { measure: appearance.0, ..draft.clone() }),
            ),
            Vec::new(),
        ));
    }
    let mut fields = measurement_draft_fields(&appearance);
    let interior = match style.interior_color.as_option() {
        Some(c) => Value::String(c.clone()),
        None => style.color.clone().unwrap_or(Value::Null),
    };
    fields.insert("interiorColor".into(), interior);
    let record = new_record(
        model,
        &draft.page,
        draft_of(
            "line",
            &defaults_for(model, &draft.preset),
            &geometry,
            &fields,
            draft.flags.as_ref(),
        ),
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
            ..next
        },
        vec![Effect::create(&id, record.draft)],
    ))
}
