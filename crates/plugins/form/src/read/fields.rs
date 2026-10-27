//! `read/fields.ts`: reads over the mirrored field tree: fields, values, the
//! joins between a field and its widgets' rows, the field of the selected
//! widget, and the required-field check.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::DocumentMeta;
use serde_json::{Map, Value, json};

use crate::engine::page_refs_equal;
use indexmap::IndexMap;

use crate::model::{
    FieldIndex, canonical_key, field_by_key, shared_field_by_key, shared_field_by_ref,
    shared_field_for_widget, shared_widget_row_of, widget_row_of,
};
use crate::value::{get, items, str_of, truthy};

thread_local! {
    static READ_COMPUTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many answers this thread's memoized reads (`getValue`,
/// `getWidgets`, `exportValues`, `validate`, `getSelectedField`) computed
/// rather than handed back. The guard (`tests/sharing.rs`) holds it still
/// while the form stays the same.
#[doc(hidden)]
pub fn read_computes() -> usize {
    READ_COMPUTES.with(std::cell::Cell::get)
}

fn computed() {
    READ_COMPUTES.with(|c| c.set(c.get() + 1));
}

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
        self.view().whole_snapshot()
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
            .and_then(|s| s.get("formKind").cloned())
            .unwrap_or_else(|| json!("none"))
    }

    /// `refresh(options)`: read the whole form again.
    pub fn refresh(&self, signal: Option<AbortSignal>) -> Promise<()> {
        self.cancellable(signal, self.fields.refresh())
    }

    /// `get(ref)`: one field, or `None`; the field the form holds.
    pub fn get(&self, reference: &Value) -> Option<Rc<Value>> {
        shared_field_by_ref(&self.view(), reference).cloned()
    }

    /// `getFieldForWidget(widget)`.
    pub fn get_field_for_widget(&self, widget: &Value) -> Option<Rc<Value>> {
        shared_field_for_widget(&self.view(), widget_object_of(widget)).cloned()
    }

    /// `getWidget(widget)`: a widget's row, or `None`.
    pub fn get_widget(&self, widget: &Value) -> Option<Rc<Value>> {
        shared_widget_row_of(&self.view(), widget_object_of(widget)).cloned()
    }

    /// `getWidgets(ref)`: every place a field shows, in the field's order.
    /// The same list while the field's rows stay the same (`widgetsByKey`).
    pub fn get_widgets(&self, reference: &Value) -> Rc<Vec<Rc<Value>>> {
        let index = self.view();
        let key = canonical_key(&index, reference);
        let rows: Vec<Option<Rc<Value>>> = shared_field_by_key(&index, &key)
            .map(|field| {
                items(field, "widgets")
                    .iter()
                    .map(|w| {
                        let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
                        shared_widget_row_of(&index, n).cloned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut memo = self.reads.widgets.borrow_mut();
        if let Some((kept, list)) = memo.get(&key)
            && kept.len() == rows.len()
            && kept.iter().zip(&rows).all(|(a, b)| same_row(a, b))
        {
            return list.clone();
        }
        computed();
        let list = Rc::new(rows.iter().flatten().cloned().collect::<Vec<_>>());
        remember(&mut memo, key, (rows, list.clone()));
        list
    }

    /// `list(filter)`: every field, or the ones that match every given part.
    pub fn list(&self, filter: Option<&Value>) -> Vec<Rc<Value>> {
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

    /// `getValue(ref)`: one field's value as `setValue()` takes it. The
    /// same value while the field's value prints the same (`valueByKey`),
    /// so a fill shown before the engine answers and the answer that
    /// brings the same value in a new field read as one.
    pub fn get_value(&self, reference: &Value) -> Option<Rc<Value>> {
        let index = self.view();
        let key = canonical_key(&index, reference);
        // `field ? JSON.stringify(valueOf(field)) : null`
        let printed = field_by_key(&index, &key)
            .map(|field| value_of(field).map_or_else(|| "null".to_string(), |v| v.to_string()));
        let mut memo = self.reads.values.borrow_mut();
        if let Some((kept, value)) = memo.get(&key)
            && *kept == printed
        {
            return value.clone();
        }
        computed();
        let value = printed
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .filter(|value| !value.is_null())
            .map(Rc::new);
        remember(&mut memo, key, (printed, value.clone()));
        value
    }

    /// `exportValues()`: every value by full name, as plain data, in the
    /// order a plain object holds them (A5: integer-like names first). The
    /// same object while the form stays the same (`memo`).
    pub fn export_values(&self) -> Rc<Value> {
        let index = self.view();
        if let Some((kept, values)) = &*self.reads.exported.borrow()
            && Rc::ptr_eq(kept, &index)
        {
            return values.clone();
        }
        computed();
        let mut values: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();
        for field in index.fields() {
            if let Some(value) = plain_value_of(field) {
                values.insert(crate::value::string_of(get(field, "name")), value);
            }
        }
        crate::value::sort_js_keys(&mut values);
        let values = Rc::new(Value::Object(values.into_iter().collect::<Map<_, _>>()));
        *self.reads.exported.borrow_mut() = Some((index, values.clone()));
        values
    }

    /// `placeOf(field)`: its first shown page's index, then top to bottom,
    /// then left to right.
    fn place_of(&self, index: &FieldIndex, field: &Value) -> (f64, f64, f64) {
        let row = items(field, "widgets").iter().find_map(|w| {
            let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            widget_row_of(index, n)
        });
        let page = row
            .and_then(|r| get(r, "page"))
            .and_then(|p| self.get_page(p))
            .and_then(|info| get(&info, "index").and_then(Value::as_f64))
            .unwrap_or(f64::INFINITY);
        let rect = row.and_then(|r| get(r, "rect"));
        let y = rect.and_then(|r| get(r, "y")).and_then(Value::as_f64).unwrap_or(0.0);
        let x = rect.and_then(|r| get(r, "x")).and_then(Value::as_f64).unwrap_or(0.0);
        (page, y, x)
    }

    /// `validate()`: the required fields you may fill in that are empty, in
    /// page order. The same answer while the form, the document and the
    /// session's rights stay the same (`memo`); the document is compared by
    /// value, as Rust holds no identity for it.
    pub fn validate(&self) -> Rc<FormValidation> {
        let index = self.view();
        let document = self.cx.document();
        let rights = self.rights_key();
        if let Some((kept, kept_document, kept_rights, answer)) = &*self.reads.validation.borrow()
            && Rc::ptr_eq(kept, &index)
            && *kept_document == document
            && *kept_rights == rights
        {
            return answer.clone();
        }
        computed();
        let mut missing: Vec<(Rc<Value>, (f64, f64, f64))> = index
            .fields()
            .iter()
            .filter(|f| truthy(get(f, "required")) && is_empty(f) && self.may_fill(f))
            .map(|f| (f.clone(), self.place_of(&index, f)))
            .collect();
        // `Array#sort` is stable; a NaN difference counts as equal.
        missing.sort_by(|(_, a), (_, b)| {
            let difference = |l: f64, r: f64| {
                let d = l - r;
                if d.is_nan() { std::cmp::Ordering::Equal } else { d.partial_cmp(&0.0).unwrap() }
            };
            difference(a.0, b.0).then(difference(a.1, b.1)).then(difference(a.2, b.2))
        });
        let missing: Vec<Rc<Value>> = missing.into_iter().map(|(f, _)| f).collect();
        let answer = Rc::new(FormValidation { valid: missing.is_empty(), missing });
        *self.reads.validation.borrow_mut() = Some((index, document, rights, answer.clone()));
        answer
    }

    /// `getSelectedField()`: the field of the selected widget while one
    /// widget is selected (in design mode). The same answer while the form
    /// and the annotation plugin's selection list stay the same (`memo`).
    pub fn get_selected_field(&self) -> Option<Rc<Value>> {
        let index = self.view();
        let selected = self.annotation().map(|annotation| annotation.list_selected());
        let same_selection = |kept: &Option<Rc<Vec<Value>>>| match (kept, &selected) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if let Some((kept, kept_selection, answer)) = &*self.reads.selected.borrow()
            && Rc::ptr_eq(kept, &index)
            && same_selection(kept_selection)
        {
            return answer.clone();
        }
        computed();
        let answer = match selected.as_deref().map(Vec::as_slice) {
            Some([only])
                if get(only, "subtype")
                    .and_then(Value::as_str)
                    .is_some_and(|subtype| subtype.starts_with("widget")) =>
            {
                let widget = widget_object_of(get(only, "ref").unwrap_or(&Value::Null));
                shared_field_for_widget(&index, widget).cloned()
            }
            _ => None,
        };
        *self.reads.selected.borrow_mut() = Some((index, selected, answer.clone()));
        answer
    }
}

/// `FormValidation`: whether every required field you may fill in holds
/// something, and the ones that don't, in page order, as the form holds
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct FormValidation {
    pub valid: bool,
    pub missing: Vec<Rc<Value>>,
}

impl FormValidation {
    /// As the TypeScript object prints: `{ valid, missing }`.
    pub fn to_json(&self) -> Value {
        json!({
            "valid": self.valid,
            "missing": self.missing.iter().map(|f| (**f).clone()).collect::<Vec<_>>(),
        })
    }
}

/// The TypeScript's identity memos over these reads (`memo`, `memoByKey`
/// in `read/fields.ts`), each holding the `Rc`s it was computed from and
/// comparing them with `Rc::ptr_eq`.
#[derive(Default)]
pub(crate) struct ReadMemos {
    /// `valueByKey`, by field key: the value's printed JSON and the value.
    values: RefCell<IndexMap<String, (Option<String>, Option<Rc<Value>>)>>,
    /// `widgetsByKey`, by field key: the rows and the list made of them.
    widgets: RefCell<IndexMap<String, WidgetsMemo>>,
    exported: RefCell<Option<(Rc<FieldIndex>, Rc<Value>)>>,
    validation: RefCell<Option<ValidationMemo>>,
    selected: RefCell<Option<SelectedMemo>>,
}

type WidgetsMemo = (Vec<Option<Rc<Value>>>, Rc<Vec<Rc<Value>>>);
type ValidationMemo = (Rc<FieldIndex>, Option<DocumentMeta>, String, Rc<FormValidation>);
type SelectedMemo = (Rc<FieldIndex>, Option<Rc<Vec<Value>>>, Option<Rc<Value>>);

/// `memoByKey`'s `{ maxEntries: 1024 }`.
const MAX_ENTRIES: usize = 1024;

/// `memoByKey`'s store: a key computed again goes last; a new key past the
/// bound evicts the oldest.
fn remember<V>(cache: &mut IndexMap<String, V>, key: String, entry: V) {
    if cache.shift_remove(&key).is_none() && cache.len() >= MAX_ENTRIES {
        cache.shift_remove_index(0);
    }
    cache.insert(key, entry);
}

/// `===` over a widget row or its absence.
fn same_row(a: &Option<Rc<Value>>, b: &Option<Rc<Value>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
