//! `read/fields.ts`: reads over the mirrored field tree: fields, values, the
//! joins between a field and its widgets' rows, the field of the selected
//! widget, and the required-field check.

use std::rc::Rc;

use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::engine::page_refs_equal;
use crate::model::{canonical_key, field_by_key, field_by_ref, field_for_widget, widget_row_of};
use crate::value::{get, items, str_of, truthy};

/// `widgetObjectOf(widget)`: a widget address (an annotation ref, or a
/// widget as a field names it) as the object number the index is keyed by.
pub fn widget_object_of(widget: &Value) -> f64 {
    let number = || get(widget, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
    if widget.as_object().is_some_and(|o| o.contains_key("kind")) {
        if str_of(widget, "kind") == Some("objectNumber") { number() } else { 0.0 }
    } else {
        number()
    }
}

/// `valueOf(field)`: a field's value in the write vocabulary, or none for a
/// valueless or unsupported entry.
pub fn value_of(field: &Value) -> Option<Value> {
    let kind = get(field, "valueEntry").and_then(|e| str_of(e, "kind"));
    if matches!(kind, Some("none" | "unsupported")) {
        return None;
    }
    let member = |key: &str| get(field, key).cloned().unwrap_or(Value::Null);
    match str_of(field, "family") {
        Some("text") => Some(json!({ "value": member("value") })),
        Some("checkbox") => Some(json!({ "checked": member("checked") })),
        Some("radio") => {
            let value = member("value");
            Some(json!({ "value": if value == json!("Off") { Value::Null } else { value } }))
        }
        Some("combobox") => {
            let value = member("value");
            Some(json!({ "value": if value == json!("") { Value::Null } else { value } }))
        }
        Some("listbox") => Some(json!({ "selectedValues": member("selectedValues") })),
        _ => None,
    }
}

/// `plainValueOf(field)`: a field's value as plain data; none (`undefined`)
/// for a field without one.
pub fn plain_value_of(field: &Value) -> Option<Value> {
    let value = value_of(field)?;
    if let Some(checked) = value.get("checked") {
        return Some(checked.clone());
    }
    if let Some(values) = value.get("selectedValues") {
        return Some(values.clone());
    }
    Some(value.get("value").cloned().unwrap_or(Value::Null))
}

/// `writeOfPlainValue(field, value)`: the write a plain value makes for a
/// field of this family, or none when the family can't take it.
pub fn write_of_plain_value(field: &Value, value: &Value) -> Option<Value> {
    match str_of(field, "family") {
        Some("checkbox") => match value {
            Value::Bool(b) => Some(json!({ "checked": b })),
            Value::String(_) | Value::Null => Some(json!({ "value": value })),
            _ => None,
        },
        Some("text" | "radio" | "combobox") => match value {
            Value::String(_) | Value::Null => Some(json!({ "value": value })),
            _ => None,
        },
        Some("listbox") => match value {
            Value::Null => Some(json!({ "selectedValues": [] })),
            Value::String(s) => Some(json!({ "selectedValues": [s] })),
            Value::Array(values) => Some(json!({ "selectedValues": values })),
            _ => None,
        },
        _ => None,
    }
}

/// `isEmpty(field)`: whether a field holds nothing a required check would accept.
fn is_empty(field: &Value) -> bool {
    let member = |key: &str| get(field, key);
    match str_of(field, "family") {
        Some("text" | "combobox") => member("value") == Some(&json!("")),
        Some("checkbox") => !truthy(member("checked")),
        Some("radio") => {
            member("value") == Some(&json!("Off")) || member("value") == Some(&json!(""))
        }
        Some("listbox") => items(field, "selectedValues").is_empty(),
        Some("signature") => {
            get(field, "valueEntry").and_then(|e| str_of(e, "kind")) == Some("none")
        }
        _ => false,
    }
}

impl crate::controller::Core {
    /// `getSnapshot()`: everything at once; `None` before the first read.
    pub fn get_snapshot(&self) -> Option<Rc<Value>> {
        self.view().snapshot.clone()
    }

    /// `getStatus()`: whether the form is read.
    pub fn get_status(&self) -> ResourceStatus {
        self.fields.get_status()
    }

    /// `getFormKind()`: `'none'`, `'acroform'` or `'xfa'`.
    pub fn get_form_kind(&self) -> Value {
        self.view()
            .snapshot
            .as_deref()
            .and_then(|s| get(s, "formKind").cloned())
            .unwrap_or_else(|| json!("none"))
    }

    /// `refresh(options)`: read the whole form again.
    pub fn refresh(&self, signal: Option<AbortSignal>) -> Promise<()> {
        self.cancellable(signal, self.fields.refresh())
    }

    /// `get(ref)`: one field, or `None`.
    pub fn get(&self, reference: &Value) -> Option<Value> {
        field_by_ref(&self.view(), reference).cloned()
    }

    /// `getFieldForWidget(widget)`.
    pub fn get_field_for_widget(&self, widget: &Value) -> Option<Value> {
        field_for_widget(&self.view(), widget_object_of(widget)).cloned()
    }

    /// `getWidget(widget)`: a widget's row, or `None`.
    pub fn get_widget(&self, widget: &Value) -> Option<Value> {
        widget_row_of(&self.view(), widget_object_of(widget)).cloned()
    }

    /// `getWidgets(ref)`: every place a field shows, in the field's order.
    pub fn get_widgets(&self, reference: &Value) -> Vec<Value> {
        let index = self.view();
        let key = canonical_key(&index, reference);
        let Some(field) = field_by_key(&index, &key) else { return Vec::new() };
        items(field, "widgets")
            .iter()
            .filter_map(|w| {
                let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
                widget_row_of(&index, n).cloned()
            })
            .collect()
    }

    /// `list(filter)`: every field, or the ones that match every given part.
    pub fn list(&self, filter: Option<&Value>) -> Vec<Value> {
        let index = self.view();
        let all = index.fields();
        let Some(filter) = filter else { return all.to_vec() };
        let page_arg = get(filter, "page");
        let page =
            page_arg.and_then(|p| self.get_page(p)).and_then(|info| get(&info, "ref").cloned());
        // A page that isn't in the document has no fields.
        if page_arg.is_some() && page.is_none() {
            return Vec::new();
        }
        let family = get(filter, "family").filter(|f| truthy(Some(f)));
        let name = get(filter, "name");
        all.iter()
            .filter(|field| {
                family.is_none_or(|f| crate::value::strict_eq(get(field, "family"), Some(f)))
                    && name.is_none_or(|n| crate::value::strict_eq(get(field, "name"), Some(n)))
                    && page.as_ref().is_none_or(|page| {
                        items(field, "widgets").iter().any(|w| {
                            get(w, "page")
                                .filter(|p| truthy(Some(p)))
                                .is_some_and(|wp| page_refs_equal(wp, page))
                        })
                    })
            })
            .cloned()
            .collect()
    }

    /// `getValue(ref)`: one field's value as `setValue()` takes it.
    pub fn get_value(&self, reference: &Value) -> Option<Value> {
        let index = self.view();
        let key = canonical_key(&index, reference);
        field_by_key(&index, &key).and_then(value_of)
    }

    /// `exportValues()`: every value by full name, as plain data, in the
    /// order a plain object holds them (A5: integer-like names first).
    pub fn export_values(&self) -> Value {
        let mut values: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();
        for field in self.view().fields() {
            if let Some(value) = plain_value_of(field) {
                values.insert(crate::value::string_of(get(field, "name")), value);
            }
        }
        crate::value::sort_js_keys(&mut values);
        Value::Object(values.into_iter().collect::<Map<_, _>>())
    }

    /// `placeOf(field)`: its first shown page's index, then top to bottom,
    /// then left to right.
    fn place_of(&self, field: &Value) -> (f64, f64, f64) {
        let index = self.view();
        let row = items(field, "widgets").iter().find_map(|w| {
            let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            widget_row_of(&index, n).cloned()
        });
        let page = row
            .as_ref()
            .and_then(|r| get(r, "page"))
            .and_then(|p| self.get_page(p))
            .and_then(|info| get(&info, "index").and_then(Value::as_f64))
            .unwrap_or(f64::INFINITY);
        let rect = row.as_ref().and_then(|r| get(r, "rect"));
        let y = rect.and_then(|r| get(r, "y")).and_then(Value::as_f64).unwrap_or(0.0);
        let x = rect.and_then(|r| get(r, "x")).and_then(Value::as_f64).unwrap_or(0.0);
        (page, y, x)
    }

    /// `validate()`: the required fields you may fill in that are empty, in
    /// page order.
    pub fn validate(&self) -> Value {
        let index = self.view();
        let mut missing: Vec<(Value, (f64, f64, f64))> = index
            .fields()
            .iter()
            .filter(|f| truthy(get(f, "required")) && is_empty(f) && self.may_fill(f))
            .map(|f| (f.clone(), self.place_of(f)))
            .collect();
        // `Array#sort` is stable; a NaN difference counts as equal.
        missing.sort_by(|(_, a), (_, b)| {
            let difference = |l: f64, r: f64| {
                let d = l - r;
                if d.is_nan() { std::cmp::Ordering::Equal } else { d.partial_cmp(&0.0).unwrap() }
            };
            difference(a.0, b.0).then(difference(a.1, b.1)).then(difference(a.2, b.2))
        });
        let missing: Vec<Value> = missing.into_iter().map(|(f, _)| f).collect();
        json!({ "valid": missing.is_empty(), "missing": missing })
    }

    /// `getSelectedField()`: the field of the selected widget while one
    /// widget is selected (in design mode).
    pub fn get_selected_field(&self) -> Option<Value> {
        let annotation = self.annotation()?;
        let selected = annotation.list_selected();
        let [only] = selected.as_slice() else { return None };
        let subtype = get(only, "subtype").and_then(Value::as_str).unwrap_or("");
        if !subtype.starts_with("widget") {
            return None;
        }
        field_for_widget(&self.view(), widget_object_of(get(only, "ref").unwrap_or(&Value::Null)))
            .cloned()
    }
}
