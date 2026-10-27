//! What a form import writes and leaves out, decided before its first write
//! (`core/src/transfer/planFormImport.ts`): a design import's fields checked
//! against the create schema, a values import's values matched to the
//! form's fields. And the facts each commits (`formImportFacts`,
//! `formValuesImportFacts` in `formImport.ts`).

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::bundle::{
    BundleImportPages, BundleImportTarget, BundlePage, PageMap, bundle_page_mapping,
};
use super::form_export::{annotation_key_of, field_ref_value_key};
use super::page_refs::{map_page_refs, page_refs_in};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::reader::ActionTree;
use crate::features::actions::write::{
    WIDGET_ACTION_EVENTS, action_write_of, field_script_of, needs_script_right,
};
use crate::features::forms::value::FormFieldValue;
use crate::identity::PageRef;
use crate::page_space::destinations::PageDestination;

/// The flags a widget row has, which a copy keeps (`WIDGET_FLAG_NAMES`), in
/// `/F` bit order.
pub const WIDGET_FLAG_NAMES: [&str; 10] = [
    "invisible",
    "hidden",
    "print",
    "noZoom",
    "noRotate",
    "noView",
    "readOnly",
    "locked",
    "toggleNoView",
    "lockedContents",
];

/// One field a design import creates (`PlannedFormField`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFormField {
    /// Its row in the bundle.
    pub field: usize,
    /// What the create writes: the field's settings, scripts and widgets,
    /// their pages mapped. Page space.
    pub draft: Map<String, Value>,
    /// Each widget's row in the bundle, in `draft.widgets` order.
    pub widgets: Vec<usize>,
    /// Each widget's flags, in `draft.widgets` order, in `/F` bit order.
    pub flags: Vec<[Option<bool>; 10]>,
    /// The value it gets; `None` when the import writes none.
    pub value: Option<FormFieldValue>,
}

/// `FormImportPlan`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormImportPlan {
    /// In bundle order.
    pub creates: Vec<PlannedFormField>,
    /// The calculation order among the creates, by their place in
    /// `creates`, as the bundle has it.
    pub calculation_order: Vec<usize>,
    /// What was left out, in bundle order (`FormImportDrop`).
    pub dropped: Vec<Value>,
}

/// What [`plan_form_import`] reads.
pub struct FormPlanInput<'a> {
    pub pages: &'a [BundlePage],
    /// The bundle's field rows (`{ data }`).
    pub fields: &'a [Value],
    /// The bundle's widget rows (`{ data, resources }`).
    pub widgets: &'a [Value],
    pub calculation_order: &'a [Value],
    pub page_mapping: &'a BundleImportPages,
    pub target: &'a [BundleImportTarget],
    /// Whether a new field of this full name can't join the form, as it is
    /// before the import.
    pub name_taken: &'a dyn Fn(&str) -> bool,
    /// Whether scripts, submits and links are kept (`doc.forms.script`).
    pub may_script: bool,
    /// Whether each field gets the value the bundle holds.
    pub values: bool,
}

/// What a design import of a bundle into the `target` pages creates, and
/// what it leaves out (`planFormImport`). Nothing here writes, so an import
/// refuses before its first write: `InvalidArg` when a field's data isn't
/// valid for its family, or when a page a widget is on or points at maps to
/// no target page; `NotFound` when a page list maps to a page the target
/// doesn't have. Left out, in bundle order: a field of an unknown family, a
/// field whose name is taken, a widget whose field isn't in the bundle, and
/// each action that is a script, a submit or a link without `may_script`,
/// or that a write can't make. Each field keeps every widget it has.
pub fn plan_form_import(input: FormPlanInput<'_>) -> Result<FormImportPlan, EngineError> {
    let widget_data: Vec<&Value> = input.widgets.iter().map(|row| &row["data"]).collect();
    let named: Vec<PageRef> = widget_data.iter().flat_map(|data| page_refs_in(data)).collect();
    let map = bundle_page_mapping(input.pages, input.page_mapping, input.target, &named)?;

    let row_of = |widget: &Value| -> Option<usize> {
        let key = annotation_key_of(widget.get("ref")?)?;
        widget_data.iter().position(|data| annotation_key_of(&data["ref"]).as_ref() == Some(&key))
    };
    let mut owned: HashSet<usize> = HashSet::new();
    let mut dropped = Vec::new();
    let mut claimed: HashSet<String> = HashSet::new();
    let mut creates = Vec::new();

    for (index, row) in input.fields.iter().enumerate() {
        let field = &row["data"];
        let rows: Vec<usize> =
            field["widgets"].as_array().into_iter().flatten().filter_map(row_of).collect();
        owned.extend(rows.iter().copied());
        if field["family"] == "unknown" {
            dropped.push(json!({ "ref": field["ref"], "reason": "unsupported-family" }));
            continue;
        }
        let name = field["name"].as_str().unwrap_or_default();
        if claimed.contains(name) || (input.name_taken)(name) {
            dropped.push(json!({ "ref": field["ref"], "reason": "name-conflict" }));
            continue;
        }
        claimed.insert(name.to_owned());
        let actions = field_actions_of(field, input.may_script, &mut dropped);
        let placements: Vec<Value> = rows
            .iter()
            .map(|at| placement_of(field, widget_data[*at], &map, input.may_script, &mut dropped))
            .collect();
        let draft = draft_of(field, actions, placements, index)?;
        let value = (input.values && holds_value(field)).then(|| field_value_of(field)).flatten();
        creates.push(PlannedFormField {
            field: index,
            draft,
            flags: rows.iter().map(|at| flags_of(widget_data[*at])).collect(),
            widgets: rows,
            value,
        });
    }

    for (index, data) in widget_data.iter().enumerate() {
        if !owned.contains(&index) {
            dropped
                .push(json!({ "ref": data["field"], "widget": data["ref"], "reason": "no-field" }));
        }
    }

    // The order the bundle gives the fields that kept their calculate script.
    let calculation_order = input
        .calculation_order
        .iter()
        .filter_map(|reference| {
            let key = field_ref_value_key(reference);
            creates.iter().position(|create| {
                field_ref_value_key(&input.fields[create.field]["data"]["ref"]) == key
                    && create.draft.get("actions").is_some_and(|a| a.get("calculate").is_some())
            })
        })
        .collect();
    Ok(FormImportPlan { creates, calculation_order, dropped })
}

/// One value a values import writes (`PlannedValueWrite`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedValueWrite {
    /// Its field's row in the bundle.
    pub field: usize,
    /// The document's field of that name, by its place in `target`.
    pub target: usize,
    pub value: FormFieldValue,
}

/// `FormValuesImportPlan`'s writes and drops.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormValuesPlan {
    pub writes: Vec<PlannedValueWrite>,
    /// `FormValuesImportDrop`s, in bundle order.
    pub dropped: Vec<Value>,
}

/// Why a values import can't fill the target field at its place with a
/// value: `value-not-allowed`, `locked` or `fill-not-allowed`; `None` when
/// it can.
pub type ValueRefusal<'a> =
    dyn FnMut(usize, &FormFieldValue) -> Result<Option<&'static str>, EngineError> + 'a;

/// What a values import of a bundle writes into a form whose fields are
/// `target` (field reads as JSON): each field the bundle holds a value for
/// gets it, matched by full name (`planFormValuesImport`). Left out, in
/// bundle order: a field the document doesn't have (`no-field`), one of
/// another family (`wrong-family`), and one `refusal` names a reason for.
pub fn plan_form_values_import(
    fields: &[Value],
    target: &[Value],
    refusal: &mut ValueRefusal<'_>,
) -> Result<FormValuesPlan, EngineError> {
    let mut plan = FormValuesPlan::default();
    for (index, row) in fields.iter().enumerate() {
        let field = &row["data"];
        let Some(value) = holds_value(field).then(|| field_value_of(field)).flatten() else {
            continue;
        };
        let Some(at) = target.iter().position(|candidate| candidate["name"] == field["name"])
        else {
            plan.dropped.push(json!({ "ref": field["ref"], "reason": "no-field" }));
            continue;
        };
        if target[at]["family"] != field["family"] {
            plan.dropped.push(json!({ "ref": field["ref"], "reason": "wrong-family" }));
            continue;
        }
        match refusal(at, &value)? {
            Some(reason) => plan.dropped.push(json!({ "ref": field["ref"], "reason": reason })),
            None => plan.writes.push(PlannedValueWrite { field: index, target: at, value }),
        }
    }
    Ok(plan)
}

/// Whether a field read holds a value a write can set (`holdsValue`).
fn holds_value(field: &Value) -> bool {
    matches!(field.pointer("/valueEntry/kind").and_then(Value::as_str), Some("scalar" | "array"))
}

/// The value a field read holds, as a value write takes it (`fieldValueOf`,
/// over the read's JSON).
pub fn field_value_of(field: &Value) -> Option<FormFieldValue> {
    match field["family"].as_str()? {
        "checkbox" => field["checked"].as_bool().map(FormFieldValue::Checked),
        "listbox" => serde_json::from_value(field["selectedValues"].clone())
            .ok()
            .map(FormFieldValue::SelectedValues),
        "radio" => {
            let value = field["value"].as_str()?;
            Some(FormFieldValue::Value((value != "Off").then(|| value.to_owned())))
        }
        "text" | "combobox" => {
            field["value"].as_str().map(|value| FormFieldValue::Value(Some(value.to_owned())))
        }
        _ => None,
    }
}

/// A field's scripts as a create writes them; each left out is reported
/// (`fieldActionsOf`).
fn field_actions_of(
    field: &Value,
    may_script: bool,
    dropped: &mut Vec<Value>,
) -> Map<String, Value> {
    let mut actions = Map::new();
    for (event, tree) in field["actions"].as_object().into_iter().flatten() {
        if tree.is_null() {
            continue;
        }
        let member = format!("actions.{event}");
        let script = serde_json::from_value::<ActionTree<PageDestination>>(tree.clone())
            .ok()
            .and_then(|tree| field_script_of(&tree));
        match script {
            None => dropped.push(json!({ "ref": field["ref"], "field": member,
                "reason": "unsupported-action" })),
            Some(_) if !may_script => dropped.push(json!({ "ref": field["ref"], "field": member,
                "reason": "script-not-allowed" })),
            Some(script) => {
                actions.insert(event.clone(), serde_json::to_value(script).unwrap_or(Value::Null));
            }
        }
    }
    actions
}

/// A widget row as a create places it: on its mapped page, with its look
/// and actions (`placementOf`).
fn placement_of(
    field: &Value,
    row: &Value,
    map: &PageMap,
    may_script: bool,
    dropped: &mut Vec<Value>,
) -> Value {
    let mut actions = Map::new();
    for event in WIDGET_ACTION_EVENTS {
        let Some(tree) = row.pointer(&format!("/actions/{event}")).filter(|t| !t.is_null()) else {
            continue;
        };
        let write = serde_json::from_value::<ActionTree<PageDestination>>(tree.clone())
            .ok()
            .and_then(|tree| action_write_of(&tree));
        let drop = |reason: &str| {
            json!({ "ref": field["ref"], "widget": row["ref"], "field": format!("actions.{event}"),
                "reason": reason })
        };
        match write {
            None => dropped.push(drop("unsupported-action")),
            Some(write) if !may_script && needs_script_right(&write) => {
                dropped.push(drop("script-not-allowed"));
            }
            Some(write) => {
                let write = serde_json::to_value(write).unwrap_or(Value::Null);
                actions.insert(event.to_owned(), map_page_refs(write, &|p| map.map(p)));
            }
        }
    }
    let family = field["family"].as_str().unwrap_or_default();
    let toggle = if matches!(family, "checkbox" | "radio") {
        let key = annotation_key_of(&row["ref"]);
        field["widgets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|widget| widget.get("ref").and_then(annotation_key_of) == key)
    } else {
        None
    };
    let page = serde_json::from_value::<PageRef>(row["page"].clone())
        .map(|page| json!(map.map(page)))
        .unwrap_or_else(|_| row["page"].clone());
    let mut placement = Map::new();
    placement.insert("page".into(), page);
    placement.insert("rect".into(), row["rect"].clone());
    if row["rotation"].as_f64().is_some_and(|turn| turn != 0.0) {
        placement.insert("rotation".into(), row["rotation"].clone());
    }
    for key in ["strokeWidth", "borderStyle", "textAlign"] {
        placement.insert(key.into(), row[key].clone());
    }
    // A new widget has none of these: only what the row has is written.
    let caption = if family == "pushbutton" { row["caption"].clone() } else { Value::Null };
    for (key, value) in [
        ("color", row["color"].clone()),
        ("interiorColor", row["interiorColor"].clone()),
        ("fontFamily", row["fontFamily"].clone()),
        ("fontSize", row["fontSize"].clone()),
        ("fontColor", row["fontColor"].clone()),
        ("caption", caption),
    ] {
        if !value.is_null() {
            placement.insert(key.into(), value);
        }
    }
    if let Some(export) = toggle.and_then(|t| t.get("exportValue")) {
        placement.insert("exportValue".into(), export.clone());
    }
    if !actions.is_empty() {
        placement.insert("actions".into(), Value::Object(actions));
    }
    Value::Object(placement)
}

/// A widget's flags, as its row has them (`flagsOf`).
fn flags_of(row: &Value) -> [Option<bool>; 10] {
    WIDGET_FLAG_NAMES.map(|name| row[name].as_bool())
}

/// The create a field row becomes, checked against the create schema
/// (`draftOf`).
fn draft_of(
    field: &Value,
    actions: Map<String, Value>,
    widgets: Vec<Value>,
    index: usize,
) -> Result<Map<String, Value>, EngineError> {
    let mut draft = Map::new();
    let family = field["family"].as_str().unwrap_or_default();
    draft.insert("name".into(), field["name"].clone());
    for key in ["readOnly", "required", "noExport"] {
        draft.insert(key.into(), field[key].clone());
    }
    // The group the field was in; the import decides whether it keeps it.
    for key in ["alternateName", "mappingName", "groupId"] {
        if !field[key].is_null() {
            draft.insert(key.into(), field[key].clone());
        }
    }
    if !actions.is_empty() {
        draft.insert("actions".into(), Value::Object(actions));
    }
    draft.insert("widgets".into(), Value::Array(widgets));
    let has_default =
        field.pointer("/defaultValueEntry/kind").and_then(Value::as_str) != Some("none");
    let options = || {
        Value::Array(
            field["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|option| json!({ "label": option["label"], "value": option["value"] }))
                .collect(),
        )
    };
    match family {
        "text" => {
            draft.insert("family".into(), json!("text"));
            if has_default {
                draft.insert("defaultValue".into(), field["defaultValue"].clone());
            }
            if !field["maxLength"].is_null() {
                draft.insert("maxLength".into(), field["maxLength"].clone());
            }
            for key in ["multiline", "password", "comb"] {
                draft.insert(key.into(), field[key].clone());
            }
        }
        "radio" => {
            draft.insert("family".into(), json!("radio"));
            for key in ["radiosInUnison", "noToggleToOff"] {
                draft.insert(key.into(), field[key].clone());
            }
        }
        "combobox" => {
            draft.insert("family".into(), json!("combobox"));
            draft.insert("edit".into(), field["edit"].clone());
            draft.insert("options".into(), options());
            if has_default {
                draft.insert("defaultValue".into(), field["defaultValue"].clone());
            }
        }
        "listbox" => {
            draft.insert("family".into(), json!("listbox"));
            draft.insert("multiSelect".into(), field["multiSelect"].clone());
            draft.insert("options".into(), options());
            if has_default {
                draft.insert("defaultValue".into(), field["defaultValue"].clone());
            }
        }
        "checkbox" | "pushbutton" | "signature" => {
            draft.insert("family".into(), json!(family));
        }
        _ => {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("import: field {index} has no family"),
            ));
        }
    }
    check_field_draft(&draft).map_err(|(path, message)| {
        let at = if path.is_empty() { String::new() } else { format!(" member '{path}'") };
        let mut details = json!({ "item": index });
        if !path.is_empty() {
            details["field"] = json!(path);
        }
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("import: field {index}{at}: {message}"),
        )
        .with_details(details)
    })?;
    Ok(draft)
}

/// The members a family's draft takes besides the base ones
/// (`FormFieldDraftSchema`, strict).
fn family_members(family: &str) -> &'static [&'static str] {
    match family {
        "text" => &["defaultValue", "maxLength", "multiline", "password", "comb"],
        "radio" => &["radiosInUnison", "noToggleToOff"],
        "combobox" => &["edit", "options", "defaultValue"],
        "listbox" => &["multiSelect", "options", "defaultValue"],
        _ => &[],
    }
}

const BASE_MEMBERS: &[&str] = &[
    "family",
    "name",
    "readOnly",
    "required",
    "noExport",
    "alternateName",
    "mappingName",
    "groupId",
    "actions",
    "widgets",
];

/// The create schema's check of a draft (`FormFieldDraftSchema`), the
/// first refusal as its path and message, as zod's first issue names it.
/// The shape the create reads is then read whole
/// (`FormFieldDraft<PageCoordinates>`).
fn check_field_draft(draft: &Map<String, Value>) -> Result<(), (String, String)> {
    let family = draft["family"].as_str().unwrap_or_default();
    let own = family_members(family);
    if let Some(key) =
        draft.keys().find(|k| !BASE_MEMBERS.contains(&k.as_str()) && !own.contains(&k.as_str()))
    {
        return Err((String::new(), format!("Unrecognized key: \"{key}\"")));
    }
    let fail = |path: &str, message: &str| Err((path.to_owned(), message.to_owned()));
    if draft["name"].as_str().is_none_or(str::is_empty) {
        return fail("name", "Too small: expected string to have >=1 characters");
    }
    for key in [
        "readOnly",
        "required",
        "noExport",
        "multiline",
        "password",
        "comb",
        "radiosInUnison",
        "noToggleToOff",
        "edit",
        "multiSelect",
    ] {
        if draft.get(key).is_some_and(|v| !v.is_boolean()) {
            return fail(key, "Invalid input: expected boolean");
        }
    }
    for key in ["alternateName", "mappingName"] {
        if draft.get(key).is_some_and(|v| !v.is_string()) {
            return fail(key, "Invalid input: expected string");
        }
    }
    if let Some(group) = draft.get("groupId")
        && group.as_str().is_none_or(|g| g.is_empty() || g.chars().count() > 256)
    {
        return fail("groupId", "expected a string of 1 to 256 characters");
    }
    if let Some(limit) = draft.get("maxLength")
        && limit.as_f64().is_none_or(|n| n.fract() != 0.0 || n <= 0.0)
    {
        return fail("maxLength", "Too small: expected number to be >0");
    }
    if let Some(default) = draft.get("defaultValue") {
        let fits = if family == "listbox" {
            default.as_array().is_some_and(|values| values.iter().all(Value::is_string))
        } else {
            default.is_string()
        };
        if !fits {
            return fail("defaultValue", "Invalid input");
        }
    }
    for (at, widget) in draft["widgets"].as_array().into_iter().flatten().enumerate() {
        let Some(placement) = widget.as_object() else {
            return fail(&format!("widgets.{at}"), "Invalid input: expected object");
        };
        if let Some(export) = placement.get("exportValue")
            && export.as_str().is_none_or(str::is_empty)
        {
            return fail(
                &format!("widgets.{at}.exportValue"),
                "Too small: expected string to have >=1 characters",
            );
        }
    }
    serde_json::from_value::<
        crate::features::forms::field::FormFieldDraft<crate::page_space::PageCoordinates>,
    >(Value::Object(draft.clone()))
    .map(|_| ())
    .map_err(|e| (String::new(), e.to_string()))
}

/// The facts a design import committed: one `forms.created` per field, in
/// bundle order, each with its own widgets and the part of the meta that is
/// its own; the calculation order, when the import changed it, rides on the
/// last (`formImportFacts`). `result` is the import's result as JSON.
pub fn form_import_facts(result: &Value) -> Vec<Value> {
    let fields = result["fields"].as_array().cloned().unwrap_or_default();
    let last = fields.len().saturating_sub(1);
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let own = own_widgets(field);
            let mut fact = field_fact(field, result, &own);
            if index == last && !result["calculationOrder"].is_null() {
                fact["calculationOrder"] = result["calculationOrder"].clone();
            }
            if index != last {
                fact["meta"]["cacheDelta"] = Value::Null;
            }
            let mut event = json!({ "type": "forms.created" });
            event
                .as_object_mut()
                .expect("an object")
                .extend(fact.as_object().cloned().unwrap_or_default());
            event
        })
        .collect()
}

/// The facts a values import committed: one `forms.valueSet` per field it
/// filled, as a reset's (`formValuesImportFacts`, `formResetFacts`).
pub fn form_values_import_facts(result: &Value) -> Vec<Value> {
    result["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|field| {
            let own = own_widgets(field);
            let mut fact = field_fact(field, result, &own);
            fact["meta"]["cacheDelta"] = Value::Null;
            let mut event = json!({ "type": "forms.valueSet" });
            event
                .as_object_mut()
                .expect("an object")
                .extend(fact.as_object().cloned().unwrap_or_default());
            event
        })
        .collect()
}

/// The object numbers of a field's widgets.
fn own_widgets(field: &Value) -> Vec<u64> {
    field["widgets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|widget| widget["objectNumber"].as_u64())
        .collect()
}

/// `{ field, widgets, meta }` for one field of a form import's result: its
/// own widget rows and the part of the meta that is its own.
fn field_fact(field: &Value, result: &Value, own: &[u64]) -> Value {
    let mine = |widget: &Value| widget["objectNumber"].as_u64().is_some_and(|n| own.contains(&n));
    let widgets: Vec<Value> = result["widgets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            row["ref"]["kind"] == "objectNumber"
                && row["ref"]["objectNumber"].as_u64().is_some_and(|n| own.contains(&n))
        })
        .cloned()
        .collect();
    let meta = &result["meta"];
    let changed_widgets: Vec<Value> = meta["changedWidgets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| mine(w))
        .cloned()
        .collect();
    let pages: Vec<Value> = changed_widgets
        .iter()
        .filter_map(|widget| widget["page"].get("objectNumber").cloned())
        .collect();
    let affected: Vec<Value> = meta["affectedPages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|page| pages.contains(&page["objectNumber"]))
        .cloned()
        .collect();
    let redrawn: Vec<Value> = meta["redrawnWidgets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| mine(w))
        .cloned()
        .collect();
    json!({
        "field": field,
        "widgets": widgets,
        "meta": {
            "affectedPages": affected,
            "cacheDelta": meta["cacheDelta"],
            "opId": meta["opId"],
            "undoable": meta["undoable"],
            "changedFields": [field["ref"]],
            "changedWidgets": changed_widgets,
            "redrawnWidgets": redrawn,
        },
    })
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/formTransfer.test.ts`.

    use super::*;
    use crate::features::transfer::BundleKind;
    use crate::features::transfer::bundle::Bundle;
    use crate::features::transfer::codec;
    use crate::features::transfer::form_export::{FormExportSelection, form_export_rows_of};
    use crate::features::transfer::limits::DEFAULT_BUNDLE_LIMITS;
    use crate::identity::PageObjectNumber;

    fn page() -> PageRef {
        PageRef::new(PageObjectNumber::new(3).unwrap())
    }

    fn ref_of(number: u64) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": number })
    }

    fn widget_ref(number: u64) -> Value {
        json!({ "kind": "objectNumber", "page": page(), "objectNumber": number })
    }

    fn tree(node: Value) -> Value {
        let mut root = json!({ "subtype": "", "next": [] });
        root.as_object_mut().unwrap().extend(node.as_object().unwrap().clone());
        json!({ "root": root, "incomplete": false, "warningFlags": 0, "warnings": [] })
    }

    fn with(mut value: Value, fields: Value) -> Value {
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        value
    }

    /// A text field as a read returns it, with one widget per number.
    fn text_field(name: &str, number: u64, widgets: &[u64], fields: Value) -> Value {
        with(
            json!({
                "ref": ref_of(number), "name": name, "family": "text", "origin": "acroform",
                "readOnly": false, "required": false, "noExport": false,
                "alternateName": null, "mappingName": null,
                "valueEntry": { "kind": "scalar", "value": format!("{name} value") },
                "defaultValueEntry": { "kind": "none" },
                "createdBy": "alice", "createdAt": "2026-10-08T10:42:00Z",
                "filledBy": "alice", "filledByName": "Alice", "filledAt": "2026-10-08T10:43:00Z",
                "importedBy": null, "groupId": null,
                "value": format!("{name} value"), "defaultValue": "", "maxLength": null,
                "multiline": false, "password": false, "comb": false,
                "widgets": widgets.iter().map(|n| json!({ "ref": widget_ref(*n), "objectNumber": n,
                    "page": page() })).collect::<Vec<_>>(),
            }),
            fields,
        )
    }

    /// A widget row as a read returns it.
    fn widget_row(number: u64, field: Value, fields: Value) -> Value {
        with(
            json!({
                "subtype": "widget", "ref": widget_ref(number), "page": page(), "field": field,
                "rect": { "x": 20, "y": 20, "width": 120, "height": 24 },
                "strokeWidth": 1, "borderStyle": "solid", "textAlign": "left",
                "color": "#000000", "interiorColor": null, "fontFamily": "helvetica",
                "fontSize": 12, "fontColor": "#000000", "caption": null, "actions": null,
                "invisible": false, "hidden": false, "print": true, "noZoom": false,
                "noRotate": false, "noView": false, "readOnly": false, "locked": false,
                "toggleNoView": false, "lockedContents": false,
            }),
            fields,
        )
    }

    fn bundle_pages() -> Vec<BundlePage> {
        vec![BundlePage {
            page: page(),
            position: 0,
            size: crate::geom::PdfSize { width: 300.0, height: 300.0 },
        }]
    }

    struct Rows {
        fields: Vec<Value>,
        widgets: Vec<Value>,
        order: Vec<Value>,
    }

    fn rows(fields: &[Value], widgets: &[Value], order: &[Value]) -> Rows {
        Rows {
            fields: fields.iter().map(|data| json!({ "data": data })).collect(),
            widgets: widgets.iter().map(|data| json!({ "data": data, "resources": {} })).collect(),
            order: order.to_vec(),
        }
    }

    fn plan(
        rows: &Rows,
        taken: &[&str],
        may_script: bool,
        values: bool,
    ) -> Result<FormImportPlan, EngineError> {
        let pages = bundle_pages();
        let target = [BundleImportTarget { page: page(), position: 0 }];
        plan_form_import(FormPlanInput {
            pages: &pages,
            fields: &rows.fields,
            widgets: &rows.widgets,
            calculation_order: &rows.order,
            page_mapping: &BundleImportPages::Same,
            target: &target,
            name_taken: &|name| taken.contains(&name),
            may_script,
            values,
        })
    }

    #[test]
    fn makes_a_create_per_field_with_its_widgets_placed_and_its_value() {
        let name = text_field("customer.name", 10, &[11], json!({}));
        let bundle = rows(
            std::slice::from_ref(&name),
            &[widget_row(11, name["ref"].clone(), json!({}))],
            &[],
        );
        let planned = plan(&bundle, &[], true, true).unwrap();
        assert!(planned.dropped.is_empty(), "{:?}", planned.dropped);
        assert_eq!(planned.creates.len(), 1);
        let create = &planned.creates[0];
        assert_eq!((create.field, create.widgets.clone()), (0, vec![0]));
        assert_eq!(create.value, Some(FormFieldValue::Value(Some("customer.name value".into()))));
        assert_eq!(create.draft["family"], "text");
        assert_eq!(create.draft["name"], "customer.name");
        let placed = &create.draft["widgets"][0];
        assert_eq!(placed["page"], json!(page()));
        assert_eq!(placed["rect"], json!({ "x": 20, "y": 20, "width": 120, "height": 24 }));
        assert_eq!(placed["fontSize"], 12);
        assert_eq!(create.flags[0][2], Some(true), "print");
        assert_eq!(create.flags[0][1], Some(false), "hidden");
        assert_eq!(plan(&bundle, &[], true, false).unwrap().creates[0].value, None);
    }

    #[test]
    fn leaves_out_a_taken_name_a_name_twice_an_unknown_family_a_widget_without_its_field() {
        let a = text_field("a", 10, &[11], json!({}));
        let again = text_field("a", 20, &[21], json!({}));
        let taken = text_field("taken", 30, &[], json!({}));
        let unknown = with(text_field("odd", 40, &[], json!({})), json!({ "family": "unknown" }));
        let bundle = rows(
            &[a.clone(), again.clone(), taken.clone(), unknown.clone()],
            &[
                widget_row(11, a["ref"].clone(), json!({})),
                widget_row(21, again["ref"].clone(), json!({})),
                widget_row(99, ref_of(98), json!({})),
            ],
            &[],
        );
        let planned = plan(&bundle, &["taken"], true, true).unwrap();
        let names: Vec<&Value> = planned.creates.iter().map(|c| &c.draft["name"]).collect();
        assert_eq!(names, [&json!("a")]);
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": again["ref"], "reason": "name-conflict" }),
                json!({ "ref": taken["ref"], "reason": "name-conflict" }),
                json!({ "ref": unknown["ref"], "reason": "unsupported-family" }),
                json!({ "ref": ref_of(98), "widget": widget_ref(99), "reason": "no-field" }),
            ]
        );
    }

    #[test]
    fn without_doc_forms_script_leaves_out_scripts_submits_and_links() {
        let field = text_field(
            "total",
            10,
            &[11],
            json!({
                "actions": { "calculate": tree(json!({ "type": "javascript", "script": "event.value = 1;" })) },
            }),
        );
        let row = widget_row(
            11,
            field["ref"].clone(),
            json!({
                "actions": {
                    "activate": tree(json!({ "type": "uri", "uri": "https://example.com", "isMap": false })),
                    "focus": tree(json!({ "type": "reset-form", "fields": null, "exclude": false })),
                },
            }),
        );
        let bundle =
            rows(std::slice::from_ref(&field), std::slice::from_ref(&row), &[field["ref"].clone()]);
        let planned = plan(&bundle, &[], false, true).unwrap();
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": field["ref"], "field": "actions.calculate", "reason": "script-not-allowed" }),
                json!({ "ref": field["ref"], "widget": row["ref"], "field": "actions.activate",
                    "reason": "script-not-allowed" }),
            ]
        );
        assert!(!planned.creates[0].draft.contains_key("actions"));
        assert_eq!(
            planned.creates[0].draft["widgets"][0]["actions"],
            json!({ "focus": { "type": "reset-form", "fields": null, "exclude": false } })
        );
        // Without its calculate script, the copy has no place in the order.
        assert!(planned.calculation_order.is_empty());
        assert_eq!(plan(&bundle, &[], true, true).unwrap().calculation_order, [0]);
    }

    #[test]
    fn refuses_a_field_whose_data_a_create_would_refuse_naming_it() {
        let bad = text_field("bad", 10, &[], json!({ "maxLength": -1 }));
        let refused = plan(&rows(&[bad], &[], &[]), &[], true, true).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        let details = refused.details.unwrap();
        assert_eq!(
            (details["item"].clone(), details["field"].clone()),
            (json!(0), json!("maxLength"))
        );
    }

    #[test]
    fn a_values_import_fills_by_full_name_and_leaves_out_in_bundle_order_what_it_cannot() {
        let name = text_field("name", 10, &[], json!({}));
        let missing = text_field("missing", 20, &[], json!({}));
        let r#box = text_field("box", 30, &[], json!({}));
        let locked = text_field("locked", 40, &[], json!({}));
        let empty = text_field("empty", 50, &[], json!({ "valueEntry": { "kind": "none" } }));
        let target = [
            json!({ "ref": ref_of(110), "name": "name", "family": "text" }),
            json!({ "ref": ref_of(130), "name": "box", "family": "checkbox" }),
            json!({ "ref": ref_of(140), "name": "locked", "family": "text" }),
            json!({ "ref": ref_of(150), "name": "empty", "family": "text" }),
        ];
        let bundle = rows(&[name, missing.clone(), r#box.clone(), locked.clone(), empty], &[], &[]);
        let planned = plan_form_values_import(&bundle.fields, &target, &mut |at, _| {
            Ok((target[at]["name"] == "locked").then_some("locked"))
        })
        .unwrap();
        assert_eq!(
            planned.writes,
            [PlannedValueWrite {
                field: 0,
                target: 0,
                value: FormFieldValue::Value(Some("name value".into())),
            }]
        );
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": missing["ref"], "reason": "no-field" }),
                json!({ "ref": r#box["ref"], "reason": "wrong-family" }),
                json!({ "ref": locked["ref"], "reason": "locked" }),
            ]
        );
    }

    fn snapshot_of(fields: &[Value]) -> Value {
        json!({ "fields": fields, "widgets": [],
            "calculationOrder": fields.iter().map(|f| f["ref"].clone()).collect::<Vec<_>>() })
    }

    #[test]
    fn an_export_takes_whole_fields_a_signature_unsigned_a_quiet_field_without_its_value() {
        let signed = with(
            text_field("sign", 10, &[], json!({})),
            json!({ "family": "signature", "valueEntry": { "kind": "scalar", "value": "12 0 R" } }),
        );
        let quiet = text_field("quiet", 20, &[], json!({ "noExport": true }));
        let exported = form_export_rows_of(
            &snapshot_of(&[signed.clone(), quiet.clone()]),
            &FormExportSelection::default(),
            &[page()],
        )
        .unwrap();
        assert_eq!(exported.fields[0]["valueEntry"], json!({ "kind": "none" }));
        let field = &exported.fields[1];
        assert_eq!(field["value"], "");
        assert_eq!(field["valueEntry"], json!({ "kind": "none" }));
        assert_eq!(
            (field["filledBy"].clone(), field["filledAt"].clone()),
            (Value::Null, Value::Null)
        );
        assert_eq!(field["createdBy"], "alice");
        assert_eq!(exported.calculation_order, [signed["ref"].clone(), quiet["ref"].clone()]);
    }

    #[test]
    fn an_export_refuses_a_field_or_a_page_the_document_does_not_have() {
        let snapshot = snapshot_of(&[text_field("a", 10, &[], json!({}))]);
        let by_name: FormExportSelection =
            serde_json::from_value(json!({ "fields": [{ "kind": "fqn", "name": "b" }] })).unwrap();
        assert_eq!(
            form_export_rows_of(&snapshot, &by_name, &[page()]).unwrap_err().code,
            EngineErrorCode::NotFound
        );
        let by_page: FormExportSelection = serde_json::from_value(
            json!({ "pages": [{ "kind": "objectNumber", "objectNumber": 9 }] }),
        )
        .unwrap();
        assert_eq!(
            form_export_rows_of(&snapshot, &by_page, &[page()]).unwrap_err().code,
            EngineErrorCode::NotFound
        );
    }

    fn form_bundle() -> Bundle {
        let name = text_field("name", 10, &[11], json!({}));
        let manifest = json!({
            "format": "embedpdf/form", "version": 1,
            "pages": [{ "page": page(), "position": 0, "size": { "width": 300, "height": 300 } }],
            "fields": [{ "data": name }],
            "widgets": [{ "data": widget_row(11, name["ref"].clone(), json!({})), "resources": {} }],
            "calculationOrder": [name["ref"]],
        });
        Bundle { manifest: manifest.as_object().unwrap().clone(), resources: Vec::new() }
    }

    #[test]
    fn a_form_bundle_comes_back_through_the_file_as_the_same_bundle() {
        let bundle = form_bundle();
        let text = codec::stringify(BundleKind::Form, &bundle, &DEFAULT_BUNDLE_LIMITS).unwrap();
        assert_eq!(codec::parse(BundleKind::Form, &text, &DEFAULT_BUNDLE_LIMITS).unwrap(), bundle);
    }

    #[test]
    fn a_form_file_refuses_another_familys_file_and_one_past_a_limit() {
        let bundle = form_bundle();
        let text = codec::stringify(BundleKind::Form, &bundle, &DEFAULT_BUNDLE_LIMITS).unwrap();
        let mut annotations: Value = serde_json::from_str(&text).unwrap();
        annotations["format"] = json!("embedpdf/annotations");
        let refused =
            codec::parse(BundleKind::Form, &annotations.to_string(), &DEFAULT_BUNDLE_LIMITS)
                .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        let limits = crate::features::transfer::BundleLimits { items: 1, ..DEFAULT_BUNDLE_LIMITS };
        let refused = codec::parse(BundleKind::Form, &text, &limits).unwrap_err();
        assert_eq!(refused.details.unwrap()["limit"], "items");
    }
}
