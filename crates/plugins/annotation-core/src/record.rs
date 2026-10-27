//! `record/`: one annotation as the core works on it. The engine's
//! annotation is its only data; this reads it (who it is, its shape, how it
//! is drawn, how its text is set), writes engine fields back
//! ([`with_values`]), and reads it as the engine writes a create or an edit
//! ([`annotation_of_new`], [`annotation_after`]).

use epdf_geometry::quad_bounds;
use serde_json::Value;

use crate::engine::appearance::{AppearanceChange, appearance_change_of};
use crate::engine::facts::{
    annotation_defaults_of, field_names, is_drawn_rect_kind, is_unbaked_kind,
};
use crate::engine::geom::appearance_turn_of;
use crate::engine::resolve::{
    DraftContext, annotation_of_draft, annotation_patch_between, apply_annotation_patch,
    merge_annotation_patch, resolve_annotation_patch, resolve_rect_command,
};
use crate::error::CoreResult;
use crate::flags::may_write;
use crate::geometry::{geom_rotation, selection_quad};
use crate::json::{self, Object};
use crate::kinds::{AnnotationKind, field_family_of_kind, kind_named, widget_kind_of};
use crate::measurement::{distance_drawn_bounds, distance_layout, measurement_of};
use crate::measurement_shape::shape_measurement_layout;
use crate::rect::{rect_corner_points, rotated_aabb, union_rect};
use crate::shapes;
use crate::types::{
    Annotation, AnnotationDraft, AnnotationPatch, FieldValues, Id, ModelAnnotation, Rect, Shape,
    Source, Style, TextStyle,
};

// ── identity ────────────────────────────────────────────────────────────────

/// `annotationKey(ref)`: `obj:<n>` or `base:<page>:<i>`. A ref of a kind no
/// key is defined for has none: the TypeScript keys it `undefined`, which
/// would collapse every such record into one (review finding N6, a kept
/// Rust-only refusal).
pub fn annotation_key(reference: &Value) -> Option<String> {
    let number = |v: Option<&Value>| v.and_then(json::number).map(crate::js::number_to_string);
    match reference.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => number(reference.get("objectNumber")).map(|n| format!("obj:{n}")),
        Some("baseIndex") => {
            let page = number(reference.get("page").and_then(|p| p.get("objectNumber")))?;
            number(reference.get("baseIndex")).map(|i| format!("base:{page}:{i}"))
        }
        _ => None,
    }
}

/// `kindOf(annotation)`: the kind its subtype, field family or intent names.
pub fn kind_of(annotation: &Object) -> AnnotationKind {
    match json::get_str(annotation, "subtype") {
        Some("widget") => {
            let family = json::get_str(annotation, "fieldFamily").unwrap_or("undefined");
            kind_named(widget_kind_of(family))
        }
        Some("free-text") => {
            kind_named(if json::get_str(annotation, "intent") == Some("free-text-callout") {
                "free-text-callout"
            } else {
                "free-text"
            })
        }
        Some(subtype) => kind_named(subtype),
        None => kind_named("undefined"),
    }
}

/// `refOf(record)`: a record's engine ref.
pub fn ref_of(record: Option<&ModelAnnotation>) -> Option<Value> {
    record.and_then(|r| json::get(&r.annotation, "ref").cloned())
}

/// `irtOf(annotation)`: the key of the annotation it answers.
pub fn irt_of(annotation: &Object) -> Option<Id> {
    json::get(annotation, "reply").and_then(|reply| reply.get("to")).and_then(annotation_key)
}

/// `groupOf(annotation)`: the key of its group's primary, for a `/RT /Group` member.
pub fn group_of(annotation: &Object) -> Option<Id> {
    let reply = json::get(annotation, "reply")?;
    if reply.get("type").and_then(Value::as_str) != Some("group") {
        return None;
    }
    reply.get("to").and_then(annotation_key)
}

// ── views ───────────────────────────────────────────────────────────────────

/// `shapeOf(annotation)`: its shape, read by its kind's family.
pub fn shape_of(annotation: &Object) -> Shape {
    kind_of(annotation).family.read(annotation)
}

/// `styleOf(annotation)`: how it is drawn.
pub fn style_of(annotation: &Object) -> Style {
    kind_of(annotation).style_of(annotation)
}

/// `textOf(annotation)`: how its text is set, for a kind with text.
pub fn text_of(annotation: &Object) -> Option<TextStyle> {
    kind_of(annotation).text_of(annotation)
}

// ── writes ──────────────────────────────────────────────────────────────────

/// `withValues(record, values)`: the record with engine `values` written, as
/// far as it takes them. Only the fields its kind declares and this session
/// may change now; a `rect` resolved as the engine resolves it (one the
/// engine would refuse is an error). The same record (`None`) when nothing
/// changes.
pub fn with_values(
    record: &ModelAnnotation,
    values: &FieldValues,
) -> CoreResult<Option<ModelAnnotation>> {
    let subtype = record.subtype().to_string();
    let declared = field_names(&subtype);
    let mut taken = Object::new();
    for (field, value) in values {
        if json::undefined(Some(value)) || field == "subtype" || !declared.contains(&field.as_str())
        {
            continue;
        }
        if may_write(record, field) {
            taken.insert(field.clone(), value.clone());
        }
    }
    if taken.is_empty() {
        return Ok(None);
    }
    taken.insert("subtype".into(), Value::String(subtype));
    let stated = resolve_rect_command(&record.annotation, &taken)?;
    let annotation = merge_annotation_patch(&record.annotation, &stated);
    if annotation_patch_between(&record.annotation, &annotation).is_empty() {
        return Ok(None);
    }
    Ok(Some(record.with_annotation(annotation)))
}

/// [`with_values`], the record itself when nothing changes.
pub fn with_values_or_same(
    record: &ModelAnnotation,
    values: &FieldValues,
) -> CoreResult<ModelAnnotation> {
    Ok(with_values(record, values)?.unwrap_or_else(|| record.clone()))
}

/// `withShape(record, shape)`: the engine fields that state `shape`, written through `withValues`.
pub fn with_shape(record: &ModelAnnotation, shape: &Shape) -> CoreResult<Option<ModelAnnotation>> {
    with_values(record, &shapes::write(shape, record.subtype()))
}

// ── links ───────────────────────────────────────────────────────────────────

/// `linkChildRects(shape, style)`: the rects a parent's attached links take.
pub fn link_child_rects(shape: &Shape, style: &Style) -> Vec<Rect> {
    match shape {
        Shape::Quads(s) => s.quad_points.iter().map(|q| quad_bounds(*q)).collect(),
        Shape::TextBox(s) => vec![rotated_aabb(s.box_, s.rotation, None)],
        _ => vec![union_rect(&selection_quad(shape, style.stroke()))],
    }
}

/// `writableTarget(target)`: `goto` and `uri` pass through; the rest are `null`.
pub fn writable_target(target: Option<&Value>) -> Value {
    match target {
        Some(t) if matches!(t.get("kind").and_then(Value::as_str), Some("goto" | "uri")) => {
            t.clone()
        }
        _ => Value::Null,
    }
}

// ── as the engine writes it ─────────────────────────────────────────────────

/// The upright page box around all an annotation paints.
fn drawn_bounds_of(annotation: &Object) -> Rect {
    let shape = shape_of(annotation);
    let style = style_of(annotation);
    let bounds = shapes::rect(&shape, style.stroke());
    let Some(measure) = measurement_of(annotation) else { return bounds };
    if measure.is_distance() {
        return match distance_layout(&shape, &measure, style.stroke_width) {
            Some(layout) => distance_drawn_bounds(&layout, style.stroke_width),
            None => bounds,
        };
    }
    match shape_measurement_layout(&shape, &measure, &style).and_then(|l| l.caption) {
        Some(caption) => {
            let mut points = rect_corner_points(bounds).to_vec();
            points.extend(caption.bounds.iter().copied());
            union_rect(&points)
        }
        None => bounds,
    }
}

/// `annotationOfNew(draft, at)`: the annotation a draft creates, as the
/// engine reads it back, a drawn kind's `rect` the box around its drawing.
pub fn annotation_of_new(draft: &AnnotationDraft, reference: &Value) -> CoreResult<Object> {
    let context = DraftContext { reference: reference.clone(), ..Default::default() };
    let mut annotation = annotation_of_draft(draft, &context)?;
    if is_drawn_rect_kind(json::get_str(&annotation, "subtype").unwrap_or("")) {
        let rect = drawn_bounds_of(&annotation);
        annotation.insert("rect".into(), json::rect_value(rect));
    }
    Ok(annotation)
}

/// `annotationAfter(annotation, patch)`: the annotation after `patch`, as the engine reads it back.
pub fn annotation_after(annotation: &Object, patch: &AnnotationPatch) -> CoreResult<Object> {
    let mut after = apply_annotation_patch(annotation, patch, None)?;
    let change =
        appearance_change_of(annotation, &resolve_annotation_patch(annotation, patch, None)?);
    let subtype = json::get_str(&after, "subtype").unwrap_or("").to_string();
    match change {
        AppearanceChange::Inert => Ok(after),
        AppearanceChange::Translation { x, y } => {
            if !is_drawn_rect_kind(&subtype) {
                return Ok(after);
            }
            let mut rect =
                annotation.get("rect").and_then(Value::as_object).cloned().unwrap_or_default();
            let rx = json::get_num(&rect, "x").unwrap_or(f64::NAN);
            let ry = json::get_num(&rect, "y").unwrap_or(f64::NAN);
            rect.insert("x".into(), json::num(rx + x));
            rect.insert("y".into(), json::num(ry + y));
            after.insert("rect".into(), Value::Object(rect));
            Ok(after)
        }
        AppearanceChange::Regenerate => {
            if !is_unbaked_kind(&subtype) {
                after.insert("hasAppearance".into(), Value::Bool(true));
            }
            if is_drawn_rect_kind(&subtype) {
                let rect = drawn_bounds_of(&after);
                after.insert("rect".into(), json::rect_value(rect));
            }
            Ok(after)
        }
    }
}

// ── defaults ────────────────────────────────────────────────────────────────

/// `engineSubtypeOf(kind)`: a callout is a free text, a form tool's kind a widget.
pub fn engine_subtype_of(kind: &str) -> String {
    if kind == "free-text-callout" {
        "free-text".into()
    } else if kind.starts_with("widget") {
        "widget".into()
    } else {
        kind.into()
    }
}

/// `readOfDefaults(kind, defaults)`: a tool's defaults over the engine's for `kind`.
pub fn read_of_defaults(kind: &str, defaults: &FieldValues) -> Object {
    let subtype = engine_subtype_of(kind);
    let mut out = json::object([("subtype", Value::String(subtype.clone()))]);
    for (k, v) in annotation_defaults_of(&subtype) {
        out.insert(k, v);
    }
    if subtype == "widget"
        && let Some(family) = field_family_of_kind(kind)
    {
        out.insert("fieldFamily".into(), Value::String(family.into()));
    }
    for (k, v) in defaults {
        out.insert(k.clone(), v.clone());
    }
    out
}

const WIDGET_APPEARANCE_FIELDS: [&str; 8] = [
    "color",
    "interiorColor",
    "strokeWidth",
    "borderStyle",
    "fontFamily",
    "fontSize",
    "fontColor",
    "textAlign",
];

/// `widgetAppearanceOf(defaults)`: a form tool's defaults as the engine's widget appearance.
pub fn widget_appearance_of(defaults: &FieldValues) -> Object {
    WIDGET_APPEARANCE_FIELDS
        .iter()
        .filter_map(|name| {
            defaults
                .get(*name)
                .filter(|v| !json::undefined(Some(v)))
                .map(|v| ((*name).to_string(), v.clone()))
        })
        .collect()
}

/// `fromDTO(dto)`: the record an engine annotation reads as, drawn from its raster.
pub fn from_dto(dto: Annotation) -> Option<ModelAnnotation> {
    let id = annotation_key(dto.get("ref")?)?;
    let subtype = json::get_str(&dto, "subtype").unwrap_or("").to_string();
    let rect = json::get(&dto, "rect").map(json::rect_loose);
    let turned_box = json::get(&dto, "box").map(json::rect_loose);
    let stripped = if dto.contains_key("box") {
        appearance_turn_of(
            &subtype,
            rect.unwrap_or_default(),
            turned_box,
            json::get_num(&dto, "rotation"),
            json::get_str(&dto, "intent"),
        )
        .and(turned_box)
    } else {
        None
    };
    let ap_rot = stripped.map(|_| geom_rotation(&shape_of(&dto)));
    let mut record = ModelAnnotation {
        id,
        unconfirmed: None,
        source: Source::Baked,
        ap_box: stripped.or(rect),
        ap_rot,
        picture_version: None,
        painter: Default::default(),
        authority: None,
        annotation: dto,
        order: Default::default(),
    };
    let mut order = vec!["id", "source", "annotation", "apBox"];
    if record.ap_rot.is_some() {
        order.push("apRot");
    }
    record = record.ordered(&order);
    Some(record)
}
