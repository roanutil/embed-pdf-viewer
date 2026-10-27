//! `read/fill-items.ts`: the widget projection, what a framework draws for
//! one page's fields. Pure data in page space. Where a widget is and how it
//! looks come from the form's widget rows (`/Rect` and `/MK`), identity,
//! value and behavior from its field. Items are JSON in the TypeScript's
//! member order.

use serde_json::{Map, Value, json};

use crate::model::{FieldIndex, PageWidget, PageWidgets, field_key_of};
use crate::value::{get, items, number, str_of, truthy};

fn widget_of(field: &Value, annot_object_number: f64) -> Option<&Value> {
    items(field, "widgets")
        .iter()
        .find(|w| get(w, "objectNumber").and_then(Value::as_f64) == Some(annot_object_number))
}

/// `projectWidget(field, annotObjectNumber, widget, fillable)`.
pub fn project_widget(
    field: &Value,
    annot_object_number: f64,
    widget: &PageWidget,
    fillable: bool,
) -> Option<Value> {
    let mut item = Map::new();
    let mut put = |key: &str, value: Value| {
        item.insert(key.to_string(), value);
    };
    put("key", json!(field_key_of(field)));
    put("fieldRef", get(field, "ref").cloned().unwrap_or(Value::Null));
    put("annotObjectNumber", number(annot_object_number));
    put(
        "annotationRef",
        widget_of(field, annot_object_number)
            .and_then(|w| get(w, "ref").filter(|r| !r.is_null()).cloned())
            .unwrap_or(Value::Null),
    );
    put("box", widget.bx.clone());
    put("look", widget.look.clone());
    put("disabled", json!(truthy(get(field, "readOnly")) || !fillable));
    put(
        "label",
        get(field, "alternateName")
            .filter(|n| !n.is_null())
            .or_else(|| get(field, "name"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    let member = |key: &str| get(field, key).cloned().unwrap_or(Value::Null);
    match str_of(field, "family") {
        Some("text") => {
            put("control", json!("text"));
            put("value", member("value"));
            put("multiline", member("multiline"));
            put("password", member("password"));
            put("maxLength", member("maxLength"));
            put("comb", member("comb"));
        }
        Some("checkbox") => {
            let toggle = widget_of(field, annot_object_number);
            put("control", json!("toggle"));
            put("kind", json!("checkbox"));
            put("checked", member("checked"));
            put(
                "exportValue",
                match toggle {
                    Some(t) if t.get("exportValue").is_some() => member_of(t, "exportValue"),
                    _ => member("exportValue"),
                },
            );
        }
        Some("radio") => {
            let toggle = widget_of(field, annot_object_number);
            put("control", json!("toggle"));
            put("kind", json!("radio"));
            put(
                "checked",
                match toggle {
                    Some(t) if t.get("checked").is_some() => member_of(t, "checked"),
                    _ => json!(false),
                },
            );
            put(
                "exportValue",
                match toggle {
                    Some(t) if t.get("exportValue").is_some() => member_of(t, "exportValue"),
                    _ => json!(""),
                },
            );
        }
        Some("combobox") => {
            let value = member("value");
            put("control", json!("choice"));
            put("kind", json!("combo"));
            put("edit", member("edit"));
            put("multi", json!(false));
            put("options", member("options"));
            put("selected", if value == json!("") { json!([]) } else { json!([value]) });
        }
        Some("listbox") => {
            put("control", json!("choice"));
            put("kind", json!("list"));
            put("edit", json!(false));
            put("multi", member("multiSelect"));
            put("options", member("options"));
            put("selected", member("selectedValues"));
        }
        Some("pushbutton") => put("control", json!("button")),
        Some("signature") => {
            put("control", json!("signature"));
            let kind = get(field, "valueEntry").and_then(|e| str_of(e, "kind"));
            put("signed", json!(kind != Some("none")));
        }
        _ => return None,
    }
    Some(Value::Object(item))
}

fn member_of(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(Value::Null)
}

/// `fillItems(index, pageObjectNumber, widgets, mayFill)`.
pub fn fill_items(
    index: &FieldIndex,
    page: f64,
    widgets: Option<&PageWidgets>,
    may_fill: &dyn Fn(&Value) -> bool,
) -> Vec<Value> {
    let (Some(_), Some(widgets)) = (&index.snapshot, widgets) else { return Vec::new() };
    let mut out = Vec::new();
    for field in index.fields() {
        for widget in items(field, "widgets") {
            let on_page =
                get(widget, "page").and_then(|p| get(p, "objectNumber")).and_then(Value::as_f64)
                    == Some(page);
            if !on_page {
                continue;
            }
            let n = get(widget, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let Some(placed) = widgets.get(&crate::value::number_key(n)) else { continue };
            if let Some(item) = project_widget(field, n, placed, may_fill(field)) {
                out.push(item);
            }
        }
    }
    out
}
