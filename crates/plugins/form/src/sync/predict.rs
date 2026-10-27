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
        .filter(|f| str_of(f, "family").is_some_and(|family| RESET_FAMILIES.contains(&family)))
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
