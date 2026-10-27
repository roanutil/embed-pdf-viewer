//! The pure parts of `@embedpdf/engine-core/runtime` the form plugin calls:
//! the value checks a write is made of (`forms/write.ts`), what a form op
//! leaves (`forms/predict.ts`, `forms/widgets.ts`), a field's value as a
//! write takes it (`forms/value.ts`), script effects as ops
//! (`forms/effects.ts`), whether actions take `doc.forms.script`
//! (`dto/PdfAction.ts`, `forms/draft.ts`), refs and their keys
//! (`identity/*.ts`) and list positions (`mutation/ListPosition.ts`).
//!
//! `epdf-engine` carries the same rules, but it links PDFium and this crate
//! builds for wasm32 (`epdf-web`), so they are transcribed here over the
//! engine's JSON, as `epdf-plugin-annotation` does for its own (Ruling13.15).
//! `tests/engine_cross_check.rs` holds the two copies together on native
//! hosts.

use std::collections::{HashMap, HashSet};

use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::value::{get, items, number_key, str_of, string_of, truthy};

// ── refs and keys ───────────────────────────────────────────────────────────

/// `fieldKeyOfRef(ref)`: `obj:<n>` for a durable ref, else `fqn:<name>`.
pub fn field_key_of_ref(reference: &Value) -> String {
    if str_of(reference, "kind") == Some("objectNumber") {
        format!("obj:{}", string_of(get(reference, "objectNumber")))
    } else {
        format!("fqn:{}", string_of(get(reference, "name")))
    }
}

/// `annotationKey(ref)`: `obj:<n>`, `base:<page>:<i>`, or `undefined` for
/// a ref of another kind (the `switch` falls through).
pub fn annotation_key(reference: &Value) -> String {
    match str_of(reference, "kind") {
        Some("objectNumber") => format!("obj:{}", string_of(get(reference, "objectNumber"))),
        Some("baseIndex") => format!(
            "base:{}:{}",
            string_of(get(reference, "page").and_then(|p| get(p, "objectNumber"))),
            string_of(get(reference, "baseIndex"))
        ),
        _ => "undefined".into(),
    }
}

/// `toFieldRef(name)`.
pub fn to_field_ref(name: &str) -> Value {
    json!({ "kind": "fqn", "name": name })
}

/// `encodeFieldRefKey(ref)`: a durable ref's number must be a positive
/// integer (`RangeError` otherwise).
pub fn encode_field_ref_key(reference: &Value) -> Result<String, Error> {
    if str_of(reference, "kind") == Some("objectNumber") {
        let n = get(reference, "objectNumber");
        let integer = n.and_then(Value::as_f64).filter(|n| n.fract() == 0.0 && *n > 0.0);
        if integer.is_none() {
            return Err(Error::named(
                "RangeError",
                format!(
                    "encodeFieldRefKey: objectNumber must be a positive integer, got {}",
                    string_of(n)
                ),
            ));
        }
        return Ok(format!("obj:{}", string_of(n)));
    }
    Ok(format!("fqn:{}", string_of(get(reference, "name"))))
}

/// `formWidget(objectNumber, page)`: the widget record, its `ref` present
/// when it is an object on a page.
pub fn form_widget(object_number: f64, page: Option<&Value>) -> Value {
    let reference = match page {
        Some(page) if object_number > 0.0 => {
            json!({ "kind": "objectNumber", "page": page, "objectNumber": object_number })
        }
        _ => Value::Null,
    };
    json!({ "ref": reference, "objectNumber": object_number, "page": page.cloned().unwrap_or(Value::Null) })
}

/// `pageRefsEqual(a, b)`.
pub fn page_refs_equal(a: &Value, b: &Value) -> bool {
    crate::value::strict_eq(get(a, "kind"), get(b, "kind"))
        && crate::value::strict_eq(get(a, "objectNumber"), get(b, "objectNumber"))
}

/// A widget row's annotation object number: its ref's, `0` for a ref of
/// another kind.
pub fn row_number(row: &Value) -> f64 {
    let reference = get(row, "ref").unwrap_or(&Value::Null);
    if str_of(reference, "kind") == Some("objectNumber") {
        get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
    } else {
        0.0
    }
}

fn engine_error(code: &str, message: String, field: &str) -> Error {
    Error::engine(code, message).with("details", json!({ "field": field }))
}

// ── list positions ──────────────────────────────────────────────────────────

/// `anchorOf(position)`: the neighbour a position names.
pub fn anchor_of(position: &Value) -> Option<&Value> {
    match position {
        Value::Object(o) => o.get("before").or_else(|| o.get("after")),
        _ => None,
    }
}

/// `reorderedList(order, moved, position, keyOf)`: the order once `moved`
/// go to `position`, together and in the order given.
pub fn reordered_list(
    order: &[Value],
    moved: &[Value],
    position: &Value,
    key_of: &dyn Fn(&Value) -> String,
) -> Result<Vec<Value>, Error> {
    let moved_keys: HashSet<String> = moved.iter().map(key_of).collect();
    if moved_keys.len() != moved.len() {
        return Err(engine_error("InvalidArg", "a row is named twice".into(), "refs"));
    }
    let present: HashSet<String> = order.iter().map(key_of).collect();
    if let Some(missing) = moved.iter().find(|r| !present.contains(&key_of(r))) {
        return Err(engine_error(
            "NotFound",
            format!("no row {} in the list", key_of(missing)),
            "refs",
        ));
    }
    let anchor = anchor_of(position);
    if let Some(anchor) = anchor.filter(|a| !a.is_null())
        && moved_keys.contains(&key_of(anchor))
    {
        return Err(engine_error(
            "InvalidArg",
            format!("the neighbour {} is one of the rows that move", key_of(anchor)),
            "position",
        ));
    }
    let staying: Vec<Value> =
        order.iter().filter(|r| !moved_keys.contains(&key_of(r))).cloned().collect();
    let at = match position.as_str() {
        Some("start") => 0,
        Some("end") => staying.len(),
        _ => {
            let anchor = anchor.cloned().unwrap_or(Value::Null);
            let wanted = key_of(&anchor);
            let Some(at) = staying.iter().position(|r| key_of(r) == wanted) else {
                return Err(engine_error(
                    "NotFound",
                    format!("no neighbour {wanted} in the list"),
                    "position",
                ));
            };
            if position.get("before").is_some() { at } else { at + 1 }
        }
    };
    let mut out = staying[..at].to_vec();
    out.extend(moved.iter().cloned());
    out.extend(staying[at..].iter().cloned());
    Ok(out)
}

/// `reorderPart(list, inPart, order, keyOf)`: the items `in_part` picks
/// take `order`'s order in their slots; the rest keep theirs.
pub fn reorder_part<T: Clone>(
    list: &[T],
    in_part: &dyn Fn(&T) -> bool,
    order: &[String],
    key_of: &dyn Fn(&T) -> String,
) -> Vec<T> {
    let part: Vec<usize> = (0..list.len()).filter(|&i| in_part(&list[i])).collect();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for &i in &part {
        by_key.insert(key_of(&list[i]), i);
    }
    let named: Vec<usize> = order.iter().filter_map(|key| by_key.get(key).copied()).collect();
    let placed: HashSet<usize> = named.iter().copied().collect();
    let mut next = named;
    next.extend(part.iter().copied().filter(|i| !placed.contains(i)));
    let mut slot = 0;
    list.iter()
        .map(|item| {
            if in_part(item) {
                let out = list[next[slot]].clone();
                slot += 1;
                out
            } else {
                item.clone()
            }
        })
        .collect()
}

// ── scripts and effects ─────────────────────────────────────────────────────

fn needs_script_right(action: &Value) -> bool {
    matches!(str_of(action, "type"), Some("javascript" | "submit-form" | "uri"))
        || items(action, "next").iter().any(needs_script_right)
}

/// `writesScripts(actions)`: an event gets a tree holding a JavaScript,
/// submit-form or URI action. A read tree (`root`) sent back never does.
pub fn writes_scripts(actions: Option<&Value>) -> bool {
    let Some(Value::Object(actions)) = actions else { return false };
    actions.values().any(|action| {
        !action.is_null()
            && !epdf_kernel::is_undefined(action)
            && action.get("root").is_none()
            && needs_script_right(action)
    })
}

/// `draftWritesScripts(draft)`.
pub fn draft_writes_scripts(draft: &Value) -> bool {
    writes_scripts(get(draft, "actions"))
        || items(draft, "widgets").iter().any(|placement| writes_scripts(get(placement, "actions")))
}

/// `formEffectOps(effects)`: a script run's effects as the ops of one change.
pub fn form_effect_ops(effects: &[Value]) -> Vec<Value> {
    effects
        .iter()
        .map(|effect| {
            let reference = get(effect, "ref").cloned().unwrap_or(Value::Null);
            match str_of(effect, "kind") {
                Some("setValue") => json!({
                    "type": "forms.setValue", "field": reference,
                    "value": get(effect, "value").cloned().unwrap_or(Value::Null),
                }),
                Some("setDisplay") => json!({
                    "type": "forms.setDisplay", "field": reference,
                    "display": get(effect, "display").cloned().unwrap_or(Value::Null),
                }),
                Some("setAppearanceText") => json!({
                    "type": "forms.setAppearanceText", "field": reference,
                    "text": get(effect, "text").cloned().unwrap_or(Value::Null),
                }),
                _ => json!({
                    "type": "forms.reset",
                    "fields": get(effect, "refs").cloned().unwrap_or(Value::Null),
                }),
            }
        })
        .collect()
}

/// `serializeError(err).message` for an error a sink caught.
pub fn serialized_message(error: &Error) -> String {
    error.message.clone()
}

// ── values ──────────────────────────────────────────────────────────────────

/// `fieldValueOf(field)`: the value a field holds, as a write takes it;
/// none for a family whose value a write can't set.
pub fn field_value_of(field: &Value) -> Option<Value> {
    match str_of(field, "family") {
        Some("checkbox") => Some(json!({ "checked": member(field, "checked") })),
        Some("listbox") => Some(json!({ "selectedValues": member(field, "selectedValues") })),
        Some("radio") => {
            let value = member(field, "value");
            Some(json!({ "value": if value == json!("Off") { Value::Null } else { value } }))
        }
        Some("text" | "combobox") => Some(json!({ "value": member(field, "value") })),
        _ => None,
    }
}

fn member(v: &Value, key: &str) -> Value {
    crate::value::member(v, key)
}

/// `FieldValueWrite`.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValueWrite {
    Text(String),
    Toggle(Option<String>),
    Choice(Vec<String>),
}

fn invalid_value(message: String) -> Error {
    engine_error("InvalidArg", message, "value")
}

fn shape_of(value: &Value) -> &'static str {
    if value.get("checked").is_some() {
        "{ checked }"
    } else if value.get("selectedValues").is_some() {
        "{ selectedValues }"
    } else {
        "{ value }"
    }
}

/// `'value' in value` and friends, as the TypeScript tests a write's shape.
fn has(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|o| o.contains_key(key))
}

fn string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) if epdf_kernel::is_undefined(v) => String::new(),
        Some(v) => string_of(Some(v)),
    }
}

fn toggle_state_of(field: &Value, export_value: Option<&Value>) -> Result<Option<String>, Error> {
    let export_value = export_value.filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v));
    let Some(export_value) = export_value else { return Ok(None) };
    if export_value == &json!("Off") {
        return Ok(None);
    }
    let widget = items(field, "widgets")
        .iter()
        .find(|w| crate::value::strict_eq(get(w, "exportValue"), Some(export_value)));
    match widget {
        Some(widget) => Ok(get(widget, "onState").map(|s| string_of(Some(s)))),
        None => Err(invalid_value(format!(
            "no widget of '{}' has the export value '{}'",
            string_of(get(field, "name")),
            string_of(Some(export_value))
        ))),
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| string_of(Some(v))).collect())
        .unwrap_or_default()
}

fn option_values(field: &Value) -> Vec<String> {
    items(field, "options").iter().map(|o| string_of(get(o, "value"))).collect()
}

fn choice_of(field: &Value, values: Vec<String>) -> Result<FieldValueWrite, Error> {
    let distinct: HashSet<&String> = values.iter().collect();
    if distinct.len() != values.len() {
        return Err(invalid_value("the values must not repeat".into()));
    }
    let family = str_of(field, "family");
    if family == Some("listbox") && !truthy(get(field, "multiSelect")) && values.len() > 1 {
        return Err(invalid_value(format!(
            "'{}' takes one value (it has no multiSelect)",
            string_of(get(field, "name"))
        )));
    }
    let options: HashSet<String> = option_values(field).into_iter().collect();
    let free_text = family == Some("combobox") && truthy(get(field, "edit")) && values.len() == 1;
    let unknown = values.iter().find(|v| !options.contains(*v));
    if !free_text && let Some(unknown) = unknown {
        return Err(invalid_value(format!(
            "'{unknown}' is not an option value of '{}'",
            string_of(get(field, "name"))
        )));
    }
    Ok(FieldValueWrite::Choice(values))
}

/// `fieldValueWriteOf(field, value)`: the write `value` makes, checked
/// against the field first (`InvalidArg` otherwise).
pub fn field_value_write_of(field: &Value, value: &Value) -> Result<FieldValueWrite, Error> {
    let family = str_of(field, "family").map(str::to_string);
    match family.as_deref() {
        Some("text") => {
            if has(value, "value") {
                return Ok(FieldValueWrite::Text(string_or_empty(get(value, "value"))));
            }
        }
        Some("checkbox") => {
            if has(value, "checked") {
                if !truthy(get(value, "checked")) {
                    return Ok(FieldValueWrite::Toggle(None));
                }
                let Some(first) = items(field, "widgets").first() else {
                    return Err(invalid_value(format!(
                        "'{}' has no widget to check",
                        string_of(get(field, "name"))
                    )));
                };
                return Ok(FieldValueWrite::Toggle(
                    get(first, "onState").map(|s| string_of(Some(s))),
                ));
            }
            if has(value, "value") {
                return Ok(FieldValueWrite::Toggle(toggle_state_of(field, get(value, "value"))?));
            }
        }
        Some("radio") => {
            if has(value, "value") {
                let state = toggle_state_of(field, get(value, "value"))?;
                if state.is_none() && truthy(get(field, "noToggleToOff")) {
                    return Err(invalid_value(format!(
                        "'{}' must keep a choice: it can't be cleared",
                        string_of(get(field, "name"))
                    )));
                }
                return Ok(FieldValueWrite::Toggle(state));
            }
        }
        Some("combobox") => {
            if has(value, "value") {
                let v = get(value, "value");
                let values = match v {
                    None | Some(Value::Null) => Vec::new(),
                    Some(v) => vec![string_of(Some(v))],
                };
                return choice_of(field, values);
            }
        }
        Some("listbox") => {
            if has(value, "selectedValues") {
                return choice_of(field, strings(get(value, "selectedValues")));
            }
        }
        _ => {
            return Err(invalid_value(format!(
                "{} fields hold no value you can set",
                string_of(get(field, "family"))
            )));
        }
    }
    let takes = match family.as_deref() {
        Some("text") => "{ value }",
        Some("checkbox") => "{ checked } or { value }",
        Some("radio") => "{ value }",
        Some("combobox") => "{ value }",
        _ => "{ selectedValues }",
    };
    Err(invalid_value(format!(
        "a {} field takes {takes}, not {}",
        string_of(get(field, "family")),
        shape_of(value)
    )))
}

/// `assertChoiceDefault(target, values)`.
pub fn assert_choice_default(
    family: &str,
    options: &[String],
    edit: bool,
    multi_select: bool,
    values: &[String],
) -> Result<(), Error> {
    let invalid = |message: String| engine_error("InvalidArg", message, "defaultValue");
    let distinct: HashSet<&String> = values.iter().collect();
    if distinct.len() != values.len() {
        return Err(invalid("the default values must not repeat".into()));
    }
    if values.len() > 1 && !(family == "listbox" && multi_select) {
        return Err(invalid(format!(
            "a {} has one default",
            if family == "listbox" { "list without multiSelect" } else { "dropdown" }
        )));
    }
    let free_text = family == "combobox" && edit;
    let option_values: HashSet<&String> = options.iter().collect();
    if let Some(unknown) = values.iter().find(|v| !option_values.contains(v))
        && !free_text
    {
        return Err(invalid(format!(
            "'{unknown}' is not an option value of the {}",
            if family == "listbox" { "list" } else { "dropdown" }
        )));
    }
    Ok(())
}

// ── what an op leaves (`forms/predict.ts`) ──────────────────────────────────

/// `FieldWithWidgets`: a field and the rows of its widgets.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldWithWidgets {
    pub field: Value,
    pub widgets: Vec<Value>,
}

fn scalar(value: &str) -> Value {
    json!({ "kind": "scalar", "value": value })
}

fn none() -> Value {
    json!({ "kind": "none" })
}

/// `fieldAfterValue(current, value)`.
pub fn field_after_value(
    current: &FieldWithWidgets,
    value: &Value,
) -> Result<FieldWithWidgets, Error> {
    let field = &current.field;
    let family = str_of(field, "family");
    match field_value_write_of(field, value)? {
        FieldValueWrite::Text(text) => {
            if family != Some("text") {
                return Ok(current.clone());
            }
            let text = first_characters(&text, get(field, "maxLength"));
            if get(field, "value").and_then(Value::as_str) == Some(text.as_str()) {
                return Ok(current.clone());
            }
            let next = crate::value::spread(
                field,
                [("value", json!(text)), ("valueEntry", scalar(&text))],
            );
            Ok(redrawn(next, current))
        }
        FieldValueWrite::Toggle(state) => {
            if family != Some("checkbox") && family != Some("radio") {
                return Ok(current.clone());
            }
            let target = match &state {
                None => -1,
                Some(state) => items(field, "widgets")
                    .iter()
                    .position(|w| get(w, "onState").and_then(Value::as_str) == Some(state.as_str()))
                    .map_or(-1, |i| i as i64),
            };
            Ok(toggled(current, target))
        }
        FieldValueWrite::Choice(values) => {
            if family != Some("combobox") && family != Some("listbox") {
                return Ok(current.clone());
            }
            Ok(redrawn(choice_field_with(field, &values), current))
        }
    }
}

fn not_reset(field: &Value) -> Error {
    Error::engine("Unknown", format!("'{}' could not be reset", string_of(get(field, "name"))))
}

/// `fieldAfterReset(current)`.
pub fn field_after_reset(current: &FieldWithWidgets) -> Result<FieldWithWidgets, Error> {
    let field = &current.field;
    let defaults = get(field, "defaultValueEntry").cloned().unwrap_or(Value::Null);
    let kind = str_of(&defaults, "kind").map(str::to_string);
    match str_of(field, "family") {
        Some("text") => {
            if matches!(kind.as_deref(), Some("array" | "unsupported")) {
                return Err(not_reset(field));
            }
            let scalar_default = kind.as_deref() == Some("scalar");
            let value = if scalar_default { member(&defaults, "value") } else { json!("") };
            let entry = if scalar_default { defaults.clone() } else { none() };
            let next = crate::value::spread(field, [("value", value), ("valueEntry", entry)]);
            Ok(redrawn(next, current))
        }
        Some("checkbox" | "radio") => {
            if matches!(kind.as_deref(), Some("array" | "unsupported")) {
                return Err(not_reset(field));
            }
            let state = if kind.as_deref() == Some("scalar") {
                Some(string_of(get(&defaults, "value")))
            } else {
                None
            };
            match state {
                None => Ok(toggled(current, -1)),
                Some(state) if state == "Off" => Ok(toggled(current, -1)),
                Some(state) => {
                    let target = widget_named_by(items(field, "widgets"), &state);
                    if target < 0 {
                        return Err(not_reset(field));
                    }
                    Ok(toggled(current, target))
                }
            }
        }
        Some("combobox" | "listbox") => {
            if kind.as_deref() == Some("unsupported") {
                return Err(not_reset(field));
            }
            let mut values = match kind.as_deref() {
                Some("scalar") => vec![string_of(get(&defaults, "value"))],
                Some("array") => strings(get(&defaults, "values")),
                _ => Vec::new(),
            };
            if values.len() == 1 && values[0].is_empty() && !choice_matches(field, &values) {
                values = Vec::new();
            }
            if !choice_matches(field, &values) && !free_text_allowed(field, &values) {
                return Err(not_reset(field));
            }
            Ok(redrawn(choice_field_with(field, &values), current))
        }
        _ => Err(not_reset(field)),
    }
}

const DISPLAY_FLAGS: [(&str, &[(&str, bool)]); 4] = [
    ("visible", &[("invisible", false), ("hidden", false), ("noView", false), ("print", true)]),
    ("hidden", &[("invisible", false), ("hidden", true), ("noView", false), ("print", true)]),
    ("noPrint", &[("invisible", false), ("hidden", false), ("noView", false), ("print", false)]),
    ("noView", &[("hidden", false), ("noView", true), ("print", true)]),
];

/// `widgetsAfterDisplay(widgets, display)`: each row's flags as Acrobat's
/// `Field.display` sets them. A display the table doesn't name reads its
/// flags as `undefined` and throws, as `Object.keys(undefined)` does.
pub fn widgets_after_display(rows: &[Value], display: &Value) -> Result<Vec<Value>, Error> {
    let name = display.as_str().unwrap_or("");
    let Some((_, flags)) = DISPLAY_FLAGS.iter().find(|(n, _)| *n == name) else {
        return Err(Error::named("TypeError", "Cannot convert undefined or null to object"));
    };
    Ok(rows
        .iter()
        .map(|row| {
            let same = flags.iter().all(|(key, flag)| get(row, key) == Some(&json!(flag)));
            if same {
                row.clone()
            } else {
                crate::value::spread(row, flags.iter().map(|(k, f)| (*k, json!(f))))
            }
        })
        .collect())
}

/// `firstCharacters(text, limit)`: the first `limit` characters (code
/// points); all without a limit.
fn first_characters(text: &str, limit: Option<&Value>) -> String {
    let limit = limit.and_then(Value::as_f64).filter(|l| *l != 0.0 && !l.is_nan());
    let Some(limit) = limit else { return text.to_string() };
    let count = text.chars().count() as f64;
    if count > limit {
        // `slice(0, limit)` truncates a fractional end; a negative one
        // counts from the end.
        let end = if limit < 0.0 { (count + limit.trunc()).max(0.0) } else { limit.trunc() };
        text.chars().take(end as usize).collect()
    } else {
        text.to_string()
    }
}

/// `redrawn(field, current)`: every widget's appearance drawn again.
fn redrawn(field: Value, current: &FieldWithWidgets) -> FieldWithWidgets {
    FieldWithWidgets {
        field,
        widgets: current
            .widgets
            .iter()
            .map(|row| {
                if truthy(get(row, "hasAppearance")) {
                    row.clone()
                } else {
                    crate::value::spread(row, [("hasAppearance", json!(true))])
                }
            })
            .collect(),
    }
}

/// `widgetNamedBy(widgets, state)`.
fn widget_named_by(widgets: &[Value], state: &str) -> i64 {
    let named = widgets
        .iter()
        .position(|w| get(w, "onState").and_then(Value::as_str) == Some(state))
        .map_or(-1, |i| i as i64);
    let digits = !state.is_empty() && state.bytes().all(|b| b.is_ascii_digit());
    if named >= 0 || !digits {
        return named;
    }
    let at = epdf_kernel::mirror::string_to_number(state);
    if at < widgets.len() as f64 { at as i64 } else { -1 }
}

/// `toggled(current, target)`.
fn toggled(current: &FieldWithWidgets, target: i64) -> FieldWithWidgets {
    let field = &current.field;
    let family = str_of(field, "family");
    if family != Some("checkbox") && family != Some("radio") {
        return current.clone();
    }
    let unison = family == Some("checkbox") || truthy(get(field, "radiosInUnison"));
    let widgets = items(field, "widgets");
    let on = if target >= 0 { widgets.get(target as usize) } else { None };
    let toggles: Vec<Value> = widgets
        .iter()
        .enumerate()
        .map(|(at, widget)| {
            let checked = on.is_some_and(|on| {
                if unison {
                    crate::value::strict_eq(get(widget, "exportValue"), get(on, "exportValue"))
                        && crate::value::strict_eq(get(widget, "onState"), get(on, "onState"))
                } else {
                    at as i64 == target
                }
            });
            if get(widget, "checked") == Some(&json!(checked)) {
                widget.clone()
            } else {
                crate::value::spread(widget, [("checked", json!(checked))])
            }
        })
        .collect();
    let value_entry = match on {
        Some(on) => scalar(&string_of(get(on, "onState"))),
        None => scalar("Off"),
    };
    let next = if family == Some("checkbox") {
        let checked = toggles.iter().any(|w| truthy(get(w, "checked")));
        crate::value::spread(
            field,
            [
                ("widgets", Value::Array(toggles.clone())),
                ("valueEntry", value_entry),
                ("checked", json!(checked)),
            ],
        )
    } else {
        let value = toggles
            .iter()
            .find(|w| truthy(get(w, "checked")))
            .and_then(|w| get(w, "exportValue").cloned())
            .unwrap_or_else(|| json!("Off"));
        crate::value::spread(
            field,
            [
                ("widgets", Value::Array(toggles.clone())),
                ("valueEntry", value_entry),
                ("value", value),
            ],
        )
    };
    let states: HashMap<String, Value> = toggles
        .iter()
        .map(|w| {
            let state = if truthy(get(w, "checked")) {
                get(w, "onState").cloned().unwrap_or(Value::Null)
            } else {
                json!("Off")
            };
            (number_key(get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)), state)
        })
        .collect();
    let rows = current
        .widgets
        .iter()
        .map(|row| match states.get(&number_key(row_number(row))) {
            None => row.clone(),
            Some(state) if get(row, "appearanceState") == Some(state) => row.clone(),
            Some(state) => crate::value::spread(row, [("appearanceState", state.clone())]),
        })
        .collect();
    FieldWithWidgets { field: next, widgets: rows }
}

/// `matchedIndexes(options, values)`.
fn matched_indexes(options: &[String], values: &[String]) -> Vec<usize> {
    let mut indexes: Vec<usize> = Vec::new();
    for value in values {
        if let Some(at) = options.iter().position(|o| o == value)
            && !indexes.contains(&at)
        {
            indexes.push(at);
        }
    }
    indexes.sort_unstable();
    indexes
}

fn choice_matches(field: &Value, values: &[String]) -> bool {
    let options = option_values(field);
    values.iter().all(|v| options.contains(v))
}

fn free_text_allowed(field: &Value, values: &[String]) -> bool {
    str_of(field, "family") == Some("combobox") && truthy(get(field, "edit")) && values.len() == 1
}

/// `choiceFieldWith(field, values)`.
fn choice_field_with(field: &Value, values: &[String]) -> Value {
    let options = option_values(field);
    let matched =
        if choice_matches(field, values) { matched_indexes(&options, values) } else { Vec::new() };
    let free_text = !values.is_empty() && matched.is_empty();
    let next_options: Vec<Value> = items(field, "options")
        .iter()
        .enumerate()
        .map(|(at, option)| {
            let selected = matched.contains(&at);
            if get(option, "selected") == Some(&json!(selected)) {
                option.clone()
            } else {
                crate::value::spread(option, [("selected", json!(selected))])
            }
        })
        .collect();
    let chosen: Vec<String> = if free_text {
        values.to_vec()
    } else {
        matched.iter().map(|&at| options[at].clone()).collect()
    };
    let value_entry = choice_entry_of(&chosen);
    if str_of(field, "family") == Some("combobox") {
        crate::value::spread(
            field,
            [
                ("options", Value::Array(next_options)),
                ("valueEntry", value_entry),
                ("value", json!(chosen.first().cloned().unwrap_or_default())),
            ],
        )
    } else {
        crate::value::spread(
            field,
            [
                ("options", Value::Array(next_options)),
                ("valueEntry", value_entry),
                ("selectedValues", json!(chosen)),
            ],
        )
    }
}

/// `choiceEntryOf(values)`.
fn choice_entry_of(values: &[String]) -> Value {
    match values {
        [] => none(),
        [one] => scalar(one),
        many => json!({ "kind": "array", "values": many }),
    }
}

/// `inOptionOrder(options, values)`.
fn in_option_order(options: &[String], values: &[String]) -> Vec<String> {
    matched_indexes(options, values).into_iter().map(|at| options[at].clone()).collect()
}

/// `topRowOf(topIndex, options)`.
pub fn top_row_of(top_index: f64, options: usize) -> f64 {
    epdf_geometry::js::max(0.0, epdf_geometry::js::min(top_index, options as f64 - 1.0))
}

fn kept_values(values: &[String], exports: &[String], keep_free_text: bool) -> Vec<String> {
    let kept: Vec<String> = values.iter().filter(|v| exports.contains(v)).cloned().collect();
    if kept.is_empty() && keep_free_text && !values.is_empty() { values.to_vec() } else { kept }
}

fn values_of_entry(entry: Option<&Value>) -> Vec<String> {
    let Some(entry) = entry else { return Vec::new() };
    match str_of(entry, "kind") {
        Some("scalar") => vec![string_of(get(entry, "value"))],
        Some("array") => strings(get(entry, "values")),
        _ => Vec::new(),
    }
}

/// `withDefault(field, values, filtered)`.
fn with_default(field: &Value, values: &[String], filtered: bool) -> Value {
    let combobox = str_of(field, "family") == Some("combobox");
    if values.is_empty() {
        let entry = if !filtered {
            none()
        } else if !combobox && truthy(get(field, "multiSelect")) {
            json!({ "kind": "array", "values": [] })
        } else {
            scalar("")
        };
        let default_value = if combobox { json!("") } else { json!([]) };
        return crate::value::spread(
            field,
            [("defaultValueEntry", entry), ("defaultValue", default_value)],
        );
    }
    let options = option_values(field);
    let matched = choice_matches(field, values);
    let ordered = if matched { in_option_order(&options, values) } else { values.to_vec() };
    let entry = if matched { choice_entry_of(&ordered) } else { scalar(&values[0]) };
    let default_value =
        if combobox { json!(ordered.first().cloned().unwrap_or_default()) } else { json!(ordered) };
    crate::value::spread(field, [("defaultValueEntry", entry), ("defaultValue", default_value)])
}

/// `withOptions(field, options)`.
fn with_options(field: &Value, options: Vec<Value>) -> Value {
    let exports: Vec<String> = options.iter().map(|o| string_of(get(o, "value"))).collect();
    let combobox = str_of(field, "family") == Some("combobox");
    let keeps_free_text = combobox && truthy(get(field, "edit"));
    let current = if combobox {
        let value = string_of(get(field, "value"));
        if value.is_empty() { Vec::new() } else { vec![value] }
    } else {
        strings(get(field, "selectedValues"))
    };
    let kept = kept_values(&current, &exports, keeps_free_text);
    let count = options.len();
    let listed = if combobox {
        crate::value::spread(field, [("options", Value::Array(options))])
    } else {
        let top =
            top_row_of(get(field, "topIndex").and_then(Value::as_f64).unwrap_or(f64::NAN), count);
        crate::value::spread(
            field,
            [("options", Value::Array(options)), ("topIndex", crate::value::number(top))],
        )
    };
    let reselected = choice_field_with(&listed, &kept);
    let default_entry = get(field, "defaultValueEntry");
    if default_entry.and_then(|e| str_of(e, "kind")) == Some("none") {
        return reselected;
    }
    let defaults = values_of_entry(default_entry);
    let kept_defaults = kept_values(&defaults, &exports, keeps_free_text);
    with_default(&reselected, &kept_defaults, true)
}

fn option_of(input: &Value) -> Value {
    json!({
        "label": crate::value::member(input, "label"),
        "value": crate::value::member(input, "value"),
        "selected": false,
    })
}

const FLAGS: [&str; 10] = [
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
];

/// `formFieldAfter(current, patch)`.
pub fn form_field_after(
    current: &FieldWithWidgets,
    patch: &Value,
) -> Result<FieldWithWidgets, Error> {
    let mut field: Map<String, Value> = current.field.as_object().cloned().unwrap_or_default();
    if let Some(name) = get(patch, "name") {
        let own = string_of(field.get("name"));
        let joined = match own.rfind('.') {
            None => string_of(Some(name)),
            Some(parent) => format!("{}{}", &own[..=parent], string_of(Some(name))),
        };
        field.insert("name".into(), json!(joined));
    }
    for flag in FLAGS {
        if let Some(Value::Bool(b)) = get(patch, flag)
            && field.contains_key(flag)
        {
            field.insert(flag.into(), json!(b));
        }
    }
    if let Some(alternate) = get(patch, "alternateName") {
        field.insert(
            "alternateName".into(),
            if truthy(Some(alternate)) { alternate.clone() } else { Value::Null },
        );
    }
    if let Some(mapping) = get(patch, "mappingName") {
        field.insert(
            "mappingName".into(),
            if truthy(Some(mapping)) { mapping.clone() } else { Value::Null },
        );
    }
    if let Some(group) = get(patch, "groupId") {
        field.insert("groupId".into(), group.clone());
    }
    let family = field.get("family").and_then(Value::as_str).map(str::to_string);
    if family.as_deref() == Some("text") {
        if let Some(limit) = get(patch, "maxLength") {
            field.insert("maxLength".into(), limit.clone());
        }
        if let Some(value) = get(patch, "defaultValue") {
            // `value ?? ''` and `scalar(value)`: the value as given.
            let text = if value.is_null() { json!("") } else { value.clone() };
            field.insert("defaultValue".into(), text);
            field.insert(
                "defaultValueEntry".into(),
                if value.is_null() { none() } else { json!({ "kind": "scalar", "value": value }) },
            );
        }
    }
    let mut field = Value::Object(field);
    let mut redraw = false;
    if matches!(family.as_deref(), Some("combobox" | "listbox")) {
        if let Some(options) = get(patch, "options").filter(|o| truthy(Some(o))) {
            let options: Vec<Value> =
                options.as_array().map(|a| a.iter().map(option_of).collect()).unwrap_or_default();
            field = with_options(&field, options);
            redraw = true;
        }
        if let Some(defaults) = get(patch, "defaultValue") {
            let values = match defaults {
                Value::Null => Vec::new(),
                Value::Array(_) => strings(Some(defaults)),
                other => vec![string_of(Some(other))],
            };
            assert_choice_default(
                family.as_deref().unwrap_or(""),
                &option_values(&field),
                truthy(get(&field, "edit")),
                truthy(get(&field, "multiSelect")),
                &values,
            )?;
            field = with_default(&field, &values, false);
        }
    }
    let after = FieldWithWidgets { field, widgets: current.widgets.clone() };
    Ok(if redraw { redrawn(after.field.clone(), &after) } else { after })
}

// ── creating fields ─────────────────────────────────────────────────────────

/// `defaultEntryOfDraft(draft)`.
pub fn default_entry_of_draft(draft: &Value) -> Result<Value, Error> {
    let family = str_of(draft, "family").unwrap_or("");
    let options: Vec<String> =
        items(draft, "options").iter().map(|o| string_of(get(o, "value"))).collect();
    let default = get(draft, "defaultValue");
    if matches!(family, "combobox" | "listbox")
        && let Some(default) = default
    {
        let values = match default {
            Value::Array(_) => strings(Some(default)),
            other => vec![string_of(Some(other))],
        };
        assert_choice_default(
            family,
            &options,
            truthy(get(draft, "edit")),
            truthy(get(draft, "multiSelect")),
            &values,
        )?;
    }
    Ok(match family {
        // `scalar(draft.defaultValue)`: the value as given.
        "text" | "combobox" => match default {
            Some(default) => json!({ "kind": "scalar", "value": default }),
            None => none(),
        },
        "listbox" => choice_entry_of(&in_option_order(&options, &strings(default))),
        _ => none(),
    })
}

/// `standingRectOf(box, rotation)`: a box with its sides swapped under a
/// quarter turn.
pub fn standing_rect_of(bx: &Value, rotation: Option<&Value>) -> Value {
    let turn = rotation.and_then(Value::as_f64).unwrap_or(0.0);
    let turn = ((turn % 360.0) + 360.0) % 360.0;
    if turn != 90.0 && turn != 270.0 {
        return bx.clone();
    }
    let n = |k: &str| get(bx, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let (x, y, width, height) = (n("x"), n("y"), n("width"), n("height"));
    crate::value::spread(
        bx,
        [
            ("x", crate::value::number(x + (width - height) / 2.0)),
            ("y", crate::value::number(y + (height - width) / 2.0)),
            ("width", crate::value::number(height)),
            ("height", crate::value::number(width)),
        ],
    )
}

fn object_number_of(reference: &Value) -> f64 {
    if str_of(reference, "kind") == Some("objectNumber") {
        get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
    } else {
        0.0
    }
}

/// `widgetRowOfPlacement(draft, placement, at)`.
fn widget_row_of_placement(
    draft: &Value,
    placement: &Value,
    reference: &Value,
    field: f64,
    nm: Option<String>,
) -> Result<Value, Error> {
    let mut look: Map<String, Value> = Map::new();
    look.insert("subtype".into(), json!("widget"));
    for (key, value) in placement.as_object().into_iter().flatten() {
        if key != "page" && key != "exportValue" {
            look.insert(key.clone(), value.clone());
        }
    }
    let context = epdf_annotation_core::engine::resolve::DraftContext {
        reference: reference.clone(),
        nm,
        ..Default::default()
    };
    let row = epdf_annotation_core::engine::resolve::annotation_of_draft(&look, &context)
        .map_err(|e| Error::new(e.to_string()))?;
    let row = Value::Object(row);
    let family = str_of(draft, "family").unwrap_or("");
    let toggle = family == "checkbox" || family == "radio";
    let rect = standing_rect_of(get(&row, "box").unwrap_or(&Value::Null), get(&row, "rotation"));
    Ok(crate::value::spread(
        &row,
        [
            ("rect", rect),
            ("field", json!({ "kind": "objectNumber", "objectNumber": field })),
            ("fieldFamily", json!(family)),
            ("print", json!(true)),
            ("hasAppearance", json!(family != "signature")),
            ("appearanceState", if toggle { json!("Off") } else { Value::Null }),
            (
                "caption",
                if family == "pushbutton" {
                    get(placement, "caption").cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                },
            ),
        ],
    ))
}

/// `FieldDraftContext`.
pub struct FieldDraftContext {
    pub object_number: f64,
    pub widgets: Vec<Value>,
}

/// `formFieldOfDraft(draft, context)`: the field and its widgets as a read
/// of them returns them.
pub fn form_field_of_draft(
    draft: &Value,
    context: &FieldDraftContext,
) -> Result<FieldWithWidgets, Error> {
    let placements = items(draft, "widgets");
    let rows = placements
        .iter()
        .enumerate()
        .map(|(at, placement)| {
            widget_row_of_placement(
                draft,
                placement,
                context.widgets.get(at).unwrap_or(&Value::Null),
                context.object_number,
                None,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let widgets: Vec<Value> = rows
        .iter()
        .map(|row| {
            form_widget(
                object_number_of(get(row, "ref").unwrap_or(&Value::Null)),
                get(row, "page").filter(|p| !p.is_null()),
            )
        })
        .collect();
    let family = str_of(draft, "family").unwrap_or("").to_string();
    let options: Vec<Value> = if family == "combobox" || family == "listbox" {
        items(draft, "options").iter().map(option_of).collect()
    } else {
        Vec::new()
    };
    let or = |key: &str, fallback: Value| get(draft, key).cloned().unwrap_or(fallback);
    let mut field = Map::new();
    let mut put = |key: &str, value: Value| {
        field.insert(key.to_string(), value);
    };
    put("ref", json!({ "kind": "objectNumber", "objectNumber": context.object_number }));
    put("name", or("name", Value::Null));
    put("origin", json!("acroform"));
    put("readOnly", or("readOnly", json!(false)));
    put("required", or("required", json!(false)));
    put("noExport", or("noExport", json!(false)));
    put("alternateName", or("alternateName", Value::Null));
    put("mappingName", or("mappingName", Value::Null));
    put("valueEntry", none());
    put("defaultValueEntry", none());
    put("groupId", or("groupId", Value::Null));
    put("createdBy", Value::Null);
    put("createdAt", Value::Null);
    put("filledBy", Value::Null);
    put("filledByName", Value::Null);
    put("filledAt", Value::Null);
    put("importedBy", Value::Null);
    let toggle_widget = |widget: &Value, export: Value| {
        crate::value::spread(
            widget,
            [("onState", export.clone()), ("exportValue", export), ("checked", json!(false))],
        )
    };
    match family.as_str() {
        "text" => {
            put("family", json!("text"));
            put("defaultValueEntry", default_entry_of_draft(draft)?);
            put("value", json!(""));
            put("defaultValue", or("defaultValue", json!("")));
            put("maxLength", or("maxLength", Value::Null));
            put("multiline", or("multiline", json!(false)));
            put("password", or("password", json!(false)));
            put("comb", or("comb", json!(false)));
            put("widgets", Value::Array(widgets));
        }
        "checkbox" => {
            let toggles: Vec<Value> = placements
                .iter()
                .zip(&widgets)
                .map(|(p, w)| {
                    toggle_widget(w, get(p, "exportValue").cloned().unwrap_or(json!("Yes")))
                })
                .collect();
            put("family", json!("checkbox"));
            put("checked", json!(false));
            put(
                "exportValue",
                toggles
                    .first()
                    .and_then(|t| get(t, "exportValue").cloned())
                    .unwrap_or(json!("Yes")),
            );
            put("widgets", Value::Array(toggles));
        }
        "radio" => {
            let toggles: Vec<Value> = placements
                .iter()
                .zip(&widgets)
                .map(|(p, w)| toggle_widget(w, get(p, "exportValue").cloned().unwrap_or(json!(""))))
                .collect();
            put("family", json!("radio"));
            put("value", json!("Off"));
            put("radiosInUnison", or("radiosInUnison", json!(false)));
            put("noToggleToOff", or("noToggleToOff", json!(false)));
            put("widgets", Value::Array(toggles));
        }
        "combobox" => {
            put("family", json!("combobox"));
            put("defaultValueEntry", default_entry_of_draft(draft)?);
            put("value", json!(""));
            put("defaultValue", or("defaultValue", json!("")));
            put("edit", or("edit", json!(false)));
            put("options", Value::Array(options));
            put("widgets", Value::Array(widgets));
        }
        "listbox" => {
            let defaults = strings(get(draft, "defaultValue"));
            let values: Vec<String> = options.iter().map(|o| string_of(get(o, "value"))).collect();
            put("family", json!("listbox"));
            put("defaultValueEntry", default_entry_of_draft(draft)?);
            put("selectedValues", json!([]));
            put("defaultValue", json!(in_option_order(&values, &defaults)));
            put("multiSelect", or("multiSelect", json!(false)));
            put("options", Value::Array(options));
            put("topIndex", json!(0));
            put("widgets", Value::Array(widgets));
        }
        other => {
            put("family", json!(other));
            put("widgets", Value::Array(widgets));
        }
    }
    Ok(FieldWithWidgets { field: Value::Object(field), widgets: rows })
}

// ── widgets (`forms/widgets.ts`) ────────────────────────────────────────────

fn fit_to_family(
    family: &str,
    value: Value,
    key: &str,
    takes: bool,
    whose: &str,
) -> Result<Value, Error> {
    let present = get(&value, key);
    if takes || present.is_none() {
        return Ok(value);
    }
    if !present.is_some_and(Value::is_null) {
        let widget = if family == "unknown" {
            "this widget".to_string()
        } else {
            format!("a {family} field's widget")
        };
        return Err(Error::engine(
            "InvalidArg",
            format!("'{key}' applies to {whose} widgets, not to {widget}"),
        )
        .with("details", json!({ "field": key })));
    }
    let mut out = value.as_object().cloned().unwrap_or_default();
    out.shift_remove(key);
    Ok(Value::Object(out))
}

/// `fitWidgetToFamily(family, value)`.
pub fn fit_widget_to_family(family: &str, value: &Value) -> Result<Value, Error> {
    let toggle = family == "checkbox" || family == "radio";
    let fitted =
        fit_to_family(family, value.clone(), "caption", family == "pushbutton", "a push button's")?;
    fit_to_family(family, fitted, "checkStyle", toggle, "a checkbox's or radio button's")
}

/// `widgetAfterPatch(row, patch)`.
pub fn widget_after_patch(row: &Value, patch: &Value) -> Result<Value, Error> {
    let family = str_of(row, "fieldFamily")
        .map_or_else(|| string_of(get(row, "fieldFamily")), str::to_string);
    let fitted = fit_widget_to_family(&family, patch)?;
    let mut full: Map<String, Value> = Map::new();
    full.insert("subtype".into(), json!("widget"));
    for (key, value) in fitted.as_object().into_iter().flatten() {
        full.insert(key.clone(), value.clone());
    }
    let current = row.as_object().cloned().unwrap_or_default();
    let after =
        epdf_annotation_core::engine::resolve::apply_annotation_patch(&current, &full, None)
            .map_err(|e| Error::new(e.to_string()))?;
    let after = Value::Object(after);
    let placed = ["box", "rect", "rotation"].iter().any(|k| fitted.get(k).is_some());
    if !placed {
        return Ok(after);
    }
    let rect =
        standing_rect_of(get(&after, "box").unwrap_or(&Value::Null), get(&after, "rotation"));
    Ok(crate::value::spread(&after, [("rect", rect)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_characters_counts_code_points() {
        assert_eq!(first_characters("a😀bc", Some(&json!(2))), "a😀");
        assert_eq!(first_characters("abc", Some(&json!(0))), "abc");
        assert_eq!(first_characters("abc", None), "abc");
    }

    #[test]
    fn a_reorder_moves_rows_together() {
        let key = |v: &Value| string_of(Some(v));
        let order = vec![json!("a"), json!("b"), json!("c"), json!("d")];
        let moved = vec![json!("d"), json!("a")];
        let out = reordered_list(&order, &moved, &json!({ "before": "c" }), &key).unwrap();
        assert_eq!(out, vec![json!("b"), json!("d"), json!("a"), json!("c")]);
        assert!(reordered_list(&order, &[json!("x")], &json!("end"), &key).is_err());
    }
}
