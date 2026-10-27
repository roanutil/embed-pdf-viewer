//! `update/text-markup.ts`: markup over selected text, carets, replace-text
//! (a caret grouped with a strikeout), and the live preview.

use std::rc::Rc;

use serde_json::Value;

use crate::error::CoreResult;
use crate::geometry::geom_bounds;
use crate::js::Record;
use crate::json::{self, Object};
use crate::object::Json;
use crate::record::style_of;
use crate::shapes::caret::caret_from_anchor;
use crate::types::{Effect, MarkupPreview, Model, PageRef, Quad, QuadsShape, Shape, TextEndAnchor};
use crate::update::changes::{draft_of, new_record, numbers_left, with_new_record};
use crate::update::session::{defaults_for, tool_annotation};

fn usable_quads(quads: &[Quad]) -> Vec<Quad> {
    quads
        .iter()
        .filter(|q| {
            let ux = q.upper_right.x - q.upper_left.x;
            let uy = q.upper_right.y - q.upper_left.y;
            let sx = q.lower_left.x - q.upper_left.x;
            let sy = q.lower_left.y - q.upper_left.y;
            (ux * sy - uy * sx).abs() > 0.0
        })
        .copied()
        .collect()
}

fn created(model: &Model, records: Vec<crate::types::ModelAnnotation>, used: usize) -> Model {
    let ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
    let mut next = model.clone();
    for record in records {
        next = with_new_record(&next, record);
    }
    Model {
        object_numbers: numbers_left(model, used),
        selected: Rc::new(ids),
        draft: None,
        preview: None,
        ..next
    }
}

/// `createMarkup(model, subtype, page, quads, preset, flags)`.
pub fn create_markup(
    model: &Model,
    subtype: &str,
    page: &PageRef,
    quads: &[Quad],
    preset: Option<&str>,
    flags: Option<&Object>,
) -> CoreResult<(Model, Vec<Effect>)> {
    let quads = usable_quads(quads);
    if quads.is_empty() {
        return Ok((model.clone(), Vec::new()));
    }
    let shape = Shape::Quads(QuadsShape { quad_points: quads, order: Default::default() });
    let mut fields = Object::new();
    if subtype == "redact" {
        fields.insert("rect".into(), geom_bounds(&shape).json());
    }
    let record = new_record(
        model,
        page,
        draft_of(subtype, &defaults_for(model, preset.unwrap_or(subtype)), &shape, &fields, flags),
        1,
        None,
    )?;
    let id = record.record.id.clone();
    Ok((created(model, vec![record.record], 1), vec![Effect::create(&id, record.draft)]))
}

/// `createReplaceText(model, page, quads, anchor, preset)`: a caret and its grouped strikeout.
pub fn create_replace_text(
    model: &Model,
    page: &PageRef,
    quads: &[Quad],
    anchor: &TextEndAnchor,
    preset: Option<&str>,
) -> CoreResult<(Model, Vec<Effect>)> {
    let quads = usable_quads(quads);
    if quads.is_empty() {
        return Ok((model.clone(), Vec::new()));
    }
    let preset = preset.unwrap_or("replace-text");
    let strike_style = style_of(&tool_annotation(model, "strikeout", Some(preset)));
    let caret_fields = {
        let mut f = Object::new();
        if let Some(color) = strike_style.color.clone() {
            f.insert("color".into(), color);
        }
        if let Some(opacity) = strike_style.opacity.clone() {
            f.insert("opacity".into(), opacity);
        }
        f.insert("intent".into(), Value::String("replace".into()));
        f
    };
    let caret = new_record(
        model,
        page,
        draft_of(
            "caret",
            &Object::new(),
            &Shape::Caret(caret_from_anchor(anchor)),
            &caret_fields,
            None,
        ),
        1,
        None,
    )?;
    let reply = Value::Object(json::object([
        ("to", caret.record.annotation.get("ref").cloned().unwrap_or(Value::Null)),
        ("type", Value::String("group".into())),
    ]));
    let strikeout_shape =
        Shape::Quads(QuadsShape { quad_points: quads, order: Default::default() });
    let strike_fields = json::object([("intent", Value::String("strikeout-text-edit".into()))]);
    let strikeout = new_record(
        model,
        page,
        draft_of("strikeout", &defaults_for(model, preset), &strikeout_shape, &strike_fields, None),
        2,
        Some(reply),
    )?;
    let (caret_id, strike_id) = (caret.record.id.clone(), strikeout.record.id.clone());
    let effects =
        vec![Effect::create(&caret_id, caret.draft), Effect::create(&strike_id, strikeout.draft)];
    Ok((created(model, vec![caret.record, strikeout.record], 2), effects))
}

/// `createCaret(model, page, anchor, flags)`.
pub fn create_caret(
    model: &Model,
    page: &PageRef,
    anchor: &TextEndAnchor,
    flags: Option<&Object>,
) -> CoreResult<(Model, Vec<Effect>)> {
    let caret = caret_from_anchor(anchor);
    if caret.box_.width <= 0.0 || caret.box_.height <= 0.0 {
        return Ok((model.clone(), Vec::new()));
    }
    let record = new_record(
        model,
        page,
        draft_of(
            "caret",
            &defaults_for(model, "caret"),
            &Shape::Caret(caret),
            &Object::new(),
            flags,
        ),
        1,
        None,
    )?;
    let id = record.record.id.clone();
    Ok((created(model, vec![record.record], 1), vec![Effect::create(&id, record.draft)]))
}

/// `setMarkupPreview(model, subtype, quadsByPage, preset)`.
pub fn set_markup_preview(
    model: &Model,
    subtype: &str,
    quads_by_page: &Record<Vec<Quad>>,
    preset: Option<&str>,
) -> Model {
    let mut by_page: Record<Vec<Quad>> = Record::new();
    for (key, quads) in quads_by_page.iter() {
        let usable = usable_quads(quads);
        if !usable.is_empty() {
            by_page.insert(crate::js::number_to_string(crate::js::string_to_number(key)), usable);
        }
    }
    let preview = MarkupPreview {
        subtype: subtype.into(),
        preset: preset.unwrap_or(subtype).into(),
        by_page,
        order: Default::default(),
    };
    Model { preview: Some(Rc::new(preview)), ..model.clone() }
}
