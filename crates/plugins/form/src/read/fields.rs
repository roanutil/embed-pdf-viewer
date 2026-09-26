//! `read/fields.ts`: reads over the reconciled snapshot: fields, values,
//! the widget-to-field map.

use serde_json::{Map, Value, json};

use crate::contract::FormFilter;
use crate::controller::{FormCapability, key_of};
use crate::core::model::{field_by_key, field_for_widget};
use crate::value::{get, num, str_of};

/// `widgetObjectOf(widget)`: a `WidgetAddress` (an annotation ref, or
/// `{annotObjectNumber}` alone) as its object number; 0 for a ref of
/// another kind.
pub(crate) fn widget_object_of(widget: &Value) -> f64 {
    if widget.get("kind").is_some() {
        return if str_of(widget, "kind") == Some("objectNumber") {
            num(widget, "annotObjectNumber").unwrap_or(f64::NAN)
        } else {
            0.0
        };
    }
    num(widget, "annotObjectNumber").unwrap_or(f64::NAN)
}

fn snapshot_fields(snapshot: Option<&Value>) -> Vec<Value> {
    snapshot.and_then(|s| get(s, "fields")).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `valueOf(field)`: the value in the write vocabulary, or `None` for a
/// valueless or unsupported entry.
pub(crate) fn value_of(field: &Value) -> Option<Value> {
    let entry = get(field, "valueEntry")?;
    let kind = str_of(entry, "kind");
    if matches!(kind, Some("none" | "unsupported")) {
        return None;
    }
    let scalar = kind == Some("scalar");
    let value = || get(entry, "value").cloned().unwrap_or(Value::Null);
    let values = || get(entry, "values").and_then(Value::as_array).cloned().unwrap_or_default();
    match str_of(field, "family") {
        Some("checkbox" | "radio") => {
            let state =
                if scalar { value() } else { values().into_iter().next().unwrap_or(Value::Null) };
            Some(json!({ "type": "toggle", "state": state }))
        }
        Some("combobox" | "listbox") => {
            let values = if scalar { vec![value()] } else { values() };
            Some(json!({ "type": "choice", "values": values }))
        }
        _ => {
            let text = if scalar {
                value()
            } else {
                let parts: Vec<String> =
                    values().iter().map(|v| crate::value::string_of(Some(v))).collect();
                json!(parts.join("\n"))
            };
            Some(json!({ "type": "text", "value": text }))
        }
    }
}

impl FormCapability {
    /// `getSnapshot()`: the reconciled field tree, null until it lands.
    pub fn get_snapshot(&self) -> Option<Value> {
        self.model().snapshot.clone()
    }

    /// `getField(ref)`.
    pub fn get_field(&self, field_ref: &Value) -> Option<Value> {
        field_by_key(&self.model(), &key_of(field_ref))
    }

    /// `getFieldForWidget(widget)`: the field a widget belongs to.
    pub fn get_field_for_widget(&self, widget: &Value) -> Option<Value> {
        field_for_widget(&self.model(), widget_object_of(widget))
    }

    /// `listFields(filter?)`: every field, or those matching each given
    /// member of the filter (a widget on `page`; an unplaced widget matches
    /// no page).
    pub fn list_fields(&self, filter: Option<&FormFilter>) -> Vec<Value> {
        let fields = snapshot_fields(self.model().snapshot.as_ref());
        let Some(filter) = filter else { return fields };
        fields
            .into_iter()
            .filter(|f| {
                filter
                    .family
                    .as_deref()
                    .filter(|family| !family.is_empty())
                    .is_none_or(|family| str_of(f, "family") == Some(family))
                    && filter.name.as_deref().is_none_or(|name| str_of(f, "name") == Some(name))
                    && filter.page.is_none_or(|page| {
                        get(f, "widgets").and_then(Value::as_array).into_iter().flatten().any(|w| {
                            get(w, "page")
                                .filter(|p| !p.is_null())
                                .and_then(|p| num(p, "pageObjectNumber"))
                                == Some(f64::from(page.page_object_number))
                        })
                    })
            })
            .collect()
    }

    /// `getValue(ref)`: the current value in the write vocabulary, or null
    /// for a valueless, unsupported or unknown field.
    pub fn get_value(&self, field_ref: &Value) -> Option<Value> {
        self.get_field(field_ref).and_then(|f| value_of(&f))
    }

    /// `exportValues()`: values keyed by fully qualified name, valueless and
    /// unsupported entries omitted.
    pub fn export_values(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for field in snapshot_fields(self.model().snapshot.as_ref()) {
            if let Some(value) = value_of(&field) {
                out.insert(crate::value::string_of(get(&field, "name")), value);
            }
        }
        out
    }
}
