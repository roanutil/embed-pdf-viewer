//! `core/fill-items.ts`: the fill-mode projection, what a framework paints
//! for one page, in content space. Geometry comes from the widget plane via
//! `pageGeom`; the field plane contributes identity, value and behaviour.
//!
//! Members the item copies from the field DTO stay JSON and keep the DTO's
//! `undefined` for an absent member, which a printed item drops.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::model::{Box, FieldKey, Model, field_for_widget, field_key_of, number_key};
use crate::value::{get, member, num, str_of, truthy};

/// `FillItem`: the control-independent members, then the control.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillItem {
    pub key: FieldKey,
    /// Widget identity: the join to the annotation plane.
    pub annot_object_number: f64,
    #[serde(rename = "box")]
    pub bx: Box,
    /// Read-only field or write in flight: render, don't accept input.
    pub disabled: bool,
    /// Accessible name: `/TU` when present, else the fully qualified name.
    pub label: Value,
    #[serde(flatten)]
    pub control: FillControl,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "control", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FillControl {
    Text { value: Value, multiline: Value, password: Value, max_length: Value, comb: Value },
    Toggle { kind: ToggleKind, checked: Value, on_state: Value },
    Choice { kind: ChoiceKind, edit: Value, multi: Value, options: Value, selected: Value },
    Button,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToggleKind {
    Checkbox,
    Radio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChoiceKind {
    Combo,
    List,
}

const ZERO_BOX: Box = Box { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };

fn widget_of(field: &Value, annot_object_number: f64) -> Option<&Value> {
    get(field, "widgets")
        .and_then(Value::as_array)?
        .iter()
        .find(|w| num(w, "annotObjectNumber") == Some(annot_object_number))
}

/// `toggle && key in toggle ? toggle[key] : fallback`.
fn toggle_member(toggle: Option<&Value>, key: &str, fallback: Value) -> Value {
    match toggle.and_then(|t| t.get(key)) {
        Some(value) => value.clone(),
        None => fallback,
    }
}

/// `projectWidget(model, field, annotObjectNumber, box = ZERO_BOX)`: one
/// widget as a fill control, `None` for families with no fill control.
pub fn project_widget(
    model: &Model,
    field: &Value,
    annot_object_number: f64,
    bx: Option<Box>,
) -> Option<FillItem> {
    let key = field_key_of(field);
    let read_only = get(field, "flags").and_then(|f| get(f, "readOnly"));
    let disabled = truthy(read_only) || model.writing.get(&key) == Some(&true);
    let label = match get(field, "alternateName").filter(|v| !v.is_null()) {
        Some(name) => name.clone(),
        None => member(field, "name"),
    };
    let control = match str_of(field, "family") {
        Some("text") => FillControl::Text {
            value: member(field, "value"),
            multiline: member(field, "multiline"),
            password: member(field, "password"),
            max_length: member(field, "maxLength"),
            comb: member(field, "comb"),
        },
        Some("checkbox") => {
            let toggle = widget_of(field, annot_object_number);
            FillControl::Toggle {
                kind: ToggleKind::Checkbox,
                checked: member(field, "checked"),
                on_state: toggle_member(toggle, "onState", json!("Yes")),
            }
        }
        Some("radio") => {
            let toggle = widget_of(field, annot_object_number);
            FillControl::Toggle {
                kind: ToggleKind::Radio,
                checked: toggle_member(toggle, "checked", json!(false)),
                on_state: toggle_member(toggle, "onState", json!("")),
            }
        }
        Some("combobox") => {
            let value = member(field, "value");
            let selected = if value == json!("") { json!([]) } else { json!([value]) };
            FillControl::Choice {
                kind: ChoiceKind::Combo,
                edit: member(field, "edit"),
                multi: json!(false),
                options: member(field, "options"),
                selected,
            }
        }
        Some("listbox") => FillControl::Choice {
            kind: ChoiceKind::List,
            edit: json!(false),
            multi: member(field, "multiSelect"),
            options: member(field, "options"),
            selected: member(field, "selectedValues"),
        },
        Some("pushbutton") => FillControl::Button,
        _ => return None,
    };
    Some(FillItem {
        key,
        annot_object_number,
        bx: bx.unwrap_or(ZERO_BOX),
        disabled,
        label,
        control,
    })
}

/// `fillItems(model, pageObjectNumber)`: one page's widgets as fill
/// controls. Widgets without geometry yet, or on another page, are skipped.
pub fn fill_items(model: &Model, page_object_number: f64) -> Vec<FillItem> {
    let (Some(snapshot), Some(geom)) =
        (&model.snapshot, model.geom.get(&number_key(page_object_number)))
    else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for field in get(snapshot, "fields").and_then(Value::as_array).into_iter().flatten() {
        for widget in get(field, "widgets").and_then(Value::as_array).into_iter().flatten() {
            if num(widget, "pageObjectNumber") != Some(page_object_number) {
                continue;
            }
            let annot = get(widget, "annotObjectNumber").and_then(Value::as_f64);
            let Some(bx) = annot.and_then(|n| geom.get(&number_key(n))) else { continue };
            let annot = annot.unwrap_or(f64::NAN);
            if let Some(item) = project_widget(model, field, annot, Some(*bx)) {
                items.push(item);
            }
        }
    }
    items
}

/// `fillItemForWidget(model, annotObjectNumber)`: one widget, using the
/// model's geometry only when it happens to be loaded.
pub fn fill_item_for_widget(model: &Model, annot_object_number: f64) -> Option<FillItem> {
    let field = field_for_widget(model, annot_object_number)?;
    let widget = widget_of(&field, annot_object_number)?;
    let pon = get(widget, "pageObjectNumber").and_then(Value::as_f64);
    let bx = pon
        .and_then(|pon| model.geom.get(&number_key(pon)))
        .and_then(|boxes| boxes.get(&number_key(annot_object_number)))
        .copied();
    project_widget(model, &field, annot_object_number, bx)
}
