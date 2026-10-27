//! `update/changes.ts`: what every transition that changes records shares:
//! a new record and the draft it is written from, a view-space gesture's
//! commit, and removing records.

use std::rc::Rc;

use serde_json::Value;

use crate::anchor::{anchor_mode_of, anchored_geom, unanchored_geom};
use crate::appearance::source_of_new;
use crate::engine::facts::field_names;
use crate::error::{CoreError, CoreResult};
use crate::flags::drawn_flags;
use crate::json::{self, Object, num};
use crate::object::{Json, same_json};
use crate::record::{annotation_key, annotation_of_new, engine_subtype_of, shape_of};
use crate::shapes;
use crate::types::{
    AnnotationDraft, FieldValues, Id, Model, ModelAnnotation, PageRef, Point, Shape, Source,
    ViewEnv,
};
use crate::update::session::forget;

/// `isPolySubtype(subtype)`.
pub fn is_poly_subtype(subtype: &str) -> bool {
    subtype == "polygon" || subtype == "polyline"
}

/// `sub(from, to)`.
pub fn sub(from: Point, to: Point) -> Point {
    Point { x: from.x - to.x, y: from.y - to.y }
}

/// `commitViewGesture(record, view, op)`: apply `op` to the projected
/// geometry and map the result back to stored space.
pub fn commit_view_gesture(
    record: &ModelAnnotation,
    view: Option<&ViewEnv>,
    op: impl FnOnce(&Shape) -> Shape,
) -> Shape {
    let mode = anchor_mode_of(record);
    unanchored_geom(
        &op(&anchored_geom(&shape_of(&record.annotation), mode.as_ref(), view)),
        mode.as_ref(),
        view,
    )
}

/// `geomEqual(left, right)`: `JSON.stringify` equality.
pub fn geom_equal(left: &Shape, right: &Shape) -> bool {
    same_json(left, right)
}

/// The ref of the `offset`-th record a message creates.
fn new_record_ref(model: &Model, page: &PageRef, offset: usize) -> CoreResult<Value> {
    let Some(number) = model.object_numbers.get(offset - 1).copied() else {
        return Err(CoreError::plain(
            "annotation-core: the session holds no object number for a new record",
        ));
    };
    Ok(Value::Object(json::object([
        ("kind", Value::String("objectNumber".into())),
        ("page", page.json()),
        ("objectNumber", num(number)),
    ])))
}

/// `numbersLeft(model, count)`: a new array, as `slice` makes one.
pub fn numbers_left(model: &Model, count: usize) -> Rc<Vec<f64>> {
    Rc::new(model.object_numbers.iter().skip(count).copied().collect())
}

/// `draftOf(kind, defaults, shape, fields, flags)`: the draft a drawing creates.
pub fn draft_of(
    kind: &str,
    defaults: &FieldValues,
    shape: &Shape,
    fields: &FieldValues,
    flags: Option<&Object>,
) -> AnnotationDraft {
    let subtype = engine_subtype_of(kind);
    let declared = field_names(&subtype);
    let mut out: Object = defaults
        .iter()
        .filter(|(name, _)| declared.contains(&name.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in shapes::write(shape, &subtype) {
        out.insert(k, v);
    }
    for (k, v) in fields {
        out.insert(k.clone(), v.clone());
    }
    for (k, v) in drawn_flags() {
        out.insert(k, v);
    }
    if let Some(flags) = flags {
        for (k, v) in flags {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert("subtype".into(), Value::String(subtype));
    out
}

/// A record a message creates, and the draft it is written from (`NewRecord`).
#[derive(Clone, Debug)]
pub struct NewRecord {
    pub record: ModelAnnotation,
    pub draft: AnnotationDraft,
}

/// `newRecord(model, page, draft, { offset, reply })`.
pub fn new_record(
    model: &Model,
    page: &PageRef,
    draft: AnnotationDraft,
    offset: usize,
    reply: Option<Value>,
) -> CoreResult<NewRecord> {
    let reference = new_record_ref(model, page, offset)?;
    let mut annotation = annotation_of_new(&draft, &reference)?;
    let mut draft = draft;
    if let Some(reply) = reply {
        annotation.insert("reply".into(), reply.clone());
        draft.insert("reply".into(), reply);
    }
    let id = annotation_key(&reference).expect("an object-number ref has a key");
    let source: Source = source_of_new(&annotation);
    let record = ModelAnnotation {
        id,
        unconfirmed: Some(true),
        source,
        ap_box: None,
        ap_rot: None,
        picture_version: None,
        painter: Default::default(),
        authority: None,
        annotation: Rc::new(annotation),
        order: Default::default(),
    }
    .ordered(&["id", "unconfirmed", "source", "annotation"]);
    Ok(NewRecord { record, draft })
}

/// `withoutRecords(model, ids)`: the model without these records, or any session reference to them.
pub fn without_records(model: &Model, ids: &[Id]) -> Model {
    let gone: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let by_id: crate::types::ById = model
        .by_id
        .iter()
        .filter(|(id, _)| !gone.contains(id.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let order: Vec<Id> =
        model.order.iter().filter(|id| !gone.contains(id.as_str())).cloned().collect();
    forget(&Model { by_id: Rc::new(by_id), order: Rc::new(order), ..model.clone() }, ids)
}

/// The model with `record` put in `byId` and appended to `order` (a new record).
pub fn with_new_record(model: &Model, record: ModelAnnotation) -> Model {
    let id = record.id.clone();
    let mut by_id = (*model.by_id).clone();
    by_id.insert(id.clone(), Rc::new(record));
    let mut order = (*model.order).clone();
    order.push(id);
    Model { by_id: Rc::new(by_id), order: Rc::new(order), ..model.clone() }
}

/// The model with `record` replacing (or added to) `byId`, `order` as it is.
pub fn with_record(model: &Model, record: Rc<ModelAnnotation>) -> Model {
    let mut by_id = (*model.by_id).clone();
    by_id.insert(record.id.clone(), record);
    Model { by_id: Rc::new(by_id), ..model.clone() }
}
