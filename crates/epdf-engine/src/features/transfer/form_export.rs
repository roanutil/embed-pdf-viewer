//! Which fields a form export takes (`core/src/transfer/formExport.ts`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::bundle::page_key;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::forms::snapshot::FormFieldRef;
use crate::identity::PageRef;

/// Which fields an export takes, each whole (`FormExportSelection`).
/// Without `fields` or `pages`, every field.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormExportSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<FormFieldRef>>,
    /// Every field with a widget on one of these pages, with all of its
    /// widgets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<PageRef>>,
}

/// What an export of a form takes, before the page table and resources
/// (`FormExportRows`): JSON rows as a read returns them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormExportRows {
    pub fields: Vec<Value>,
    pub widgets: Vec<Value>,
    pub calculation_order: Vec<Value>,
}

/// `encodeFieldRefKey`.
pub fn field_ref_key(reference: &FormFieldRef) -> String {
    match reference {
        FormFieldRef::ObjectNumber { field } => format!("obj:{field}"),
        FormFieldRef::Fqn { name } => format!("fqn:{name}"),
    }
}

/// [`field_ref_key`] of a ref as JSON; empty for anything else.
pub fn field_ref_value_key(reference: &Value) -> String {
    serde_json::from_value::<FormFieldRef>(reference.clone())
        .map(|r| field_ref_key(&r))
        .unwrap_or_default()
}

/// Whether `field` is the one `reference` names, by its object number or
/// its full name (`fieldMatches`).
pub fn field_matches(field: &Value, reference: &FormFieldRef) -> bool {
    match reference {
        FormFieldRef::Fqn { name } => field.get("name").and_then(Value::as_str) == Some(name),
        FormFieldRef::ObjectNumber { field: number } => {
            field.pointer("/ref/kind").and_then(Value::as_str) == Some("objectNumber")
                && field.pointer("/ref/objectNumber").and_then(Value::as_u64)
                    == Some(u64::from(*number))
        }
    }
}

/// An annotation ref's key (`annotationKey`), of a ref as JSON.
pub fn annotation_key_of(reference: &Value) -> Option<String> {
    serde_json::from_value::<crate::identity::AnnotationRef>(reference.clone())
        .ok()
        .map(|r| r.key())
}

/// The rows an export of `snapshot` takes for `selection`, in the form's
/// order: whole fields, each with every widget it has, whatever page they're
/// on; the fields' widget rows, in their fields' order; and the calculation
/// order among them. A field the form doesn't export (`noExport`) goes
/// without its value and who filled it, a signature field unsigned. `pages`
/// is the document's pages; a selected field or page the document doesn't
/// have is `NotFound` (`formExportRowsOf`). `snapshot` is a form read
/// (`FormSnapshot`) as JSON.
pub fn form_export_rows_of(
    snapshot: &Value,
    selection: &FormExportSelection,
    pages: &[PageRef],
) -> Result<FormExportRows, EngineError> {
    let fields = snapshot.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut chosen: HashSet<usize> = HashSet::new();
    for reference in selection.fields.iter().flatten() {
        let Some(at) = fields.iter().position(|field| field_matches(field, reference)) else {
            let named = match reference {
                FormFieldRef::Fqn { name } => name.clone(),
                other => field_ref_key(other),
            };
            return Err(EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("export: the form has no field {named}"),
            ));
        };
        chosen.insert(at);
    }
    let in_document: HashSet<String> = pages.iter().map(|page| page_key(*page)).collect();
    let mut selected_pages: HashSet<String> = HashSet::new();
    for page in selection.pages.iter().flatten() {
        let key = page_key(*page);
        if !in_document.contains(&key) {
            return Err(EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("export: the document has no page {key}"),
            ));
        }
        selected_pages.insert(key);
    }
    for (at, field) in fields.iter().enumerate() {
        let on_selected = field["widgets"].as_array().into_iter().flatten().any(|widget| {
            serde_json::from_value::<PageRef>(widget["page"].clone())
                .is_ok_and(|page| selected_pages.contains(&page_key(page)))
        });
        if on_selected {
            chosen.insert(at);
        }
    }
    let every_field = selection.fields.is_none() && selection.pages.is_none();
    let taken: Vec<&Value> = fields
        .iter()
        .enumerate()
        .filter(|(at, _)| every_field || chosen.contains(at))
        .map(|(_, field)| field)
        .collect();

    let rows = snapshot.get("widgets").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut widgets = Vec::new();
    for field in &taken {
        for widget in field["widgets"].as_array().into_iter().flatten() {
            let Some(key) = annotation_key_of(&widget["ref"]) else { continue };
            if let Some(row) =
                rows.iter().find(|row| annotation_key_of(&row["ref"]).as_ref() == Some(&key))
            {
                widgets.push(row.clone());
            }
        }
    }
    let exported: HashSet<String> =
        taken.iter().map(|field| field_ref_value_key(&field["ref"])).collect();
    let calculation_order = snapshot
        .get("calculationOrder")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|reference| {
            !reference.is_null() && exported.contains(&field_ref_value_key(reference))
        })
        .cloned()
        .collect();
    Ok(FormExportRows {
        fields: taken.into_iter().map(|field| exported_field(field.clone())).collect(),
        widgets,
        calculation_order,
    })
}

/// A field as an export carries it: one the form doesn't export without its
/// value and who filled it, and a signature field without its signature
/// (`exportedField`).
fn exported_field(mut field: Value) -> Value {
    let family = field["family"].as_str().unwrap_or_default().to_owned();
    if family == "signature" {
        field["valueEntry"] = json!({ "kind": "none" });
        return field;
    }
    if field["noExport"] != Value::Bool(true) {
        return field;
    }
    field["valueEntry"] = json!({ "kind": "none" });
    field["filledBy"] = Value::Null;
    field["filledByName"] = Value::Null;
    field["filledAt"] = Value::Null;
    match family.as_str() {
        "text" | "combobox" => field["value"] = json!(""),
        "checkbox" => {
            field["checked"] = json!(false);
            uncheck_widgets(&mut field);
        }
        "radio" => {
            field["value"] = json!("Off");
            uncheck_widgets(&mut field);
        }
        "listbox" => {
            field["selectedValues"] = json!([]);
            for option in field["options"].as_array_mut().into_iter().flatten() {
                option["selected"] = json!(false);
            }
        }
        _ => {}
    }
    field
}

fn uncheck_widgets(field: &mut Value) {
    for widget in field["widgets"].as_array_mut().into_iter().flatten() {
        widget["checked"] = json!(false);
    }
}
