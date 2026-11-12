//! `sync/predict.ts`: what this session asked for and the engine hasn't
//! answered, on top of the form. [`predict_form_op`] is the fields mirror's
//! `predict` (the view replays it op by op), and [`undo_look_of`] what
//! undoing a change looks like. Both follow the engine's own rules
//! (`crate::engine`). Pure.
//!
//! The TypeScript memoizes each op over each index by identity, so a field
//! keeps its object while nothing about it changed. The kernel's mirror
//! memoizes its view on the confirmed value and the queue's version, and
//! nothing here compares by identity, so the per-op memo has no
//! counterpart.

use std::rc::Rc;

use serde_json::{Value, json};

use crate::engine::{
    FieldDraftContext, FieldWithWidgets, annotation_key, field_after_reset, field_after_value,
    field_key_of_ref, form_field_after, form_field_of_draft, reordered_list, widget_after_patch,
    widgets_after_display,
};
use crate::model::{
    FieldIndex, FieldKey, field_by_key, field_by_ref, field_for_widget, field_key_of, remove_field,
    remove_widgets, reorder_widget_rows, upsert_fields, upsert_widgets, widget_number,
    widget_row_of, with_calculation_order, with_snapshot_member,
};
use crate::value::{get, items, str_of, truthy};

/// The rows of a field's widgets, in its widget order.
fn rows_of_field(index: &FieldIndex, field: &Value) -> Vec<Value> {
    items(field, "widgets")
        .iter()
        .filter_map(|w| {
            let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            widget_row_of(index, n).cloned()
        })
        .collect()
}

/// `withRows(index, field)`.
fn with_rows(index: &FieldIndex, field: &Value) -> FieldWithWidgets {
    FieldWithWidgets { field: field.clone(), widgets: rows_of_field(index, field) }
}

/// `withFields(index, parts)`: fields and their rows put in at once.
fn with_fields(index: &Rc<FieldIndex>, parts: &[FieldWithWidgets]) -> Rc<FieldIndex> {
    if parts.is_empty() {
        return index.clone();
    }
    let fields: Vec<Value> = parts.iter().map(|p| p.field.clone()).collect();
    let rows: Vec<Value> = parts.iter().flat_map(|p| p.widgets.iter().cloned()).collect();
    upsert_widgets(&upsert_fields(index, &fields), &rows)
}

const RESET_FAMILIES: [&str; 5] = ["text", "checkbox", "radio", "combobox", "listbox"];

/// `resetTargets(index)`: the fields a whole-form reset puts back.
fn reset_targets(index: &FieldIndex) -> Vec<Value> {
    index
        .fields()
        .iter()
        .filter(|f| str_of(f, "kind").is_some_and(|family| RESET_FAMILIES.contains(&family)))
        .map(|f| (**f).clone())
        .collect()
}

/// `predictFormOp(index, op)`: one op of a change this session staged,
/// shown before the engine answers. An op the engine's rules refuse shows
/// nothing.
pub fn predict_form_op(index: &Rc<FieldIndex>, op: &Value) -> Rc<FieldIndex> {
    predict_once(index, op).unwrap_or_else(|_| index.clone())
}

fn predict_once(index: &Rc<FieldIndex>, op: &Value) -> Result<Rc<FieldIndex>, epdf_kernel::Error> {
    let none = Value::Null;
    Ok(match str_of(op, "type").unwrap_or("") {
        "forms.setValue" => match field_by_ref(index, get(op, "field").unwrap_or(&none)) {
            Some(field) => {
                let value = get(op, "value").cloned().unwrap_or(Value::Null);
                let after = field_after_value(&with_rows(index, field), &value)?;
                with_fields(index, &[after])
            }
            None => index.clone(),
        },
        "forms.reset" => {
            let targets: Vec<Value> = match get(op, "fields").filter(|f| truthy(Some(f))) {
                Some(refs) => refs
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| field_by_ref(index, r).cloned())
                    .collect(),
                None => reset_targets(index),
            };
            let parts = targets
                .iter()
                .map(|field| field_after_reset(&with_rows(index, field)))
                .collect::<Result<Vec<_>, _>>()?;
            with_fields(index, &parts)
        }
        "forms.setDisplay" => match field_by_ref(index, get(op, "field").unwrap_or(&none)) {
            Some(field) => {
                let rows = rows_of_field(index, field);
                let display = get(op, "display").cloned().unwrap_or(Value::Null);
                upsert_widgets(index, &widgets_after_display(&rows, &display)?)
            }
            None => index.clone(),
        },
        "forms.restore" => {
            let part = FieldWithWidgets {
                field: get(op, "field").cloned().unwrap_or(Value::Null),
                widgets: items(op, "widgets").to_vec(),
            };
            with_fields(index, &[part])
        }
        "forms.restoreWidget" => {
            let restored =
                upsert_widgets(index, &[get(op, "widget").cloned().unwrap_or(Value::Null)]);
            match get(op, "field").filter(|f| truthy(Some(f))) {
                Some(field) => upsert_fields(&restored, std::slice::from_ref(field)),
                None => restored,
            }
        }
        "forms.create" => created_field(index, op)?,
        "forms.copy" => copied_fields(index, op)?,
        "forms.addWidget" => added_widget(index, op)?,
        "forms.update" => match field_by_ref(index, get(op, "field").unwrap_or(&none)) {
            Some(field) => {
                let patch = get(op, "patch").cloned().unwrap_or(Value::Null);
                with_fields(index, &[form_field_after(&with_rows(index, field), &patch)?])
            }
            None => index.clone(),
        },
        "forms.delete" => {
            let Some(field) = field_by_ref(index, get(op, "field").unwrap_or(&none)).cloned()
            else {
                return Ok(index.clone());
            };
            let Some(snapshot) = &index.snapshot else { return Ok(index.clone()) };
            let own = get(&field, "ref").cloned().unwrap_or(Value::Null);
            let gone = remove_widgets(&remove_field(index, &own), items(&field, "widgets"));
            let order: Vec<Value> = snapshot
                .items("calculationOrder")
                .iter()
                .filter(|r| !same_field(r, &own))
                .cloned()
                .collect();
            with_calculation_order(&gone, Some(&Value::Array(order)))
        }
        "forms.updateWidget" => match row_of_ref(index, get(op, "widget").unwrap_or(&none)) {
            Some(row) => {
                let patch = get(op, "patch").cloned().unwrap_or(Value::Null);
                upsert_widgets(index, &[widget_after_patch(row, &patch)?])
            }
            None => index.clone(),
        },
        "forms.deleteWidget" => {
            let Some(row) = row_of_ref(index, get(op, "widget").unwrap_or(&none)).cloned() else {
                return Ok(index.clone());
            };
            let number = widget_number(&row);
            let field = field_for_widget(index, number).cloned();
            let gone = remove_widgets(
                index,
                &[
                    json!({ "objectNumber": number, "page": get(&row, "page").cloned().unwrap_or(Value::Null) }),
                ],
            );
            match field {
                None => gone,
                Some(field) => {
                    let widgets: Vec<Value> = items(&field, "widgets")
                        .iter()
                        .filter(|w| get(w, "objectNumber").and_then(Value::as_f64) != Some(number))
                        .cloned()
                        .collect();
                    upsert_fields(
                        &gone,
                        &[crate::value::spread(&field, [("widgets", Value::Array(widgets))])],
                    )
                }
            }
        }
        "forms.reorderWidgets" => {
            if index.snapshot.is_none() {
                return Ok(index.clone());
            }
            let page = get(op, "page").cloned().unwrap_or(Value::Null);
            let on_page: Vec<Value> = index
                .widgets()
                .iter()
                .filter(|row| {
                    crate::value::strict_eq(
                        get(row, "page").and_then(|p| get(p, "objectNumber")),
                        get(&page, "objectNumber"),
                    )
                })
                .map(|row| get(row, "ref").cloned().unwrap_or(Value::Null))
                .collect();
            let order = reordered_list(
                &on_page,
                items(op, "widgets"),
                get(op, "position").unwrap_or(&Value::Null),
                &annotation_key,
            )?;
            reorder_widget_rows(index, &page, &order)
        }
        "forms.reorderCalculations" => {
            let Some(snapshot) = &index.snapshot else { return Ok(index.clone()) };
            let order: Vec<Value> = snapshot
                .items("calculationOrder")
                .iter()
                .filter(|r| !r.is_null())
                .cloned()
                .collect();
            let next = reordered_list(
                &order,
                items(op, "fields"),
                get(op, "position").unwrap_or(&Value::Null),
                &field_key_of_ref,
            )?;
            with_calculation_order(index, Some(&Value::Array(next)))
        }
        _ => index.clone(),
    })
}

/// `rowOfRef(index, ref)`.
fn row_of_ref<'a>(index: &'a FieldIndex, reference: &Value) -> Option<&'a Value> {
    if str_of(reference, "kind") != Some("objectNumber") {
        return None;
    }
    let n = get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
    widget_row_of(index, n)
}

/// `sameField(left, right)`.
fn same_field(left: &Value, right: &Value) -> bool {
    !left.is_null() && field_key_of_ref(left) == field_key_of_ref(right)
}

/// `createdField(index, op)`: a create once it names its numbers.
fn created_field(index: &Rc<FieldIndex>, op: &Value) -> Result<Rc<FieldIndex>, epdf_kernel::Error> {
    let draft = get(op, "draft").cloned().unwrap_or(Value::Null);
    let placements = items(&draft, "widgets");
    let numbers = items(op, "widgetObjectNumbers");
    let Some(object_number) = get(op, "objectNumber").and_then(Value::as_f64) else {
        return Ok(index.clone());
    };
    if numbers.len() != placements.len() {
        return Ok(index.clone());
    }
    let Some(snapshot) = index.snapshot.clone() else { return Ok(index.clone()) };
    if field_by_ref(index, &json!({ "kind": "objectNumber", "objectNumber": object_number }))
        .is_some()
    {
        return Ok(index.clone());
    }
    let widgets: Vec<Value> = placements
        .iter()
        .zip(numbers)
        .map(|(placement, n)| {
            json!({
                "kind": "objectNumber",
                "page": get(placement, "page").cloned().unwrap_or(Value::Null),
                "objectNumber": n,
            })
        })
        .collect();
    let created = form_field_of_draft(&draft, &FieldDraftContext { object_number, widgets })?;
    let formed = if snapshot.get("formKind").and_then(Value::as_str) == Some("none") {
        with_snapshot_member(index, "formKind", json!("acroform"))
    } else {
        index.clone()
    };
    let created_ref = get(&created.field, "ref").cloned().unwrap_or(Value::Null);
    let next = with_fields(&formed, &[created]);
    let calculates = get(&draft, "actions").and_then(|a| get(a, "calculate"));
    Ok(if truthy(calculates) {
        let mut order = snapshot.items("calculationOrder").to_vec();
        order.push(created_ref);
        with_calculation_order(&next, Some(&Value::Array(order)))
    } else {
        next
    })
}

/// One field a `forms.copy` makes, with the numbers the op names for it.
struct FieldCopy {
    plan: crate::engine::copy::PlannedFieldCopy,
    object_number: Value,
    widget_object_numbers: Vec<Value>,
}

/// `copiesOf(index, op)`: the fields a `forms.copy` makes, by the engine's
/// own rule (`planFormCopies`), each with the object numbers the op names:
/// its own, then its widgets'. None for a copy that doesn't name them all,
/// which shows when the engine answers, and none for one the engine would
/// refuse. What a script right leaves out is the engine's to say.
fn copies_of(index: &FieldIndex, op: &Value) -> Vec<FieldCopy> {
    let Some(numbers) = get(op, "objectNumbers").and_then(Value::as_array) else {
        return Vec::new();
    };
    if index.snapshot.is_none() {
        return Vec::new();
    }
    let widgets = get(op, "widgets").and_then(Value::as_array).map(Vec::as_slice);
    let placement = crate::engine::copy::CopyPlacement::of(op);
    let values = get(op, "values").and_then(Value::as_bool).unwrap_or(true);
    let Ok(plan) = crate::write::duplicate::plan_over(
        index,
        items(op, "fields"),
        widgets,
        &placement,
        values,
        true,
    ) else {
        return Vec::new();
    };
    let needed: usize = plan.creates.iter().map(|c| 1 + c.widgets.len()).sum();
    if numbers.len() != needed {
        return Vec::new();
    }
    let mut at = 0;
    plan.creates
        .into_iter()
        .map(|create| {
            let taken = &numbers[at..at + 1 + create.widgets.len()];
            at += 1 + create.widgets.len();
            FieldCopy {
                object_number: taken[0].clone(),
                widget_object_numbers: taken[1..].to_vec(),
                plan: create,
            }
        })
        .collect()
}

/// `copiedFields(index, op)`: each copy as a create of the same field
/// shows, then its widgets' flags, which a create writes none of, and its value.
fn copied_fields(index: &Rc<FieldIndex>, op: &Value) -> Result<Rc<FieldIndex>, epdf_kernel::Error> {
    let mut next = index.clone();
    for copy in copies_of(index, op) {
        let reference = json!({ "kind": "objectNumber", "objectNumber": copy.object_number });
        if field_by_ref(&next, &reference).is_some() {
            continue;
        }
        next = created_field(
            &next,
            &json!({
                "type": "forms.create",
                "draft": copy.plan.draft,
                "objectNumber": copy.object_number,
                "widgetObjectNumbers": copy.widget_object_numbers,
            }),
        )?;
        let Some(made) = field_by_ref(&next, &reference).cloned() else { continue };
        let rows = rows_of_field(&next, &made)
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let flags = copy.plan.flags.get(at).cloned().unwrap_or_default();
                widget_after_patch(row, &Value::Object(flags))
            })
            .collect::<Result<Vec<_>, _>>()?;
        next = upsert_widgets(&next, &rows);
        if let Some(value) = &copy.plan.value {
            let made = field_by_ref(&next, &reference).cloned().unwrap_or(made);
            next = with_fields(&next, &[field_after_value(&with_rows(&next, &made), value)?]);
        }
    }
    Ok(next)
}

/// `addedWidget(index, op)`: `forms.addWidget`, once it names its widget's
/// number: the widget as a create of a field of that kind places it, last
/// among the field's. One that splits a merged field shows when the engine
/// answers.
fn added_widget(index: &Rc<FieldIndex>, op: &Value) -> Result<Rc<FieldIndex>, epdf_kernel::Error> {
    let Some(object_number) = get(op, "objectNumber").filter(|n| !n.is_null()) else {
        return Ok(index.clone());
    };
    if get(op, "splitObjectNumber").is_some_and(|n| !n.is_null()) {
        return Ok(index.clone());
    }
    let none = Value::Null;
    let Some(field) = field_by_ref(index, get(op, "field").unwrap_or(&none)).cloned() else {
        return Ok(index.clone());
    };
    let own = get(&field, "ref").cloned().unwrap_or(Value::Null);
    if str_of(&field, "kind") == Some("unknown") || str_of(&own, "kind") != Some("objectNumber") {
        return Ok(index.clone());
    }
    if widget_row_of(index, object_number.as_f64().unwrap_or(f64::NAN)).is_some() {
        return Ok(index.clone());
    }
    let placement = get(op, "placement").cloned().unwrap_or(Value::Null);
    let mut draft = serde_json::Map::new();
    draft.insert("kind".into(), get(&field, "kind").cloned().unwrap_or(Value::Null));
    draft.insert("name".into(), get(&field, "name").cloned().unwrap_or(Value::Null));
    if let Some(options) = get(&field, "options") {
        let options: Vec<Value> = options
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| json!({ "label": get(o, "label").cloned().unwrap_or(Value::Null), "value": get(o, "value").cloned().unwrap_or(Value::Null) }))
            .collect();
        draft.insert("options".into(), Value::Array(options));
    }
    draft.insert("widgets".into(), json!([placement]));
    let made = form_field_of_draft(
        &Value::Object(draft),
        &FieldDraftContext {
            object_number: get(&own, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN),
            widgets: vec![json!({
                "kind": "objectNumber",
                "page": get(&placement, "page").cloned().unwrap_or(Value::Null),
                "objectNumber": object_number,
            })],
        },
    )?;
    let (Some(widget), Some(row)) =
        (items(&made.field, "widgets").first().cloned(), made.widgets.first().cloned())
    else {
        return Ok(index.clone());
    };
    let mut widgets = items(&field, "widgets").to_vec();
    widgets.push(widget);
    let grown = crate::value::spread(&field, [("widgets", Value::Array(widgets))]);
    Ok(upsert_widgets(&upsert_fields(index, &[grown]), &[row]))
}

/// `fieldsNamedBy(index, ops)`: the fields a change's ops write a value or
/// a look to, each once, in op order.
pub fn fields_named_by(index: &FieldIndex, ops: &[Value]) -> Vec<Value> {
    let mut named: indexmap::IndexMap<FieldKey, Value> = indexmap::IndexMap::new();
    let mut add = |field: Option<&Value>| {
        if let Some(field) = field {
            named.entry(field_key_of(field)).or_insert_with(|| field.clone());
        }
    };
    for op in ops {
        match str_of(op, "type").unwrap_or("") {
            "forms.setValue" | "forms.setDisplay" | "forms.setAppearanceText" => {
                add(field_by_ref(index, get(op, "field").unwrap_or(&Value::Null)));
            }
            "forms.reset" => match get(op, "fields").filter(|f| truthy(Some(f))) {
                Some(refs) => {
                    for r in refs.as_array().into_iter().flatten() {
                        add(field_by_ref(index, r));
                    }
                }
                None => {
                    for field in reset_targets(index) {
                        add(Some(&field));
                    }
                }
            },
            _ => {}
        }
    }
    named.into_values().collect()
}

/// `withFieldsFrom(index, source, fields)`: these fields, and their rows,
/// as `source` holds them.
pub fn with_fields_from(
    index: &Rc<FieldIndex>,
    source: &FieldIndex,
    fields: &[Value],
) -> Rc<FieldIndex> {
    let parts: Vec<FieldWithWidgets> = fields
        .iter()
        .filter_map(|field| {
            field_by_key(source, &field_key_of(field)).map(|held| with_rows(source, held))
        })
        .collect();
    with_fields(index, &parts)
}

/// `restoreOf(index, field)`.
fn restore_of(index: &FieldIndex, field: &Value) -> Value {
    json!({ "type": "forms.restore", "field": field, "widgets": rows_of_field(index, field) })
}

/// `restoreWidgetOf(index, row)`.
fn restore_widget_of(index: &FieldIndex, row: &Value) -> Value {
    json!({
        "type": "forms.restoreWidget",
        "widget": row,
        "field": field_for_widget(index, widget_number(row)).cloned().unwrap_or(Value::Null),
    })
}

/// `undoOfOp(index, op)`.
fn undo_of_op(index: &FieldIndex, op: &Value) -> Vec<Value> {
    let none = Value::Null;
    match str_of(op, "type").unwrap_or("") {
        "forms.create" => match get(op, "objectNumber") {
            None => Vec::new(),
            Some(n) => vec![json!({
                "type": "forms.delete",
                "field": { "kind": "objectNumber", "objectNumber": n },
            })],
        },
        // The copies go, the last made first.
        "forms.copy" => copies_of(index, op)
            .into_iter()
            .rev()
            .map(|copy| {
                json!({ "type": "forms.delete", "field": { "kind": "objectNumber", "objectNumber": copy.object_number } })
            })
            .collect(),
        "forms.addWidget" => match get(op, "objectNumber").filter(|n| !n.is_null()) {
            None => Vec::new(),
            Some(n) => vec![json!({
                "type": "forms.deleteWidget",
                "widget": {
                    "kind": "objectNumber",
                    "page": get(op, "placement").and_then(|p| get(p, "page")).cloned().unwrap_or(Value::Null),
                    "objectNumber": n,
                },
            })],
        },
        "forms.update" | "forms.delete" => field_by_ref(index, get(op, "field").unwrap_or(&none))
            .map(|field| vec![restore_of(index, field)])
            .unwrap_or_default(),
        "forms.updateWidget" | "forms.deleteWidget" => {
            row_of_ref(index, get(op, "widget").unwrap_or(&none))
                .map(|row| vec![restore_widget_of(index, row)])
                .unwrap_or_default()
        }
        "forms.removeWidget" => {
            let field = field_by_ref(index, get(op, "field").unwrap_or(&none));
            let row = row_of_ref(index, get(op, "widget").unwrap_or(&none));
            let mut out = Vec::new();
            if let Some(row) = row {
                out.push(restore_widget_of(index, row));
            }
            if let Some(field) = field {
                out.push(restore_of(index, field));
            }
            out
        }
        "forms.reorderWidgets" => {
            let page = get(op, "page").cloned().unwrap_or(Value::Null);
            let widgets: Vec<Value> = index
                .widgets()
                .iter()
                .filter(|row| {
                    crate::value::strict_eq(
                        get(row, "page").and_then(|p| get(p, "objectNumber")),
                        get(&page, "objectNumber"),
                    )
                })
                .map(|row| get(row, "ref").cloned().unwrap_or(Value::Null))
                .collect();
            vec![
                json!({ "type": "forms.reorderWidgets", "page": page, "widgets": widgets, "position": "start" }),
            ]
        }
        "forms.reorderCalculations" => {
            let fields: Vec<Value> = index
                .snapshot
                .as_deref()
                .map(|s| {
                    s.items("calculationOrder").iter().filter(|r| !r.is_null()).cloned().collect()
                })
                .unwrap_or_default();
            vec![
                json!({ "type": "forms.reorderCalculations", "fields": fields, "position": "start" }),
            ]
        }
        _ => fields_named_by(index, std::slice::from_ref(op))
            .iter()
            .map(|field| restore_of(index, field))
            .collect(),
    }
}

/// `undoLookOf(before, ops)`: the ops that bring the form back, the last
/// op's first.
pub fn undo_look_of(before: &Rc<FieldIndex>, ops: &[Value]) -> Vec<Value> {
    let mut undo: Vec<Vec<Value>> = Vec::new();
    let mut current = before.clone();
    for op in ops {
        undo.push(undo_of_op(&current, op));
        current = predict_form_op(&current, op);
    }
    undo.into_iter().rev().flatten().collect()
}
