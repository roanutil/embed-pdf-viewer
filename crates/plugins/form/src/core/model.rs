//! `core/model.ts`: the pure heart of the plugin. The model mirrors engine
//! truth (the snapshot, verbatim JSON) plus transient write status and the
//! widget geometry the shell feeds in.
//!
//! The TypeScript model is keyed JavaScript objects. A numeric key is the
//! number's string form, so the Rust maps key on
//! [`epdf_geometry::js::number_to_string`] of the object number.

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_geometry::js::number_to_string;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::value::{get, string_of};

/// A content-space box (top-left origin, y-down, PDF points): the
/// TypeScript `Box`.
pub type Box = Rect;

/// Stable client key for a field: object number when durable, else FQN.
pub type FieldKey = String;

/// The JavaScript property key a number becomes.
pub fn number_key(n: f64) -> String {
    number_to_string(n)
}

/// `fieldKeyOf(field)`.
pub fn field_key_of(field: &Value) -> FieldKey {
    let number = get(field, "fieldObjectNumber").and_then(Value::as_f64);
    match number {
        Some(n) if n > 0.0 => format!("obj:{}", number_to_string(n)),
        _ => format!("fqn:{}", string_of(get(field, "name"))),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// Engine truth; replaced wholesale on refresh, patched on write-backs.
    pub snapshot: Option<Value>,
    /// fieldKey to index into `snapshot.fields`.
    pub by_key: BTreeMap<FieldKey, f64>,
    /// annotObjectNumber (as a property key) to index into `snapshot.fields`.
    pub by_widget: BTreeMap<String, f64>,
    /// Fields with an in-flight engine write.
    pub writing: BTreeMap<FieldKey, bool>,
    /// Widget geometry per page: pon, then annotObjectNumber, to a box.
    pub geom: BTreeMap<String, BTreeMap<String, Box>>,
    /// Bumped on every change.
    pub seq: f64,
}

/// `initialModel()`.
pub fn initial_model() -> Model {
    Model::default()
}

/// `Msg`, the model's inputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Msg {
    Snapshot {
        snapshot: Value,
    },
    WriteStart {
        key: FieldKey,
    },
    WriteDone {
        key: FieldKey,
        field: Value,
    },
    WriteFailed {
        key: FieldKey,
    },
    PageGeom {
        page_object_number: f64,
        boxes: BTreeMap<String, Box>,
    },
    ClearGeom {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_object_number: Option<f64>,
    },
}

fn fields_of(snapshot: &Value) -> &[Value] {
    get(snapshot, "fields").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

type Index = (BTreeMap<FieldKey, f64>, BTreeMap<String, f64>);

fn index(snapshot: &Value) -> Index {
    let mut by_key = BTreeMap::new();
    let mut by_widget = BTreeMap::new();
    // `forEach` skips the holes a past-the-end write leaves, which the
    // Rust model holds as nulls.
    for (i, field) in fields_of(snapshot).iter().enumerate().filter(|(_, f)| !f.is_null()) {
        by_key.insert(field_key_of(field), i as f64);
        for w in get(field, "widgets").and_then(Value::as_array).into_iter().flatten() {
            if let Some(n) = get(w, "annotObjectNumber").and_then(Value::as_f64)
                && n > 0.0
            {
                by_widget.insert(number_key(n), i as f64);
            }
        }
    }
    (by_key, by_widget)
}

/// `update(model, msg)`. Returns the model itself (the same `Rc`) where the
/// TypeScript returns `model`.
pub fn update(model: &Rc<Model>, msg: &Msg) -> Rc<Model> {
    let seq = model.seq + 1.0;
    match msg {
        Msg::Snapshot { snapshot } => {
            let (by_key, by_widget) = index(snapshot);
            Rc::new(Model {
                snapshot: Some(snapshot.clone()),
                by_key,
                by_widget,
                writing: BTreeMap::new(),
                geom: model.geom.clone(),
                seq,
            })
        }
        Msg::WriteStart { key } => {
            let mut writing = model.writing.clone();
            writing.insert(key.clone(), true);
            Rc::new(Model { writing, seq, ..(**model).clone() })
        }
        Msg::WriteDone { key, field } => {
            let Some(current) = &model.snapshot else { return model.clone() };
            let Some(&i) = model.by_key.get(key) else { return model.clone() };
            let mut fields = fields_of(current).to_vec();
            let i = i as usize;
            // `fields[i] = field` past the end pads with holes, which JSON
            // prints as null.
            fields.resize(fields.len().max(i + 1), Value::Null);
            fields[i] = field.clone();
            let mut snapshot = current.clone();
            if let Value::Object(members) = &mut snapshot {
                members.insert("fields".into(), Value::Array(fields));
            }
            let mut writing = model.writing.clone();
            writing.remove(key);
            let (by_key, by_widget) = index(&snapshot);
            Rc::new(Model {
                snapshot: Some(snapshot),
                by_key,
                by_widget,
                writing,
                geom: model.geom.clone(),
                seq,
            })
        }
        Msg::WriteFailed { key } => {
            let mut writing = model.writing.clone();
            writing.remove(key);
            Rc::new(Model { writing, seq, ..(**model).clone() })
        }
        Msg::PageGeom { page_object_number, boxes } => {
            let mut geom = model.geom.clone();
            geom.insert(number_key(*page_object_number), boxes.clone());
            Rc::new(Model { geom, seq, ..(**model).clone() })
        }
        Msg::ClearGeom { page_object_number } => {
            let geom = match page_object_number {
                None => BTreeMap::new(),
                Some(pon) => {
                    let mut geom = model.geom.clone();
                    geom.remove(&number_key(*pon));
                    geom
                }
            };
            Rc::new(Model { geom, seq, ..(**model).clone() })
        }
    }
}

fn field_at(model: &Model, i: Option<&f64>) -> Option<Value> {
    let i = *i?;
    let snapshot = model.snapshot.as_ref()?;
    // `snapshot.fields[i] ?? null`: a hole or an index past the end is null.
    fields_of(snapshot).get(i as usize).filter(|f| !f.is_null()).cloned()
}

/// `fieldByKey(model, key)`.
pub fn field_by_key(model: &Model, key: &str) -> Option<Value> {
    field_at(model, model.by_key.get(key))
}

/// `fieldForWidget(model, annotObjectNumber)`.
pub fn field_for_widget(model: &Model, annot_object_number: f64) -> Option<Value> {
    field_at(model, model.by_widget.get(&number_key(annot_object_number)))
}

/// `WidgetHit`: the annotation under a point and the field it belongs to.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetHit {
    pub annot_object_number: f64,
    pub field: Value,
    /// The widget's page-space box.
    #[serde(rename = "box")]
    pub bx: Box,
}

/// `contains(b, p)`: edges are inside.
pub fn contains(b: &Box, x: f64, y: f64) -> bool {
    x >= b.x && x <= b.x + b.width && y >= b.y && y <= b.y + b.height
}

/// `widgetAt(model, pageObjectNumber, point)`: the widget under a page
/// point from the model's loaded geometry for that page, the smallest
/// containing box winning. `Object.entries` visits integer keys in numeric
/// order, so the boxes are visited that way here too.
pub fn widget_at(model: &Model, page_object_number: f64, x: f64, y: f64) -> Option<WidgetHit> {
    let geom = model.geom.get(&number_key(page_object_number))?;
    let mut entries: Vec<(f64, &Box)> =
        geom.iter().map(|(k, b)| (k.parse::<f64>().unwrap_or(f64::NAN), b)).collect();
    entries.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<WidgetHit> = None;
    for (annot_object_number, bx) in entries {
        if !contains(bx, x, y) {
            continue;
        }
        let Some(field) = field_for_widget(model, annot_object_number) else { continue };
        if best.as_ref().is_none_or(|b| bx.width * bx.height < b.bx.width * b.bx.height) {
            best = Some(WidgetHit { annot_object_number, field, bx: *bx });
        }
    }
    best
}
