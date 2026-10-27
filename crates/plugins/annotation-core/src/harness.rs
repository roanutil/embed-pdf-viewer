//! The conformance adapter: the function table the vectors call by the
//! TypeScript names, with arguments and results in canonical JSON. The
//! wrappers mirror the ones in
//! `tooling/plugin-conformance/src/suites/annotation-core-*.ts`; the case
//! builder there expands every model argument into a whole current model
//! before recording, so a model reads as it is.
//!
//! A few names reach JavaScript's `Object.prototype` through a plain-object
//! lookup (`kindNamed('constructor')`, `defaultsFor(model, 'toString')`).
//! The core answers those as any unknown name; the wrappers here print what
//! the TypeScript prints for them, so the vectors' negative cases replay.

use std::rc::Rc;

use epdf_geometry::{Point, Rect, Size};
use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::anchor::{
    AnchorMode, anchor_mode_of, anchor_of, anchored_box, anchored_box_value, anchored_geom,
    anchored_stroke_width, unanchored_geom,
};
use crate::cloudy::{cloudy_path, cloudy_poly_path};
use crate::endings::{ending_nodes, ending_points};
use crate::error::CoreError;
use crate::flags::{
    FLAG_KEYS, annot_contents_editable, annot_deletable, annot_interactive, annot_transformable,
    drawn_flags, flags_equal, interactive, merge_flags, no_annotation_flags, viewable,
};
use crate::geometry::*;
use crate::group::{expand_groups, group_caps, group_key_of, group_members};
use crate::hit::{can_move, cursor_at, group_union_bounds, hit_test, is_selectable, paint_order};
use crate::js::{self, IdSet, is_prototype_key};
use crate::json::{self, Object, num};
use crate::kinds::{AnnotationKind, KIND_NAMES, blend_for, declared_kind, kind_named};
use crate::links::{link_children_of, link_of};
use crate::measurement::*;
use crate::measurement_shape::*;
use crate::object::{Json, JsonError};
use crate::painted::painted_near;
use crate::placement::{
    MIN_DRAG, PlacementFrame, clamp_rect_to_box, placed_shape, resolve_click_placement,
};
use crate::plane::{is_attached_link, is_conversation_only, is_substrate_only};
use crate::props::{initial_text_style, kind_takes_link, properties_of, shared_properties};
use crate::record::{shape_of, style_of, text_of, with_values};
use crate::rect::*;
use crate::richtext::{
    self, RichTextStyleDelta, apply_style_to_range, is_plain_rich_text, locate_offset,
    paragraphs_from_plain_text, plain_text_of, range_has_style, rich_text_length, same_style_delta,
    split_runs_at, style_at,
};
use crate::scene::{layout_redact_label, scene};
use crate::selection::{Live, SelectionFrame, annotation_selection_frame};
use crate::shapes::caret::{caret_from_anchor, caret_rect_from_anchor};
use crate::shapes::points::MITER_LIMIT;
use crate::shapes::text_box::{callout_end, text_plate_inset};
use crate::snap::compute_move_snap;
use crate::types::*;
use crate::update::{
    RotateDelta, annots_in_box, callout_box, callout_upright_rot, defaults_for, initial_model,
    rotate_draft_delta, update,
};
use crate::view::{
    TextBox, chrome, creation_draft_anchor, page_items, selected_items, selection_anchor,
    selection_bounds_on_page, selection_knob, text_boxes,
};

type Out = Result<Value, Error>;

fn decode(e: JsonError) -> Error {
    Error::new(format!("[harness] cannot decode: {e}"))
}

/// A core refusal as the TypeScript's error prints.
fn thrown(e: CoreError) -> Error {
    let mut error = Error::named(e.name, e.message);
    if let Some(code) = e.code {
        error.members.insert("code".into(), Value::String(code.into()));
    }
    if let Some(details) = e.details {
        error.members.insert("details".into(), *details);
    }
    error
}

fn canonical_error(e: &Error) -> Value {
    e.to_canonical()
}

/// Argument `i`, `None` when missing or `undefined`.
fn opt(a: &[Value], i: usize) -> Option<&Value> {
    a.get(i).filter(|v| !json::undefined(Some(v)))
}

/// Argument `i` read as `T`.
fn arg<T: Json>(a: &[Value], i: usize) -> Result<T, Error> {
    T::from_json(a.get(i)).map_err(|e| decode(e.at(&format!("argument {i}"))))
}

fn ok<T: Json>(value: T) -> Out {
    Ok(value.to_json().unwrap_or_else(epdf_kernel::undefined))
}

fn ok_value(value: Value) -> Out {
    Ok(value)
}

fn undefined() -> Value {
    epdf_kernel::undefined()
}

fn function_marker() -> Value {
    Value::Object(json::object([("$function", Value::Bool(true))]))
}

fn points_value(points: &[Point]) -> Value {
    json::points_value(points)
}

fn obj(value: &Value) -> Object {
    value.as_object().cloned().unwrap_or_default()
}

fn stroke(a: &[Value], i: usize) -> Result<Stroke, Error> {
    match opt(a, i) {
        Some(Value::Object(_)) => Stroke::of_value(&a[i]).map_err(decode),
        Some(v) => Ok(Stroke::width(json::number(v).unwrap_or(f64::NAN))),
        None => Ok(Stroke::width(f64::NAN)),
    }
}

fn record_of(a: &[Value], i: usize) -> Result<ModelAnnotation, Error> {
    arg(a, i)
}

fn annotation_of(a: &[Value], i: usize) -> Object {
    opt(a, i).map(obj).unwrap_or_default()
}

fn mode_of(a: &[Value], i: usize) -> Result<Option<AnchorMode>, Error> {
    arg::<Option<AnchorMode>>(a, i)
}

// ── printing ────────────────────────────────────────────────────────────────

fn handles_value(handles: &[Handle]) -> Value {
    handles.to_vec().json()
}

fn knob_value(knob: Option<Knob>) -> Value {
    match knob {
        None => Value::Null,
        Some(k) => Value::Object(json::object([("at", k.at.json()), ("from", k.from.json())])),
    }
}

fn segment_value(s: &DistanceSegment) -> Value {
    Value::Object(json::object([("from", s.from.json()), ("to", s.to.json())]))
}

fn caption_value(caption: Option<&CaptionLayout>) -> Value {
    match caption {
        None => Value::Null,
        Some(c) => Value::Object(json::object([
            ("text", Value::String(c.text.clone())),
            ("center", c.center.json()),
            ("along", c.along.json()),
            ("normal", c.normal.json()),
            ("width", num(c.width)),
            ("height", num(c.height)),
            ("bounds", points_value(&c.bounds)),
        ])),
    }
}

fn distance_layout_value(layout: &DistanceLayout) -> Value {
    let segments =
        |list: &[DistanceSegment]| Value::Array(list.iter().map(segment_value).collect());
    Value::Object(json::object([
        ("along", layout.along.json()),
        ("normal", layout.normal.json()),
        ("length", num(layout.length)),
        ("measuredStart", layout.measured_start.json()),
        ("measuredEnd", layout.measured_end.json()),
        ("dimensionStart", layout.dimension_start.json()),
        ("dimensionEnd", layout.dimension_end.json()),
        ("dimensionSegments", segments(&layout.dimension_segments)),
        ("leaderSegments", segments(&layout.leader_segments)),
        ("captionConnector", segments(&layout.caption_connector)),
        ("caption", caption_value(layout.caption.as_ref())),
        ("arrowPlacement", Value::String(layout.arrow_placement.into())),
        ("endings", layout.endings.json()),
        ("selectionPoints", points_value(&layout.selection_points)),
        ("visualBounds", layout.visual_bounds.json()),
    ]))
}

fn shape_layout_value(layout: &ShapeMeasurementLayout) -> Value {
    Value::Object(json::object([
        ("caption", caption_value(layout.caption.as_ref())),
        ("selectionPoints", points_value(&layout.selection_points)),
        ("visualBounds", layout.visual_bounds.json()),
    ]))
}

fn measurement_layout_value(layout: Option<MeasurementLayout>) -> Value {
    match layout {
        None => Value::Null,
        Some(MeasurementLayout::Distance(d)) => distance_layout_value(&d),
        Some(MeasurementLayout::Shape(s)) => shape_layout_value(&s),
    }
}

fn frame_value(frame: &SelectionFrame) -> Value {
    Value::Object(json::object([
        ("corners", points_value(&frame.corners)),
        ("center", frame.center.json()),
        ("angle", num(frame.angle)),
    ]))
}

fn target_value(target: Target) -> Value {
    target.json()
}

fn cursor_value(cursor: Option<Cursor>) -> Value {
    cursor.map_or(Value::Null, Value::String)
}

fn kind_value(kind: &AnnotationKind) -> Value {
    let family: Object = [
        "read",
        "write",
        "placed",
        "bounds",
        "drawnBounds",
        "rect",
        "selectionBounds",
        "oriented",
        "turnedCorners",
        "pivot",
        "translate",
        "rotateAbout",
        "scaleAbout",
        "upright",
        "handles",
        "drag",
        "painted",
        "scene",
    ]
    .iter()
    .map(|k| ((*k).to_string(), function_marker()))
    .collect();
    let mut out = json::object([
        ("name", Value::String(kind.name.clone())),
        ("family", Value::Object(family)),
        ("style", function_marker()),
    ]);
    if kind.text.is_some() {
        out.insert("text".into(), function_marker());
    }
    out.insert("caps".into(), kind.caps.json());
    out.insert("properties".into(), Value::Array(kind.own_properties()));
    Value::Object(out)
}

/// What `KINDS[name]` gives for a name `Object.prototype` has: a function, or `{}` for `__proto__`.
fn prototype_member(name: &str) -> Value {
    if name == "__proto__" { Value::Object(Map::new()) } else { function_marker() }
}

fn type_error(message: &str) -> Error {
    Error::named("TypeError", message)
}

// ── geometry ────────────────────────────────────────────────────────────────

fn geom_hit_probe(a: &[Value]) -> Out {
    let g: Shape = arg(a, 0)?;
    let margin: f64 = arg(a, 1)?;
    let filled: bool = arg(a, 2)?;
    let s = stroke(a, 3)?;
    let points: Vec<Point> = arg(a, 4)?;
    ok_value(Value::Array(
        points.iter().map(|p| Value::Bool(geom_hit(&g, *p, margin, filled, s))).collect(),
    ))
}

fn obb_value(obb: Option<Obb>) -> Value {
    match obb {
        None => Value::Null,
        Some(o) => Value::Object(json::object([
            ("corners", points_value(&o.corners)),
            ("angle", num(o.angle)),
        ])),
    }
}

fn group_resize_factors_value(base: Rect, current: Rect) -> Value {
    let (sx, sy) = group_resize_factors(base, current);
    Value::Object(json::object([("sx", num(sx)), ("sy", num(sy))]))
}

// ── models ──────────────────────────────────────────────────────────────────

fn model(a: &[Value], i: usize) -> Result<Model, Error> {
    arg(a, i)
}

fn page(a: &[Value], i: usize) -> Result<PageRef, Error> {
    arg(a, i)
}

fn view(a: &[Value], i: usize) -> Result<Option<ViewEnv>, Error> {
    arg(a, i)
}

fn inert(a: &[Value], i: usize) -> Option<IdSet> {
    opt(a, i).map(IdSet::of_value)
}

fn group_probe(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ids: Vec<Id> = arg(a, 1)?;
    let out = ids
        .iter()
        .map(|id| {
            Value::Array(vec![
                group_key_of(&m, id).map_or(Value::Null, Value::String),
                group_members(&m, id).json(),
                Value::Array(
                    link_children_of(&m, id).iter().map(|r| Value::String(r.id.clone())).collect(),
                ),
                link_of(&m, id),
            ])
        })
        .collect();
    ok_value(Value::Array(out))
}

fn defaults_for_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let preset: String = arg(a, 1)?;
    if is_prototype_key(&preset) && !m.defaults.contains_key(&preset) {
        return ok_value(prototype_member(&preset));
    }
    ok(defaults_for(&m, &preset))
}

fn record_views(a: &[Value]) -> Out {
    let record = record_of(a, 0)?;
    let mut out = json::object([("style", style_of(&record.annotation).json())]);
    if let Some(text) = text_of(&record.annotation) {
        out.insert("text".into(), text.json());
    }
    out.insert("shape".into(), shape_of(&record.annotation).json());
    ok_value(Value::Object(out))
}

fn with_values_fn(a: &[Value]) -> Out {
    let record = record_of(a, 0)?;
    let values = annotation_of(a, 1);
    match with_values(&record, &values).map_err(thrown)? {
        Some(next) => ok(next),
        None => ok(record),
    }
}

// ── hit testing ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn hit_pair(
    m: &Model,
    pg: &PageRef,
    p: Point,
    gm: &ChromeGeometry,
    margin: f64,
    page_box: Option<Rect>,
    inert: Option<&IdSet>,
    v: Option<&ViewEnv>,
) -> Value {
    Value::Array(vec![
        target_value(hit_test(m, pg, p, gm, margin, page_box, inert, v)),
        cursor_value(cursor_at(m, pg, p, gm, margin, page_box, inert, v)),
    ])
}

fn hit_calls(a: &[Value]) -> Out {
    let calls = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for call in calls {
        let c = obj(&call);
        let get = |k: &str| c.get(k).cloned();
        let m = Model::read(&get("model").unwrap_or(Value::Null)).map_err(decode)?;
        let pg = PageRef::read(&get("page").unwrap_or(Value::Null)).map_err(decode)?;
        let p = Point::read(&get("p").unwrap_or(Value::Null)).map_err(decode)?;
        let gm = ChromeGeometry::read(&get("gm").unwrap_or(Value::Null)).map_err(decode)?;
        let margin = f64::read(&get("margin").unwrap_or(Value::Null)).map_err(decode)?;
        let page_box = Option::<Rect>::from_json(c.get("pageBox")).map_err(decode)?;
        let ids = c.get("inert").filter(|v| !json::undefined(Some(v))).map(IdSet::of_value);
        let v = Option::<ViewEnv>::from_json(c.get("view")).map_err(decode)?;
        out.push(hit_pair(&m, &pg, p, &gm, margin, page_box, ids.as_ref(), v.as_ref()));
    }
    ok_value(Value::Array(out))
}

fn hit_probe(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let pg = page(a, 1)?;
    let selections: Vec<Vec<Id>> = arg(a, 2)?;
    let points: Vec<Point> = arg(a, 3)?;
    let gm: ChromeGeometry = arg(a, 4)?;
    let margin: f64 = arg(a, 5)?;
    let out = selections
        .into_iter()
        .map(|selected| {
            let m = Model { selected: Rc::new(selected), ..m.clone() };
            Value::Array(
                points
                    .iter()
                    .map(|p| hit_pair(&m, &pg, *p, &gm, margin, None, None, None))
                    .collect(),
            )
        })
        .collect();
    ok_value(Value::Array(out))
}

fn hit_test_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ids = inert(a, 6);
    ok_value(target_value(hit_test(
        &m,
        &page(a, 1)?,
        arg(a, 2)?,
        &arg(a, 3)?,
        arg(a, 4)?,
        arg(a, 5)?,
        ids.as_ref(),
        view(a, 7)?.as_ref(),
    )))
}

fn cursor_at_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ids = inert(a, 6);
    ok_value(cursor_value(cursor_at(
        &m,
        &page(a, 1)?,
        arg(a, 2)?,
        &arg(a, 3)?,
        arg(a, 4)?,
        arg(a, 5)?,
        ids.as_ref(),
        view(a, 7)?.as_ref(),
    )))
}

// ── update ──────────────────────────────────────────────────────────────────

/// `apply(model, result)` (`test/support.ts`): the model after a result, or
/// the same model (`None`) when nothing changed.
fn apply(model: &Model, result: &UpdateResult) -> Option<Model> {
    let change = &result.change;
    if change.put.is_empty() && change.drop.is_empty() && model.session().same(&result.session) {
        return None;
    }
    let mut by_id = (*model.by_id).clone();
    for id in &change.drop {
        by_id.shift_remove(id);
    }
    let added: Vec<Id> = change
        .put
        .iter()
        .map(|r| r.id.clone())
        .filter(|id| !model.by_id.contains_key(id))
        .collect();
    for record in &change.put {
        by_id.insert(record.id.clone(), record.clone());
    }
    let mut order: Vec<Id> =
        model.order.iter().filter(|id| by_id.contains_key(*id)).cloned().collect();
    order.extend(added);
    Some(Model::of(result.session.clone(), Rc::new(by_id), Rc::new(order)))
}

/// `runOne(model, msgs)`: per step, the result; then the model and what left.
fn run_one(first: &Model, messages: &[Value]) -> Result<Object, Error> {
    let mut m = first.clone();
    let mut steps = Vec::new();
    for message in messages {
        let message = Message::read(message).map_err(decode)?;
        match update(&m, &message) {
            Ok(result) => {
                let next = apply(&m, &result);
                let same = next.is_none();
                if let Some(next) = next {
                    m = next;
                }
                steps.push(Value::Object(json::object([
                    ("same", Value::Bool(same)),
                    ("effects", result.effects.json()),
                    ("change", Value::Object(result.change.to_object())),
                    ("session", Value::Object(result.session.to_object())),
                ])));
            }
            Err(error) => {
                steps.push(Value::Object(json::object([(
                    "error",
                    canonical_error(&thrown(error)),
                )])));
            }
        }
    }
    let mut model_value = m.session().to_object();
    model_value.insert("order".into(), m.order.json());
    let mut changed = Object::new();
    for (id, record) in m.by_id.iter() {
        let value = record.json();
        let same = first
            .by_id
            .get(id)
            .is_some_and(|before| json::same_value(Some(&before.json()), Some(&value)));
        if !same {
            changed.insert(id.clone(), value);
        }
    }
    model_value.insert("byId".into(), Value::Object(changed));
    let removed: Vec<Value> = first
        .by_id
        .keys()
        .filter(|id| !m.by_id.contains_key(*id))
        .map(|id| Value::String(id.clone()))
        .collect();
    Ok(json::object([
        ("steps", Value::Array(steps)),
        ("model", Value::Object(model_value)),
        ("removed", Value::Array(removed)),
    ]))
}

fn run_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let messages = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(Value::Object(run_one(&m, &messages)?))
}

fn runs_fn(a: &[Value]) -> Out {
    let list = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for entry in list {
        let entry = obj(&entry);
        let m = Model::read(entry.get("model").unwrap_or(&Value::Null)).map_err(decode)?;
        let messages = entry.get("msgs").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut row = json::object([("name", entry.get("name").cloned().unwrap_or(Value::Null))]);
        for (k, v) in run_one(&m, &messages)? {
            row.insert(k, v);
        }
        out.push(Value::Object(row));
    }
    ok_value(Value::Array(out))
}

fn rotate_delta_value(d: RotateDelta) -> Value {
    Value::Object(json::object([
        ("delta", num(d.delta)),
        ("angle", num(d.angle)),
        ("snapped", Value::Bool(d.snapped)),
    ]))
}

fn annots_in_boxes(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let calls = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for call in calls {
        let parts = call.as_array().cloned().unwrap_or_default();
        let pg = PageRef::read(parts.first().unwrap_or(&Value::Null)).map_err(decode)?;
        let rect = Rect::read(parts.get(1).unwrap_or(&Value::Null)).map_err(decode)?;
        let ids = parts.get(2).filter(|v| !json::undefined(Some(v))).map(IdSet::of_value);
        let v = Option::<ViewEnv>::from_json(parts.get(3)).map_err(decode)?;
        out.push(annots_in_box(&m, &pg, rect, ids.as_ref(), v.as_ref()).json());
    }
    ok_value(Value::Array(out))
}

// ── view ────────────────────────────────────────────────────────────────────

fn text_boxes_value(boxes: Vec<TextBox>) -> Value {
    Value::Array(boxes.iter().map(TextBox::to_value).collect())
}

fn knob_offset(a: &[Value], i: usize) -> Result<f64, Error> {
    Ok(arg::<Option<f64>>(a, i)?.unwrap_or(ROTATE_KNOB_OFFSET))
}

fn view_all(
    m: &Model,
    pg: &PageRef,
    page_box: Option<Rect>,
    offset: f64,
    v: Option<&ViewEnv>,
) -> Object {
    json::object([
        ("pageItems", page_items(m, pg, v).json()),
        ("chrome", chrome(m, pg, page_box, offset, v).json()),
        ("selectedItems", selected_items(m, v).json()),
        ("textBoxes", text_boxes_value(text_boxes(m, pg, v))),
        (
            "selectionBoundsOnPage",
            selection_bounds_on_page(m, pg, v).map_or(Value::Null, |r| r.json()),
        ),
        ("selectionKnob", knob_value(selection_knob(m, pg, page_box, offset, v))),
        ("creationDraftAnchor", creation_draft_anchor(m).map_or(Value::Null, |c| c.json())),
    ])
}

fn view_all_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let v = view(a, 4)?;
    ok_value(Value::Object(view_all(&m, &page(a, 1)?, arg(a, 2)?, knob_offset(a, 3)?, v.as_ref())))
}

fn view_many(a: &[Value]) -> Out {
    let calls = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for call in calls {
        let c = obj(&call);
        let m = Model::read(c.get("model").unwrap_or(&Value::Null)).map_err(decode)?;
        let pg = PageRef::read(c.get("page").unwrap_or(&Value::Null)).map_err(decode)?;
        let page_box = Option::<Rect>::from_json(c.get("pageBox")).map_err(decode)?;
        let offset = Option::<f64>::from_json(c.get("knobOffset"))
            .map_err(decode)?
            .unwrap_or(ROTATE_KNOB_OFFSET);
        let v = Option::<ViewEnv>::from_json(c.get("view")).map_err(decode)?;
        let mut row = json::object([("name", c.get("name").cloned().unwrap_or(Value::Null))]);
        for (k, val) in view_all(&m, &pg, page_box, offset, v.as_ref()) {
            row.insert(k, val);
        }
        out.push(Value::Object(row));
    }
    ok_value(Value::Array(out))
}

fn table_lookup<T: Json>(table: Option<&Value>) -> Option<impl Fn(&PageRef) -> Option<T>> {
    let table = table.filter(|v| !json::undefined(Some(v)))?.as_object()?.clone();
    Some(move |page: &PageRef| {
        table
            .get(&js::number_to_string(page.object_number))
            .and_then(|v| T::from_json(Some(v)).ok())
    })
}

fn anchor_value(
    m: &Model,
    boxes: Option<&Value>,
    offsets: Option<&Value>,
    views: Option<&Value>,
) -> Value {
    let page_box = table_lookup::<Rect>(boxes);
    let offset = table_lookup::<f64>(offsets);
    let view_of = table_lookup::<ViewEnv>(views);
    let anchor = selection_anchor(
        m,
        &|p| page_box.as_ref().and_then(|f| f(p)),
        &|p| offset.as_ref().and_then(|f| f(p)),
        &|p| view_of.as_ref().and_then(|f| f(p)),
    );
    match anchor {
        None => Value::Null,
        Some(anchor) => {
            let mut out =
                json::object([("page", anchor.page.json()), ("bounds", anchor.bounds.json())]);
            if let Some(knob) = anchor.knob {
                out.insert("knob".into(), knob.json());
            }
            Value::Object(out)
        }
    }
}

fn selection_anchor_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    ok_value(anchor_value(&m, opt(a, 1), opt(a, 2), opt(a, 3)))
}

fn selection_anchors(a: &[Value]) -> Out {
    let calls = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for call in calls {
        let c = obj(&call);
        let m = Model::read(c.get("model").unwrap_or(&Value::Null)).map_err(decode)?;
        let anchor = anchor_value(&m, c.get("pageBoxes"), c.get("knobOffsets"), c.get("views"));
        out.push(Value::Object(json::object([
            ("name", c.get("name").cloned().unwrap_or(Value::Null)),
            ("anchor", anchor),
        ])));
    }
    ok_value(Value::Array(out))
}

// ── measurement ─────────────────────────────────────────────────────────────

fn appearance(a: &[Value], i: usize) -> Result<MeasurementAppearance, Error> {
    arg(a, i)
}

fn distance_hit_probe(a: &[Value]) -> Out {
    let g: Shape = arg(a, 0)?;
    let ap = appearance(a, 1)?;
    let sw: f64 = arg(a, 2)?;
    let points: Vec<Point> = arg(a, 3)?;
    let margin: f64 = arg(a, 4)?;
    let layout = distance_layout(&g, &ap, sw);
    let caption = layout.as_ref().and_then(|l| l.caption.as_ref());
    let out = points
        .iter()
        .map(|p| {
            Value::Array(vec![
                Value::Bool(distance_caption_hit(caption, *p, margin)),
                Value::Bool(distance_caption_hit(caption, *p, 0.0)),
                layout.as_ref().map_or(Value::Null, |l| {
                    Value::Bool(painted_near(&distance_painted(l, sw), *p, margin))
                }),
            ])
        })
        .collect();
    ok_value(Value::Array(out))
}

fn move_measurement_caption_fn(a: &[Value]) -> Out {
    let g: Shape = arg(a, 0)?;
    let (geometry, measure) =
        move_measurement_caption(&g, &appearance(a, 1)?, arg(a, 2)?, &arg(a, 3)?);
    ok_value(Value::Object(json::object([
        ("geometry", geometry.json()),
        ("measure", measure.json()),
    ])))
}

fn annotation_selection_frame_fn(a: &[Value]) -> Out {
    let record = record_of(a, 0)?;
    let v = view(a, 1)?;
    let live = match opt(a, 2) {
        None => Live::default(),
        Some(value) => {
            let l = obj(value);
            Live {
                geometry: Option::<Shape>::from_json(l.get("geometry")).map_err(decode)?,
                style: Option::<Style>::from_json(l.get("style")).map_err(decode)?,
                measure: Some(
                    Option::<MeasurementAppearance>::from_json(l.get("measure")).map_err(decode)?,
                ),
            }
        }
    };
    ok_value(frame_value(&annotation_selection_frame(&record, v.as_ref(), &live)))
}

// ── rich text ───────────────────────────────────────────────────────────────

fn rich<T: for<'de> Deserialize<'de>>(a: &[Value], i: usize) -> Result<T, Error> {
    plugin_conformance::js::arg(a, i)
}

fn rich_ok<T: serde::Serialize>(value: T) -> Out {
    Ok(plugin_conformance::js::to_value(&value))
}

fn doc(a: &[Value], i: usize) -> Result<richtext::RichTextDocumentInput, Error> {
    rich(a, i)
}

#[derive(Deserialize)]
struct StyleProbe {
    key: String,
    #[serde(default)]
    value: Option<Value>,
}

fn range_has_style_fn(a: &[Value]) -> Out {
    let d = doc(a, 0)?;
    let probe: StyleProbe = rich(a, 2)?;
    let predicate = |delta: &RichTextStyleDelta| {
        let v = plugin_conformance::js::to_value(delta);
        match (v.get(&probe.key), probe.value.as_ref()) {
            (None, None) => true,
            (Some(actual), Some(expected)) => {
                plugin_conformance::canonical::equal(expected, actual)
            }
            _ => false,
        }
    };
    rich_ok(range_has_style(&d.paragraphs, rich(a, 1)?, &predicate))
}

fn apply_style_to_range_fn(a: &[Value]) -> Out {
    let d = doc(a, 0)?;
    let clear: Option<Vec<richtext::RichTextStyleKey>> = rich(a, 3)?;
    let styled =
        apply_style_to_range(&d.paragraphs, rich(a, 1)?, &rich(a, 2)?, &clear.unwrap_or_default());
    rich_ok(match styled {
        Some(paragraphs) => richtext::RichTextDocumentInput { paragraphs, ..d },
        None => d,
    })
}

fn style_at_fn(a: &[Value]) -> Out {
    match style_at(&doc(a, 0)?.paragraphs, rich(a, 1)?) {
        Some(style) => rich_ok(style),
        None => Ok(undefined()),
    }
}

// ── the table ───────────────────────────────────────────────────────────────

pub fn functions() -> FunctionTable {
    vec![
        // geometry basics
        ("RECT_HANDLES", |_| ok(RECT_HANDLES.iter().map(|h| (*h).to_string()).collect::<Vec<_>>())),
        ("ROTATE_KNOB_OFFSET", |_| ok(ROTATE_KNOB_OFFSET)),
        ("DEFAULT_CHROME_GEOM", |_| {
            ok_value(Value::Object(json::object([
                ("handleTol", num(6.0)),
                ("knobTol", num(6.0)),
                ("knobOffset", num(ROTATE_KNOB_OFFSET)),
            ])))
        }),
        ("MITER_LIMIT", |_| ok(MITER_LIMIT)),
        ("rotatedHandleCursor", |a| {
            let handle: String = arg(a, 0)?;
            ok_value(
                rotated_handle_cursor(&handle, arg(a, 1)?)
                    .map_or_else(undefined, |c| Value::String(c.into())),
            )
        }),
        ("rectFromPoints", |a| ok(rect_from_points(arg(a, 0)?, arg(a, 1)?))),
        ("rectsIntersect", |a| ok(rects_intersect(arg(a, 0)?, arg(a, 1)?))),
        ("unionRect", |a| ok(union_rect(&arg::<Vec<Point>>(a, 0)?))),
        ("normalizeDeg", |a| ok(normalize_deg(arg(a, 0)?))),
        ("rotatePoint", |a| ok(rotate_point(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("rotatedAabb", |a| ok(rotated_aabb(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("uprightRotation", |a| ok(upright_rotation(arg(a, 0)?))),
        ("transposedAboutCenter", |a| ok(transposed_about_center(arg(a, 0)?))),
        ("uprightAnchoredRect", |a| {
            ok(upright_anchored_rect(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("fitStampBox", |a| {
            let size = |i: usize| -> Result<Size, Error> {
                let o = opt(a, i).map(obj).unwrap_or_default();
                Ok(Size {
                    width: json::get_num(&o, "width").unwrap_or(f64::NAN),
                    height: json::get_num(&o, "height").unwrap_or(f64::NAN),
                })
            };
            ok(fit_stamp_box(arg(a, 0)?, size(1)?, size(2)?, arg(a, 3)?))
        }),
        ("rectHandlesFor", |a| ok_value(handles_value(&rect_handles_for(arg(a, 0)?)))),
        ("groupResizeAnchor", |a| ok(group_resize_anchor(arg(a, 0)?, &arg::<String>(a, 1)?))),
        ("groupResizeBox", |a| {
            ok(group_resize_box(arg(a, 0)?, &arg::<String>(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("groupResizeFactors", |a| ok_value(group_resize_factors_value(arg(a, 0)?, arg(a, 1)?))),
        ("rotateKnob", |a| {
            let corners: Vec<Point> = arg(a, 0)?;
            let corners: [Point; 4] =
                corners.try_into().map_err(|_| Error::new("[harness] four corners"))?;
            ok_value(knob_value(Some(rotate_knob(&corners, arg(a, 1)?))))
        }),
        ("chordThrough", |a| {
            ok_value(match chord_through(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?) {
                None => Value::Null,
                Some(c) => Value::Object(json::object([("a", c.a.json()), ("b", c.b.json())])),
            })
        }),
        ("placeRotateKnob", |a| {
            let corners: Vec<Point> = arg(a, 0)?;
            let corners: [Point; 4] =
                corners.try_into().map_err(|_| Error::new("[harness] four corners"))?;
            ok_value(knob_value(Some(place_rotate_knob(&corners, arg(a, 1)?, arg(a, 2)?))))
        }),
        ("pointInQuad", |a| {
            let corners: Vec<Point> = arg(a, 1)?;
            let corners: [Point; 4] =
                corners.try_into().map_err(|_| Error::new("[harness] four corners"))?;
            ok(point_in_quad(arg(a, 0)?, &corners))
        }),
        // per-geom
        ("geomBounds", |a| ok(geom_bounds(&arg(a, 0)?))),
        ("geomVisualBounds", |a| ok(geom_visual_bounds(&arg(a, 0)?, stroke(a, 1)?))),
        ("selectionBounds", |a| ok(selection_bounds(&arg(a, 0)?, stroke(a, 1)?))),
        ("selectionQuad", |a| ok_value(points_value(&selection_quad(&arg(a, 0)?, stroke(a, 1)?)))),
        ("obbFromGeom", |a| ok_value(obb_value(obb_from_geom(&arg(a, 0)?, stroke(a, 1)?)))),
        ("turnPivotOf", |a| ok(turn_pivot_of(&arg(a, 0)?))),
        ("geomRotation", |a| ok(geom_rotation(&arg(a, 0)?))),
        ("isRotatableGeom", |a| ok(is_rotatable_geom(&arg(a, 0)?))),
        ("geomHandles", |a| ok_value(handles_value(&geom_handles(&arg(a, 0)?)))),
        ("geomScene", |a| ok(geom_scene(&arg(a, 0)?, stroke(a, 1)?))),
        ("geomTranslate", |a| ok(geom_translate(&arg(a, 0)?, arg(a, 1)?))),
        ("geomRotateAbout", |a| ok(geom_rotate_about(&arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("geomResetRotation", |a| ok(geom_reset_rotation(&arg(a, 0)?, arg(a, 1)?))),
        ("geomScaleAbout", |a| {
            ok(geom_scale_about(&arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))
        }),
        ("geomDragHandle", |a| {
            ok(geom_drag_handle(&arg(a, 0)?, &arg::<String>(a, 1)?, arg(a, 2)?))
        }),
        ("geomHitProbe", geom_hit_probe),
        // text anchors
        ("calloutEnd", |a| {
            ok(callout_end(arg(a, 0)?, arg(a, 1)?, arg::<Option<f64>>(a, 2)?.unwrap_or(0.0)))
        }),
        ("caretRectFromAnchor", |a| ok(caret_rect_from_anchor(&arg(a, 0)?))),
        ("caretFromAnchor", |a| ok(Shape::Caret(caret_from_anchor(&arg(a, 0)?)))),
        // endings
        ("endingPoints", |a| {
            let ending: Option<String> = arg(a, 2)?;
            ok_value(points_value(&ending_points(
                arg(a, 0)?,
                arg(a, 1)?,
                ending.as_deref(),
                arg(a, 3)?,
            )))
        }),
        ("endingNodes", |a| {
            let ending: Option<String> = arg(a, 2)?;
            ok(ending_nodes(arg(a, 0)?, arg(a, 1)?, ending.as_deref(), arg(a, 3)?))
        }),
        // cloudy
        ("cloudyPath", |a| ok(cloudy_path(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        ("cloudyPolyPath", |a| {
            ok(cloudy_poly_path(&arg::<Vec<Point>>(a, 0)?, arg(a, 1)?, arg(a, 2)?))
        }),
        // kinds
        ("KINDS", |_| {
            let table: Object = KIND_NAMES
                .iter()
                .map(|n| {
                    ((*n).to_string(), kind_value(&declared_kind(n).expect("a declared kind")))
                })
                .collect();
            ok_value(Value::Object(table))
        }),
        ("kindNamed", |a| {
            let name: String = arg(a, 0)?;
            if is_prototype_key(&name) {
                return ok_value(prototype_member(&name));
            }
            ok_value(kind_value(&kind_named(&name)))
        }),
        ("propertiesOfKind", |a| {
            let name: String = arg(a, 0)?;
            if is_prototype_key(&name) {
                return Err(type_error("kind.properties is not iterable"));
            }
            ok_value(Value::Array(properties_of(&kind_named(&name))))
        }),
        ("kindTakesLink", |a| {
            let name: String = arg(a, 0)?;
            if is_prototype_key(&name) {
                return Err(type_error("Cannot read properties of undefined (reading 'some')"));
            }
            ok(kind_takes_link(&kind_named(&name)))
        }),
        ("sharedPropertiesOfKinds", |a| {
            let names: Vec<String> = arg(a, 0)?;
            ok_value(Value::Array(shared_properties(
                &names.iter().map(|n| kind_named(n)).collect::<Vec<_>>(),
            )))
        }),
        // flags
        ("NO_ANNOTATION_FLAGS", |_| ok(no_annotation_flags())),
        ("DRAWN_FLAGS", |_| ok(drawn_flags())),
        ("FLAG_KEYS", |_| ok(FLAG_KEYS.iter().map(|k| (*k).to_string()).collect::<Vec<_>>())),
        ("flagsEqual", |a| ok(flags_equal(&annotation_of(a, 0), &annotation_of(a, 1)))),
        ("mergeFlags", |a| ok(merge_flags(&annotation_of(a, 0), &annotation_of(a, 1)))),
        ("viewable", |a| {
            ok(viewable(&annotation_of(a, 0), arg::<Option<bool>>(a, 1)?.unwrap_or(false)))
        }),
        ("interactive", |a| ok(interactive(&annotation_of(a, 0)))),
        ("annotInteractive", |a| ok(annot_interactive(&record_of(a, 0)?))),
        ("annotTransformable", |a| ok(annot_transformable(&record_of(a, 0)?))),
        ("annotDeletable", |a| ok(annot_deletable(&record_of(a, 0)?))),
        ("annotContentsEditable", |a| ok(annot_contents_editable(&record_of(a, 0)?))),
        ("isConversationOnly", |a| ok(is_conversation_only(&record_of(a, 0)?.annotation))),
        ("isAttachedLink", |a| ok(is_attached_link(&record_of(a, 0)?.annotation))),
        ("isSubstrateOnly", |a| ok(is_substrate_only(&record_of(a, 0)?.annotation))),
        // groups and links
        ("groupCaps", |a| ok(group_caps(&model(a, 0)?, &arg::<Vec<Id>>(a, 1)?))),
        ("groupKeyOf", |a| {
            ok_value(
                group_key_of(&model(a, 0)?, &arg::<String>(a, 1)?)
                    .map_or(Value::Null, Value::String),
            )
        }),
        ("groupMembers", |a| ok(group_members(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("expandGroups", |a| ok(expand_groups(&model(a, 0)?, &arg::<Vec<Id>>(a, 1)?))),
        ("linkChildrenOf", |a| {
            let m = model(a, 0)?;
            ok_value(Value::Array(
                link_children_of(&m, &arg::<String>(a, 1)?).iter().map(|r| r.json()).collect(),
            ))
        }),
        ("linkOf", |a| ok_value(link_of(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("groupProbe", group_probe),
        // props
        ("initialTextStyle", |_| ok(initial_text_style())),
        ("styleOf", |a| ok(style_of(&annotation_of(a, 0)))),
        ("textOf", |a| {
            ok_value(text_of(&annotation_of(a, 0)).map_or_else(undefined, |t| t.json()))
        }),
        ("recordViews", record_views),
        ("withValues", with_values_fn),
        ("defaultsFor", defaults_for_fn),
        // anchor
        ("anchorModeOf", |a| {
            ok_value(anchor_mode_of(&record_of(a, 0)?).map_or(Value::Null, |m| m.json()))
        }),
        ("anchorOf", |a| ok(anchor_of(&arg(a, 0)?))),
        ("anchoredGeom", |a| {
            ok(anchored_geom(&arg(a, 0)?, mode_of(a, 1)?.as_ref(), view(a, 2)?.as_ref()))
        }),
        ("unanchoredGeom", |a| {
            ok(unanchored_geom(&arg(a, 0)?, mode_of(a, 1)?.as_ref(), view(a, 2)?.as_ref()))
        }),
        ("anchoredStrokeWidth", |a| {
            ok(anchored_stroke_width(arg(a, 0)?, mode_of(a, 1)?.as_ref(), view(a, 2)?.as_ref()))
        }),
        ("anchoredBox", |a| {
            ok_value(anchored_box_value(anchored_box(
                arg(a, 0)?,
                arg(a, 1)?,
                mode_of(a, 2)?.as_ref(),
                view(a, 3)?.as_ref(),
            )))
        }),
        // snap
        ("computeMoveSnap", |a| {
            let r = compute_move_snap(
                &model(a, 0)?,
                &arg::<Vec<Id>>(a, 1)?,
                &page(a, 2)?,
                arg(a, 3)?,
                arg(a, 4)?,
                arg(a, 5)?,
            );
            ok_value(Value::Object(json::object([
                ("delta", r.delta.json()),
                ("guides", r.guides.json()),
            ])))
        }),
        // hit
        ("paintOrder", |a| ok(paint_order(&model(a, 0)?, &page(a, 1)?))),
        ("isSelectable", |a| ok(is_selectable(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("canMove", |a| ok(can_move(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("selectability", |a| {
            let m = model(a, 0)?;
            let ids: Vec<Id> = arg(a, 1)?;
            ok_value(Value::Array(
                ids.iter()
                    .map(|id| {
                        Value::Array(vec![
                            Value::Bool(is_selectable(&m, id)),
                            Value::Bool(can_move(&m, id)),
                        ])
                    })
                    .collect(),
            ))
        }),
        ("groupUnionBounds", |a| {
            ok_value(
                group_union_bounds(&model(a, 0)?, &page(a, 1)?, view(a, 2)?.as_ref())
                    .map_or(Value::Null, |r| r.json()),
            )
        }),
        ("hitTest", hit_test_fn),
        ("cursorAt", cursor_at_fn),
        ("hitCalls", hit_calls),
        ("hitProbe", hit_probe),
        // scene
        ("scene", |a| ok(scene(&arg::<RenderItem>(a, 0)?))),
        ("blendFor", |a| {
            ok_value(blend_for(&arg::<Style>(a, 0)?).map_or_else(undefined, Value::String))
        }),
        ("layoutRedactLabel", |a| {
            let label: RedactLabel = arg(a, 1)?;
            let text: Option<TextStyle> = arg(a, 2)?;
            ok(layout_redact_label(arg(a, 0)?, &label, text.as_ref()))
        }),
        // placement
        ("clampRectToBox", |a| ok(clamp_rect_to_box(arg(a, 0)?, arg(a, 1)?))),
        ("resolveClickPlacement", |a| {
            let frame = match opt(a, 2) {
                None => PlacementFrame::default(),
                Some(v) => {
                    let f = obj(v);
                    PlacementFrame {
                        page_box: Option::<Rect>::from_json(f.get("pageBox")).map_err(decode)?,
                        upright: Option::<bool>::from_json(f.get("upright")).map_err(decode)?,
                        display_rotation: Option::<f64>::from_json(f.get("displayRotation"))
                            .map_err(decode)?,
                    }
                }
            };
            ok(resolve_click_placement(arg(a, 0)?, &arg(a, 1)?, &frame))
        }),
        ("placedShape", |a| {
            ok_value(
                placed_shape(&annotation_of(a, 0), &arg::<Placement>(a, 1)?)
                    .map_or(Value::Null, |s| s.json()),
            )
        }),
        ("straightenInkStroke", |a| {
            ok_value(points_value(&crate::ink::straighten_ink_stroke(
                &arg::<Vec<Point>>(a, 0)?,
                &arg(a, 1)?,
            )))
        }),
        // view
        ("pageItems", |a| ok(page_items(&model(a, 0)?, &page(a, 1)?, view(a, 2)?.as_ref()))),
        ("chrome", |a| {
            ok(chrome(
                &model(a, 0)?,
                &page(a, 1)?,
                arg(a, 2)?,
                knob_offset(a, 3)?,
                view(a, 4)?.as_ref(),
            ))
        }),
        ("selectedItems", |a| ok(selected_items(&model(a, 0)?, view(a, 1)?.as_ref()))),
        ("textBoxes", |a| {
            ok_value(text_boxes_value(text_boxes(
                &model(a, 0)?,
                &page(a, 1)?,
                view(a, 2)?.as_ref(),
            )))
        }),
        ("selectionBoundsOnPage", |a| {
            ok_value(
                selection_bounds_on_page(&model(a, 0)?, &page(a, 1)?, view(a, 2)?.as_ref())
                    .map_or(Value::Null, |r| r.json()),
            )
        }),
        ("selectionKnob", |a| {
            ok_value(knob_value(selection_knob(
                &model(a, 0)?,
                &page(a, 1)?,
                arg(a, 2)?,
                knob_offset(a, 3)?,
                view(a, 4)?.as_ref(),
            )))
        }),
        ("creationDraftAnchor", |a| {
            ok_value(creation_draft_anchor(&model(a, 0)?).map_or(Value::Null, |c| c.json()))
        }),
        ("selectionAnchor", selection_anchor_fn),
        ("viewMany", view_many),
        ("selectionAnchors", selection_anchors),
        ("viewAll", view_all_fn),
        // update
        ("MIN_DRAG", |_| ok(MIN_DRAG)),
        ("initialModel", |_| ok(initial_model())),
        ("run", run_fn),
        ("runs", runs_fn),
        ("rotateDraftDelta", |a| {
            let m = model(a, 0)?;
            let d: RotateDraft =
                Draft::read(a.get(1).unwrap_or(&Value::Null)).map_err(decode).and_then(|d| {
                    if let Draft::Rotate(r) = d {
                        Ok(r)
                    } else {
                        Err(Error::new("[harness] a rotate draft"))
                    }
                })?;
            ok_value(rotate_delta_value(rotate_draft_delta(&m, &d)))
        }),
        ("rotateDraftDeltas", |a| {
            let m = model(a, 0)?;
            let drafts = a.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
            let mut out = Vec::new();
            for d in drafts {
                let Draft::Rotate(r) = Draft::read(&d).map_err(decode)? else {
                    return Err(Error::new("[harness] a rotate draft"));
                };
                out.push(rotate_delta_value(rotate_draft_delta(&m, &r)));
            }
            ok_value(Value::Array(out))
        }),
        ("calloutBox", |a| ok(callout_box(&callout_draft(a)?))),
        ("calloutUprightRot", |a| ok(callout_upright_rot(&callout_draft(a)?))),
        ("annotsInBox", |a| {
            let ids = inert(a, 3);
            ok(annots_in_box(
                &model(a, 0)?,
                &page(a, 1)?,
                arg(a, 2)?,
                ids.as_ref(),
                view(a, 4)?.as_ref(),
            ))
        }),
        ("annotsInBoxes", annots_in_boxes),
        // measurement
        ("distanceCaptionWidth", |a| ok(distance_caption_width(&arg::<String>(a, 0)?))),
        ("distanceLabel", |a| ok(distance_label(&arg(a, 0)?, &appearance(a, 1)?))),
        ("distanceLayout", |a| {
            ok_value(
                distance_layout(&arg(a, 0)?, &appearance(a, 1)?, arg(a, 2)?)
                    .map_or(Value::Null, |l| distance_layout_value(&l)),
            )
        }),
        ("expandDistanceBounds", |a| ok(expand_distance_bounds(arg(a, 0)?, arg(a, 1)?))),
        ("distanceSelectionQuad", |a| {
            ok_value(points_value(&distance_selection_quad(
                &arg(a, 0)?,
                &appearance(a, 1)?,
                arg(a, 2)?,
            )))
        }),
        ("distanceHandles", |a| {
            ok_value(
                distance_layout(&arg(a, 0)?, &appearance(a, 1)?, arg(a, 2)?)
                    .map_or(Value::Null, |l| handles_value(&distance_handles(&l))),
            )
        }),
        ("distanceHitProbe", distance_hit_probe),
        ("distanceCaptionAt", |a| {
            ok_value(
                distance_caption_at(&arg(a, 0)?, &appearance(a, 1)?, arg(a, 2)?)
                    .map_or(Value::Null, |p| p.json()),
            )
        }),
        ("distanceLeaderLength", |a| ok(distance_leader_length(&arg(a, 0)?, arg(a, 1)?))),
        ("moveDistanceCaption", |a| {
            ok(move_distance_caption(&arg(a, 0)?, &appearance(a, 1)?, arg(a, 2)?))
        }),
        ("distanceScene", |a| ok(distance_scene(&arg(a, 0)?, &appearance(a, 1)?, &arg(a, 2)?))),
        ("measurementCaptionScene", |a| {
            let g: Shape = arg(a, 0)?;
            let s: Style = arg(a, 2)?;
            let layout = measurement_layout(&g, &appearance(a, 1)?, &s);
            ok(measurement_caption_scene(layout.as_ref().and_then(MeasurementLayout::caption), &s))
        }),
        ("shapeMeasurementReadout", |a| {
            ok_value(shape_measurement_readout(&arg(a, 0)?, &appearance(a, 1)?).to_value())
        }),
        ("shapeMeasurementLabel", |a| ok(shape_measurement_label(&arg(a, 0)?, &appearance(a, 1)?))),
        ("automaticShapeCaptionCenter", |a| {
            ok(automatic_shape_caption_center(&arg::<Vec<Point>>(a, 0)?, arg(a, 1)?))
        }),
        ("shapeMeasurementLayout", |a| {
            ok_value(
                shape_measurement_layout(&arg(a, 0)?, &appearance(a, 1)?, &arg(a, 2)?)
                    .map_or(Value::Null, |l| shape_layout_value(&l)),
            )
        }),
        ("measurementLayout", |a| {
            ok_value(measurement_layout_value(measurement_layout(
                &arg(a, 0)?,
                &appearance(a, 1)?,
                &arg(a, 2)?,
            )))
        }),
        ("measurementSelectionQuad", |a| {
            ok_value(points_value(&measurement_selection_quad(
                &arg(a, 0)?,
                &appearance(a, 1)?,
                &arg(a, 2)?,
            )))
        }),
        ("moveMeasurementCaption", move_measurement_caption_fn),
        ("annotationSelectionFrame", annotation_selection_frame_fn),
        ("textPlateInset", |a| ok(text_plate_inset(arg(a, 0)?))),
        ("pointInPoly", |a| ok(point_in_poly(arg(a, 0)?, &arg::<Vec<Point>>(a, 1)?))),
        // rich text
        ("richTextLength", |a| rich_ok(rich_text_length(&doc(a, 0)?.paragraphs))),
        ("sameStyleDelta", |a| {
            let x: Option<RichTextStyleDelta> = rich(a, 0)?;
            let y: Option<RichTextStyleDelta> = rich(a, 1)?;
            rich_ok(same_style_delta(x.as_ref(), y.as_ref()))
        }),
        ("normalizeRuns", |a| rich_ok(richtext::normalize_doc(&doc(a, 0)?))),
        ("locateOffset", |a| rich_ok(locate_offset(&doc(a, 0)?.paragraphs, rich(a, 1)?))),
        ("splitRunsAt", |a| rich_ok(split_runs_at(&rich(a, 0)?, rich(a, 1)?))),
        ("applyStyleToRange", apply_style_to_range_fn),
        ("styleAt", style_at_fn),
        ("rangeHasStyle", range_has_style_fn),
        ("paragraphsFromPlainText", |a| rich_ok(paragraphs_from_plain_text(&arg::<String>(a, 0)?))),
        ("plainTextOf", |a| rich_ok(plain_text_of(&doc(a, 0)?.paragraphs))),
        ("isPlainRichText", |a| rich_ok(is_plain_rich_text(&doc(a, 0)?))),
    ]
}

fn callout_draft(a: &[Value]) -> Result<CreateCallout, Error> {
    let value = a.first().cloned().unwrap_or(Value::Null);
    match Draft::read(&value).map_err(decode)? {
        Draft::CreateCallout(d) => Ok(d),
        _ => Err(Error::new("[harness] a callout draft")),
    }
}
