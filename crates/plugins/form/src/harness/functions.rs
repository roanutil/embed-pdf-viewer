//! The function table `vectors/plugins/form-core.json` names, each the
//! shipping function behind it (`tooling/plugin-conformance/src/suites/form-core.ts`).
//! `fillItemsAllowed`, `formTools`, `resolveFieldSelection` and
//! `buildSubmitEntries` are the suite's own wrappers: a fixed allowing
//! predicate, the palette with its tags, the `listed` predicate read for
//! every field, and the diagnostics collected beside the entries.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::{Error, is_undefined};
use plugin_conformance::FunctionTable;
use serde_json::{Value, json};

use crate::engine::{encode_field_ref_key, to_field_ref};
use crate::field_selection::{build_submit_entries, resolve_field_selection};
use crate::model::{
    FieldIndex, Folded, PageWidget, empty_field_index, field_by_key, field_for_widget,
    field_key_of, fold_form_event, index_fields, index_loaded_fields, initial_form_state,
    page_widgets_json, page_widgets_of, page_widgets_of_json, remove_widgets, upsert_fields,
    upsert_widgets, widget_at, widget_row_of,
};
use crate::read::fill_items::{fill_items, project_widget};
use crate::sync::predict::{fields_named_by, predict_form_op, undo_look_of};
use crate::tools::definitions::{FORM_TOOLS, PLACE_TAGS, form_tool_by_id};
use crate::value::{get, string_of, truthy};

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn value(args: &[Value], i: usize) -> Value {
    arg(args, i).cloned().unwrap_or(Value::Null)
}

fn index(args: &[Value], i: usize) -> Rc<FieldIndex> {
    Rc::new(FieldIndex::from_json(&value(args, i)))
}

fn snapshot(args: &[Value], i: usize) -> Rc<Value> {
    Rc::new(value(args, i))
}

fn list(args: &[Value], i: usize) -> Vec<Value> {
    arg(args, i).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn number(args: &[Value], i: usize) -> f64 {
    arg(args, i).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

fn or_null(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

fn field_key_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(field_key_of(&value(args, 0))))
}

fn empty_field_index_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(empty_field_index().to_json())
}

fn index_fields_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(index_fields(snapshot(args, 0), Default::default()).to_json())
}

fn index_loaded_fields_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(index_loaded_fields(&index(args, 0), snapshot(args, 1)).to_json())
}

fn upsert_fields_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(upsert_fields(&index(args, 0), &list(args, 1)).to_json())
}

fn upsert_widgets_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(upsert_widgets(&index(args, 0), &list(args, 1)).to_json())
}

fn remove_widgets_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(remove_widgets(&index(args, 0), &list(args, 1)).to_json())
}

fn fold_form_event_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(match fold_form_event(&index(args, 0), &value(args, 1)) {
        Folded::Index(next) => next.to_json(),
        // `reload()` is `{ [RELOAD]: 'all' }`, a symbol key JSON can't show.
        Folded::Reload => json!({}),
    })
}

fn field_by_key_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(or_null(field_by_key(&index(args, 0), &string_of(arg(args, 1)))))
}

fn field_for_widget_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(or_null(field_for_widget(&index(args, 0), number(args, 1))))
}

fn widget_row_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(or_null(widget_row_of(&index(args, 0), number(args, 1))))
}

fn project_widget_fn(args: &[Value]) -> Result<Value, Error> {
    let widget = PageWidget::read(&value(args, 2));
    let item = project_widget(&value(args, 0), number(args, 1), &widget, truthy(arg(args, 3)));
    Ok(item.unwrap_or(Value::Null))
}

fn widget_at_fn(args: &[Value]) -> Result<Value, Error> {
    let widgets = arg(args, 1).map(page_widgets_of_json);
    let point = value(args, 2);
    let x = get(&point, "x").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let y = get(&point, "y").and_then(Value::as_f64).unwrap_or(f64::NAN);
    Ok(widget_at(&index(args, 0), widgets.as_ref(), x, y).map_or(Value::Null, |h| h.to_json()))
}

fn page_widgets_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(page_widgets_json(&page_widgets_of(&index(args, 0), number(args, 1))))
}

fn initial_form_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(initial_form_state())
}

fn predict_form_op_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(predict_form_op(&index(args, 0), &value(args, 1)).to_json())
}

fn undo_look_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(Value::Array(undo_look_of(&index(args, 0), &list(args, 1))))
}

fn fields_named_by_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(Value::Array(fields_named_by(&index(args, 0), &list(args, 1))))
}

fn to_field_ref_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(to_field_ref(&string_of(arg(args, 0))))
}

fn encode_field_ref_key_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(encode_field_ref_key(&value(args, 0))?))
}

fn fill_items_allowed_fn(args: &[Value]) -> Result<Value, Error> {
    let widgets = arg(args, 2).map(page_widgets_of_json);
    Ok(Value::Array(fill_items(&index(args, 0), number(args, 1), widgets.as_ref(), &|_| true)))
}

fn form_tools_fn(_: &[Value]) -> Result<Value, Error> {
    let tools: Vec<Value> = FORM_TOOLS.iter().map(|t| t.to_json()).collect();
    Ok(json!({ "tools": tools, "placeTags": PLACE_TAGS }))
}

fn form_tool_by_id_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(form_tool_by_id(&string_of(arg(args, 0))).map_or(Value::Null, |t| t.to_json()))
}

fn resolve_field_selection_fn(args: &[Value]) -> Result<Value, Error> {
    let all = list(args, 0);
    let targets = arg(args, 1).and_then(Value::as_array).cloned();
    let selection = resolve_field_selection(&all, targets.as_deref(), truthy(arg(args, 2)));
    let listed: Vec<bool> = all.iter().map(|f| selection.listed(f)).collect();
    Ok(json!({ "selected": selection.selected, "listed": listed }))
}

fn build_submit_entries_fn(args: &[Value]) -> Result<Value, Error> {
    let fields = list(args, 0);
    let intent = epdf_kernel::js::from_value(&value(args, 1))?;
    let diagnostics = RefCell::new(Vec::new());
    let entries = build_submit_entries(&fields, &intent, &|d| diagnostics.borrow_mut().push(d));
    Ok(json!({
        "entries": epdf_kernel::js::to_value(&entries),
        "diagnostics": epdf_kernel::js::to_value(&diagnostics.into_inner()),
    }))
}

/// The functions `vectors/plugins/form-core.json` names.
pub fn functions() -> FunctionTable {
    vec![
        ("fieldKeyOf", field_key_of_fn),
        ("emptyFieldIndex", empty_field_index_fn),
        ("indexFields", index_fields_fn),
        ("indexLoadedFields", index_loaded_fields_fn),
        ("upsertFields", upsert_fields_fn),
        ("upsertWidgets", upsert_widgets_fn),
        ("removeWidgets", remove_widgets_fn),
        ("foldFormEvent", fold_form_event_fn),
        ("fieldByKey", field_by_key_fn),
        ("fieldForWidget", field_for_widget_fn),
        ("projectWidget", project_widget_fn),
        ("widgetAt", widget_at_fn),
        ("widgetRowOf", widget_row_of_fn),
        ("pageWidgetsOf", page_widgets_of_fn),
        ("initialFormState", initial_form_state_fn),
        ("predictFormOp", predict_form_op_fn),
        ("undoLookOf", undo_look_of_fn),
        ("fieldsNamedBy", fields_named_by_fn),
        ("toFieldRef", to_field_ref_fn),
        ("encodeFieldRefKey", encode_field_ref_key_fn),
        ("fillItemsAllowed", fill_items_allowed_fn),
        ("formTools", form_tools_fn),
        ("formToolById", form_tool_by_id_fn),
        ("resolveFieldSelection", resolve_field_selection_fn),
        ("buildSubmitEntries", build_submit_entries_fn),
    ]
}
