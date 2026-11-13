//! `write/duplicate.ts` and `write/new-widget.ts`: duplicate, for a form's
//! widgets. What a copy becomes follows from what its widget is. A radio
//! button is one answer of its group, so its copy is another button of that
//! group (`forms.addWidget`), with the next free value. Any other widget is a
//! whole question, so its copy is a new field (`forms.copy`), and widgets of
//! one field copied together become one new field. Pure: the ops, given the
//! object numbers they take.

use std::collections::{HashMap, HashSet};

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Map, Value, json};

use crate::engine::annotation_key;
use crate::engine::copy::{
    CopyPlacement, FormCopyPlan, FormParts, plan_form_copies, widget_flags_of, widget_placement_of,
};
use crate::model::{FieldIndex, field_for_widget, field_key_of, widget_row_of};
use crate::value::{get, items, str_of};

/// `nextChoiceOf(exportValues)`: the next free `Choice<n>`, counting on
/// from a radio group's buttons.
pub fn next_choice_of(export_values: &[String]) -> String {
    let mut n = export_values.len() + 1;
    while export_values.iter().any(|v| *v == format!("Choice{n}")) {
        n += 1;
    }
    format!("Choice{n}")
}

/// `isMerged(field)`: the field's dictionary is its only widget's too,
/// which adding a widget splits.
pub fn is_merged(field: &Value) -> bool {
    let reference = get(field, "ref").unwrap_or(&Value::Null);
    str_of(reference, "kind") == Some("objectNumber")
        && items(field, "widgets").iter().any(|w| {
            crate::value::strict_eq(get(w, "objectNumber"), get(reference, "objectNumber"))
        })
}

/// The flags a widget has when a create or `addWidget` made it.
const NEW_WIDGET_FLAGS: [(&str, bool); 10] = [
    ("invisible", false),
    ("hidden", false),
    ("print", true),
    ("noZoom", false),
    ("noRotate", false),
    ("noView", false),
    ("readOnly", false),
    ("locked", false),
    ("toggleNoView", false),
    ("lockedContents", false),
];

/// `flagsToSet(row)`: the flags of `row` a new widget doesn't have, as a patch.
fn flags_to_set(row: &Value) -> Map<String, Value> {
    widget_flags_of(row)
        .into_iter()
        .filter(|(name, value)| {
            let fresh = NEW_WIDGET_FLAGS.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
            value.as_bool() != fresh
        })
        .collect()
}

fn number_of(reference: &Value) -> f64 {
    if str_of(reference, "kind") == Some("objectNumber") {
        get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
    } else {
        -1.0
    }
}

/// The plan a form's snapshot gives `planFormCopies`.
pub fn plan_over(
    index: &FieldIndex,
    fields: &[Value],
    widgets: Option<&[Value]>,
    placement: &CopyPlacement,
    values: bool,
    may_script: bool,
) -> Result<FormCopyPlan, Error> {
    let Some(snapshot) = &index.snapshot else { return Ok(FormCopyPlan::default()) };
    let form_fields: Vec<Value> = snapshot.fields().iter().map(|f| (**f).clone()).collect();
    let form_widgets: Vec<Value> = snapshot.widgets().iter().map(|w| (**w).clone()).collect();
    plan_form_copies(
        FormParts {
            fields: &form_fields,
            widgets: &form_widgets,
            calculation_order: snapshot.items("calculationOrder"),
        },
        fields,
        widgets,
        placement,
        values,
        may_script,
    )
}

struct Button {
    row: Value,
    field: Value,
    placement: Map<String, Value>,
    split: bool,
}

/// `WidgetDuplicate`: the ops that copy some widgets, before they have
/// their object numbers.
pub struct WidgetDuplicate {
    /// How many object numbers the ops take.
    pub needs: usize,
    /// What is left out before the engine is asked.
    pub dropped: Vec<Value>,
    rows: Vec<Value>,
    fields: Vec<Value>,
    only: Vec<Value>,
    buttons: Vec<Button>,
    plan: Option<FormCopyPlan>,
    for_fields: usize,
    raw_page: Option<Value>,
    raw_offset: Option<Value>,
    values: bool,
}

impl WidgetDuplicate {
    /// `build(numbers)`: the ops under these numbers, and the copy of each
    /// widget that is copied, in the order the widgets were given.
    pub fn build(&self, numbers: &[u32]) -> (Vec<Value>, Vec<Value>) {
        let mut ops = Vec::new();
        let mut copy_of: HashMap<String, Value> = HashMap::new();
        let mut at = 0;
        if let Some(plan) = &self.plan {
            let mut op = Map::new();
            op.insert("type".into(), json!("forms.copy"));
            op.insert(
                "fields".into(),
                Value::Array(
                    self.fields
                        .iter()
                        .map(|f| get(f, "ref").cloned().unwrap_or(Value::Null))
                        .collect(),
                ),
            );
            op.insert("widgets".into(), Value::Array(self.only.clone()));
            if let Some(page) = &self.raw_page {
                op.insert("page".into(), page.clone());
            }
            if let Some(offset) = &self.raw_offset {
                op.insert("offset".into(), offset.clone());
            }
            op.insert("values".into(), json!(self.values));
            op.insert("objectNumbers".into(), json!(numbers[..self.for_fields.min(numbers.len())]));
            ops.push(Value::Object(op));
            for create in &plan.creates {
                // Its own number, then its widgets', in the order they are copied.
                at += 1;
                let placed = items(&create.draft, "widgets");
                for (widget, source) in create.widgets.iter().enumerate() {
                    copy_of.insert(
                        annotation_key(source),
                        json!({
                            "kind": "objectNumber",
                            "page": placed.get(widget).and_then(|p| get(p, "page")).cloned().unwrap_or(Value::Null),
                            "objectNumber": numbers.get(at + widget),
                        }),
                    );
                }
                at += create.widgets.len();
            }
        }
        for button in &self.buttons {
            let object_number = numbers.get(at).copied();
            let copy = json!({
                "kind": "objectNumber",
                "page": button.placement.get("page").cloned().unwrap_or(Value::Null),
                "objectNumber": object_number,
            });
            let mut op = Map::new();
            op.insert("type".into(), json!("forms.addWidget"));
            op.insert("field".into(), get(&button.field, "ref").cloned().unwrap_or(Value::Null));
            op.insert("placement".into(), Value::Object(button.placement.clone()));
            op.insert("objectNumber".into(), json!(object_number));
            if button.split {
                op.insert("splitObjectNumber".into(), json!(numbers.get(at + 1)));
            }
            ops.push(Value::Object(op));
            at += if button.split { 2 } else { 1 };
            // A create writes no flags: a hidden or locked button's copy gets them after.
            let flags = flags_to_set(&button.row);
            if !flags.is_empty() {
                ops.push(json!({ "type": "forms.updateWidget", "widget": copy, "patch": flags }));
            }
            copy_of.insert(annotation_key(get(&button.row, "ref").unwrap_or(&Value::Null)), copy);
        }
        let copies = self
            .rows
            .iter()
            .filter_map(|row| {
                copy_of.get(&annotation_key(get(row, "ref").unwrap_or(&Value::Null))).cloned()
            })
            .collect();
        (ops, copies)
    }
}

/// `widgetDuplicateOf(index, widgets, placement, { values, mayScript })`:
/// the ops that copy `widgets`, which the form shows, by the rule above.
/// Throws `not-found` for a widget the form doesn't show.
pub fn widget_duplicate_of(
    index: &FieldIndex,
    widgets: &[Value],
    page: Option<Value>,
    offset: Option<Value>,
    values: bool,
    may_script: bool,
) -> Result<WidgetDuplicate, Error> {
    let placement = CopyPlacement::of(&json!({ "page": page, "offset": offset }));
    let (dx, dy) = placement.offset.unwrap_or((0.0, 0.0));
    let mut dropped = Vec::new();
    let mut rows = Vec::new();
    for reference in widgets {
        let row = widget_row_of(index, number_of(reference)).filter(|_| index.snapshot.is_some());
        let Some(row) = row else {
            return Err(plugin_error(
                code::NOT_FOUND,
                "form",
                format!("no widget {}", annotation_key(reference)),
                None,
                None,
            ));
        };
        rows.push(row.clone());
    }
    let mut fields: Vec<Value> = Vec::new();
    let mut only: Vec<Value> = Vec::new();
    let mut buttons: Vec<Button> = Vec::new();
    let mut values_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut split: HashSet<String> = HashSet::new();
    for row in &rows {
        let row_ref = get(row, "ref").cloned().unwrap_or(Value::Null);
        let Some(field) = field_for_widget(index, number_of(&row_ref)).cloned() else {
            dropped.push(json!({ "ref": null, "widget": row_ref, "reason": "no-field" }));
            continue;
        };
        let key = field_key_of(&field);
        if str_of(&field, "kind") != Some("radio") {
            if !fields.iter().any(|known| field_key_of(known) == key) {
                fields.push(field.clone());
            }
            only.push(row_ref);
            continue;
        }
        let taken = values_by.get(&key).cloned().unwrap_or_else(|| {
            items(&field, "widgets")
                .iter()
                .map(|w| crate::value::string_of(get(w, "exportValue")))
                .collect()
        });
        let export_value = next_choice_of(&taken);
        let mut next = taken;
        next.push(export_value.clone());
        values_by.insert(key.clone(), next);
        let look = widget_placement_of(&field, row, may_script, &mut dropped);
        let mut placed = look.clone();
        if let Some(page) = &placement.page {
            placed.insert("page".into(), page.clone());
        }
        if let Some(rect) = look.get("rect").filter(|r| crate::value::truthy(Some(r))) {
            let mut moved = rect.as_object().cloned().unwrap_or_default();
            let at = |k: &str| get(rect, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            moved.insert("x".into(), epdf_kernel::js::number(at("x") + dx));
            moved.insert("y".into(), epdf_kernel::js::number(at("y") + dy));
            placed.insert("rect".into(), Value::Object(moved));
        }
        // `{ ...look, exportValue }`: a toggle's own value keeps its place.
        placed.insert("exportValue".into(), json!(export_value));
        // Only the first button added splits a merged group.
        let splits = is_merged(&field) && !split.contains(&key);
        split.insert(key);
        buttons.push(Button { row: row.clone(), field, placement: placed, split: splits });
    }
    let plan = if !fields.is_empty() && index.snapshot.is_some() {
        let refs: Vec<Value> =
            fields.iter().map(|f| get(f, "ref").cloned().unwrap_or(Value::Null)).collect();
        // The engine decides what the session may write, and says what it left out.
        Some(plan_over(index, &refs, Some(&only), &placement, values, true)?)
    } else {
        None
    };
    let for_fields =
        plan.as_ref().map_or(0, |p| p.creates.iter().map(|c| 1 + c.widgets.len()).sum());
    let needs = for_fields + buttons.iter().map(|b| if b.split { 2 } else { 1 }).sum::<usize>();
    Ok(WidgetDuplicate {
        needs,
        dropped,
        rows,
        fields,
        only,
        buttons,
        plan,
        for_fields,
        raw_page: page,
        raw_offset: offset,
        values,
    })
}
