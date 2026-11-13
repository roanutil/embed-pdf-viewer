//! `engine-core/src/transfer/formCopy.ts` (`planFormCopies`) and the parts of
//! `transfer/formFieldCreates.ts` and `dto/PdfAction.ts` it reads: what a
//! copy of form fields is, decided once. The engine applies it, and the form
//! plugin shows it before the engine answers and copies a radio button by it.
//! Fields, rows and actions are the engine's records as JSON, in page space.

use std::collections::HashSet;

use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::engine::{annotation_key, encode_field_ref_key, field_value_of};
use crate::value::{get, items, str_of, truthy};

/// `WIDGET_ACTION_EVENTS`: every event a widget's actions run on, in
/// `EPDF_ANNOT_ACTION_*` order.
pub const WIDGET_ACTION_EVENTS: [&str; 11] = [
    "activate",
    "cursorEnter",
    "cursorExit",
    "mouseDown",
    "mouseUp",
    "focus",
    "blur",
    "pageOpen",
    "pageClose",
    "pageVisible",
    "pageInvisible",
];

/// `WIDGET_FLAG_NAMES`: the flags a widget row has, which a copy keeps.
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

/// `needsScriptRight(action)`: a JavaScript, submit-form or URI action
/// anywhere in the chain.
pub fn needs_script_right(action: &Value) -> bool {
    matches!(str_of(action, "type"), Some("javascript" | "submit-form" | "uri"))
        || items(action, "next").iter().any(needs_script_right)
}

/// `nodeWriteOf(node)`: a read action node as a write makes it; `None` for
/// one a write can't make.
fn node_write_of(node: &Value) -> Option<Value> {
    let mut next = Vec::new();
    for child in items(node, "next") {
        next.push(node_write_of(child)?);
    }
    let member = |key: &str| get(node, key).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    match str_of(node, "type")? {
        "javascript" => {
            out.insert("type".into(), json!("javascript"));
            out.insert("script".into(), member("script"));
        }
        "goto" => {
            out.insert("type".into(), json!("goto"));
            out.insert("destination".into(), member("destination"));
        }
        "uri" => {
            out.insert("type".into(), json!("uri"));
            out.insert("uri".into(), member("uri"));
        }
        "named" => {
            out.insert("type".into(), json!("named"));
            out.insert("name".into(), member("name"));
        }
        "hide" => {
            out.insert("type".into(), json!("hide"));
            out.insert("targets".into(), member("targets"));
            out.insert("hide".into(), member("hide"));
        }
        "reset-form" => {
            out.insert("type".into(), json!("reset-form"));
            out.insert("fields".into(), member("fields"));
            out.insert("exclude".into(), member("exclude"));
        }
        "submit-form" => {
            let payload = get(node, "payload").filter(|p| truthy(Some(p)))?;
            // `submitFormFlagsOf(payload)`: the payload without its url, fields and charSet.
            for (key, value) in payload.as_object().into_iter().flatten() {
                if !matches!(key.as_str(), "url" | "fields" | "charSet") {
                    out.insert(key.clone(), value.clone());
                }
            }
            out.insert("type".into(), json!("submit-form"));
            out.insert("url".into(), get(payload, "url").cloned().unwrap_or(Value::Null));
            out.insert("fields".into(), get(payload, "fields").cloned().unwrap_or(Value::Null));
        }
        _ => return None,
    }
    if !next.is_empty() {
        out.insert("next".into(), Value::Array(next));
    }
    Some(Value::Object(out))
}

/// `actionWriteOf(tree)`: a read tree as a write makes it again; `None` for
/// an incomplete tree or one a write can't make.
pub fn action_write_of(tree: &Value) -> Option<Value> {
    if truthy(get(tree, "incomplete")) {
        return None;
    }
    node_write_of(get(tree, "root").filter(|r| truthy(Some(r)))?)
}

/// `isFieldScript(action)`: JavaScript all through.
fn is_field_script(action: &Value) -> bool {
    str_of(action, "type") == Some("javascript")
        && items(action, "next").iter().all(is_field_script)
}

/// `fieldScriptOf(tree)`: the field script of a read tree, unless it isn't
/// JavaScript all through.
pub fn field_script_of(tree: &Value) -> Option<Value> {
    action_write_of(tree).filter(is_field_script)
}

/// `holdsValue(field)`: a value a write can set.
pub fn holds_value(field: &Value) -> bool {
    matches!(get(field, "valueEntry").and_then(|e| str_of(e, "kind")), Some("scalar" | "array"))
}

/// `fieldActionsOf(field, mayScript, dropped)`: a field's scripts as a
/// create writes them; each left out is reported.
pub fn field_actions_of(
    field: &Value,
    may_script: bool,
    dropped: &mut Vec<Value>,
) -> Map<String, Value> {
    let mut actions = Map::new();
    let reference = get(field, "ref").cloned().unwrap_or(Value::Null);
    for (event, tree) in get(field, "actions").and_then(Value::as_object).into_iter().flatten() {
        if !truthy(Some(tree)) {
            continue;
        }
        let member = format!("actions.{event}");
        match field_script_of(tree) {
            None => dropped
                .push(json!({ "ref": reference, "field": member, "reason": "unsupported-action" })),
            Some(_) if !may_script => dropped
                .push(json!({ "ref": reference, "field": member, "reason": "script-not-allowed" })),
            Some(script) => {
                actions.insert(event.clone(), script);
            }
        }
    }
    actions
}

/// `widgetPlacementOf(field, row, same, mayScript, dropped)`: a widget row
/// as a create places it, on its own page, with its look and actions.
pub fn widget_placement_of(
    field: &Value,
    row: &Value,
    may_script: bool,
    dropped: &mut Vec<Value>,
) -> Map<String, Value> {
    let reference = get(field, "ref").cloned().unwrap_or(Value::Null);
    let row_ref = get(row, "ref").cloned().unwrap_or(Value::Null);
    let mut actions = Map::new();
    for event in WIDGET_ACTION_EVENTS {
        let Some(tree) =
            get(row, "actions").and_then(|a| get(a, event)).filter(|t| truthy(Some(t)))
        else {
            continue;
        };
        let mut drop = |reason: &str| {
            dropped.push(json!({ "ref": reference, "widget": row_ref, "field": format!("actions.{event}"), "reason": reason }));
        };
        match action_write_of(tree) {
            None => drop("unsupported-action"),
            Some(write) if !may_script && needs_script_right(&write) => drop("script-not-allowed"),
            Some(write) => {
                actions.insert(event.to_string(), write);
            }
        }
    }
    let kind = str_of(field, "kind").unwrap_or("");
    let row_key = annotation_key(&row_ref);
    let toggle = if kind == "checkbox" || kind == "radio" {
        items(field, "widgets").iter().find(|w| {
            get(w, "ref").filter(|r| truthy(Some(r))).is_some_and(|r| annotation_key(r) == row_key)
        })
    } else {
        None
    };
    let member = |key: &str| get(row, key).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert("page".into(), member("page"));
    out.insert("rect".into(), member("rect"));
    if truthy(get(row, "rotation")) {
        out.insert("rotation".into(), member("rotation"));
    }
    out.insert("strokeWidth".into(), member("strokeWidth"));
    out.insert("borderStyle".into(), member("borderStyle"));
    out.insert("textAlign".into(), member("textAlign"));
    // A new widget has none of these: only what the row has is written.
    for key in
        ["color", "interiorColor", "dashArray", "checkStyle", "fontFamily", "fontSize", "fontColor"]
    {
        let value = member(key);
        if !value.is_null() && !epdf_kernel::is_undefined(&value) {
            out.insert(key.into(), value);
        }
    }
    if kind == "pushbutton" {
        let caption = member("caption");
        if !caption.is_null() && !epdf_kernel::is_undefined(&caption) {
            out.insert("caption".into(), caption);
        }
    }
    if let Some(toggle) = toggle
        && let Some(export_value) = get(toggle, "exportValue")
    {
        out.insert("exportValue".into(), export_value.clone());
    }
    if !actions.is_empty() {
        out.insert("actions".into(), Value::Object(actions));
    }
    out
}

/// `widgetFlagsOf(row)`: a widget row's flags, which a made-again widget keeps.
pub fn widget_flags_of(row: &Value) -> Map<String, Value> {
    WIDGET_FLAG_NAMES
        .iter()
        .map(|name| {
            (name.to_string(), get(row, name).cloned().unwrap_or_else(epdf_kernel::undefined))
        })
        .collect()
}

/// `fieldDraftOf(field, name, actions, widgets, where)`: the create a field
/// read becomes, under `name`.
pub fn field_draft_of(
    field: &Value,
    name: &str,
    actions: Map<String, Value>,
    widgets: Vec<Value>,
    place: &str,
) -> Result<Value, Error> {
    let member = |key: &str| get(field, key).cloned().unwrap_or(Value::Null);
    let mut base = Map::new();
    base.insert("name".into(), json!(name));
    base.insert("readOnly".into(), member("readOnly"));
    base.insert("required".into(), member("required"));
    base.insert("noExport".into(), member("noExport"));
    for key in ["alternateName", "mappingName", "groupId"] {
        if !member(key).is_null() {
            base.insert(key.into(), member(key));
        }
    }
    if !actions.is_empty() {
        base.insert("actions".into(), Value::Object(actions));
    }
    base.insert("widgets".into(), Value::Array(widgets));
    let has_default =
        get(field, "defaultValueEntry").and_then(|e| str_of(e, "kind")) != Some("none");
    let options = || {
        Value::Array(
            items(field, "options")
                .iter()
                .map(|o| json!({ "label": get(o, "label").cloned().unwrap_or(Value::Null), "value": get(o, "value").cloned().unwrap_or(Value::Null) }))
                .collect(),
        )
    };
    let kind = str_of(field, "kind").unwrap_or("");
    base.insert("kind".into(), json!(kind));
    match kind {
        "text" => {
            if has_default {
                base.insert("defaultValue".into(), member("defaultValue"));
            }
            if !member("maxLength").is_null() {
                base.insert("maxLength".into(), member("maxLength"));
            }
            base.insert("multiline".into(), member("multiline"));
            base.insert("password".into(), member("password"));
            base.insert("comb".into(), member("comb"));
        }
        "radio" => {
            base.insert("radiosInUnison".into(), member("radiosInUnison"));
            base.insert("noToggleToOff".into(), member("noToggleToOff"));
        }
        "combobox" => {
            base.insert("edit".into(), member("edit"));
            base.insert("options".into(), options());
            if has_default {
                base.insert("defaultValue".into(), member("defaultValue"));
            }
        }
        "listbox" => {
            base.insert("multiSelect".into(), member("multiSelect"));
            base.insert("options".into(), options());
            if has_default {
                base.insert("defaultValue".into(), member("defaultValue"));
            }
        }
        "checkbox" | "pushbutton" | "signature" => {}
        _ => return Err(Error::engine("InvalidArg", format!("{place} has no family"))),
    }
    Ok(Value::Object(base))
}

/// `fieldMatches(field, ref)`: a field by its name or its object number.
pub fn field_matches(field: &Value, reference: &Value) -> bool {
    if str_of(reference, "kind") == Some("fqn") {
        return get(field, "name") == get(reference, "name");
    }
    let own = get(field, "ref");
    own.and_then(|r| str_of(r, "kind")) == Some("objectNumber")
        && own.and_then(|r| get(r, "objectNumber")).and_then(Value::as_f64)
            == get(reference, "objectNumber").and_then(Value::as_f64)
}

/// `fieldNameTaken(names, name)`: one has the name, one sits under it, or
/// one is its parent.
pub fn field_name_taken(names: &HashSet<String>, name: &str) -> bool {
    names.contains(name)
        || names.iter().any(|other| {
            other.starts_with(&format!("{name}.")) || name.starts_with(&format!("{other}."))
        })
}

/// `copyNameOf(name, taken)`: its last part with the next free number.
/// `email` becomes `email_2`, `text_1` becomes `text_2`, and `billing.name`
/// becomes `billing.name_2`.
pub fn copy_name_of(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (group, last) = match name.rfind('.') {
        Some(dot) => (&name[..=dot], &name[dot + 1..]),
        None => ("", name),
    };
    // `/^(.*)_(\d+)$/`: the greedy stem, then the digits after its last `_`.
    let numbered = last.rfind('_').and_then(|at| {
        let digits = &last[at + 1..];
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| (&last[..at], digits.parse::<f64>().unwrap_or(f64::INFINITY)))
    });
    let (stem, mut count) = match numbered {
        Some((stem, n)) => (stem, n + 1.0),
        None => (last, 2.0),
    };
    let named = |count: f64| format!("{group}{stem}_{}", epdf_kernel::js::number_to_string(count));
    while taken(&named(count)) {
        count += 1.0;
    }
    named(count)
}

/// `PlannedFieldCopy`.
#[derive(Clone, Debug)]
pub struct PlannedFieldCopy {
    /// The field it copies, by its place among the fields given.
    pub source: usize,
    /// What `create` writes.
    pub draft: Value,
    /// The widgets it copies, in `draft.widgets` order.
    pub widgets: Vec<Value>,
    /// Each copied widget's flags, in `draft.widgets` order.
    pub flags: Vec<Map<String, Value>>,
    /// The value it gets; `None` when the copy writes none.
    pub value: Option<Value>,
}

/// `FormCopyPlan`.
#[derive(Clone, Debug, Default)]
pub struct FormCopyPlan {
    pub creates: Vec<PlannedFieldCopy>,
    /// The calculation order among the creates, by their place in `creates`.
    pub calculation_order: Vec<usize>,
    pub dropped: Vec<Value>,
}

/// `FormCopyPlacement`, with `offset` as `(x, y)`.
#[derive(Clone, Debug, Default)]
pub struct CopyPlacement {
    pub page: Option<Value>,
    pub offset: Option<(f64, f64)>,
}

impl CopyPlacement {
    /// A placement from an op's or an option's `page` and `offset` members.
    pub fn of(holder: &Value) -> CopyPlacement {
        CopyPlacement {
            page: get(holder, "page").filter(|p| truthy(Some(p))).cloned(),
            offset: get(holder, "offset").filter(|o| o.is_object()).map(|o| {
                let read = |k: &str| get(o, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                (read("x"), read("y"))
            }),
        }
    }

    /// `moved(widget)`: on `page` when one is named, moved by `offset`.
    pub fn moved(&self, mut widget: Map<String, Value>) -> Map<String, Value> {
        let (dx, dy) = self.offset.unwrap_or((0.0, 0.0));
        if let Some(page) = &self.page {
            widget.insert("page".into(), page.clone());
        }
        if let Some(rect) = widget.get("rect").filter(|r| truthy(Some(r))).cloned() {
            let mut moved = rect.as_object().cloned().unwrap_or_default();
            let at = |k: &str| get(&rect, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            moved.insert("x".into(), epdf_kernel::js::number(at("x") + dx));
            moved.insert("y".into(), epdf_kernel::js::number(at("y") + dy));
            widget.insert("rect".into(), Value::Object(moved));
        }
        widget
    }
}

/// The parts of a form's snapshot `planFormCopies` reads.
#[derive(Clone, Copy)]
pub struct FormParts<'a> {
    pub fields: &'a [Value],
    pub widgets: &'a [Value],
    pub calculation_order: &'a [Value],
}

/// `planFormCopies(input)`: the copies of `fields`, fields of the form whose
/// fields, widget rows and calculation order are given, in the order given.
/// A field named twice is copied twice. `NotFound` for a field the form
/// doesn't have; `InvalidArg` when `widgets` names a widget of none of the
/// fields, or none of a field's.
pub fn plan_form_copies(
    form: FormParts<'_>,
    fields: &[Value],
    widgets: Option<&[Value]>,
    placement: &CopyPlacement,
    values: bool,
    may_script: bool,
) -> Result<FormCopyPlan, Error> {
    let FormParts { fields: form_fields, widgets: form_widgets, calculation_order } = form;
    let row_of = |key: &str| {
        form_widgets.iter().find(|row| get(row, "ref").map(annotation_key).as_deref() == Some(key))
    };
    let only: Option<Vec<String>> = widgets.map(|list| list.iter().map(annotation_key).collect());
    let mut names: HashSet<String> =
        form_fields.iter().filter_map(|f| str_of(f, "name").map(str::to_string)).collect();
    let mut plan = FormCopyPlan::default();
    let mut sources: Vec<&Value> = Vec::new();
    let mut copied: HashSet<String> = HashSet::new();
    for (index, reference) in fields.iter().enumerate() {
        let Some(field) = form_fields.iter().find(|candidate| field_matches(candidate, reference))
        else {
            let key = encode_field_ref_key(reference).unwrap_or_default();
            return Err(Error::engine("NotFound", format!("copy: no form field {key}"))
                .with("details", json!({ "item": index })));
        };
        let rows: Vec<&Value> = items(field, "widgets")
            .iter()
            .filter_map(|widget| {
                let key = annotation_key(get(widget, "ref").filter(|r| truthy(Some(r)))?);
                let row = row_of(&key)?;
                only.as_ref().is_none_or(|o| o.contains(&key)).then_some(row)
            })
            .collect();
        for row in &rows {
            copied.insert(annotation_key(get(row, "ref").unwrap_or(&Value::Null)));
        }
        if only.is_some() && rows.is_empty() {
            return Err(Error::engine(
                "InvalidArg",
                format!("copy: 'widgets' names no widget of field {index}"),
            )
            .with("details", json!({ "item": index, "field": "widgets" })));
        }
        if str_of(field, "kind") == Some("unknown") {
            plan.dropped.push(json!({ "ref": get(field, "ref").cloned().unwrap_or(Value::Null), "reason": "unsupported-kind" }));
            continue;
        }
        let name = copy_name_of(str_of(field, "name").unwrap_or(""), |candidate| {
            field_name_taken(&names, candidate)
        });
        names.insert(name.clone());
        let actions = field_actions_of(field, may_script, &mut plan.dropped);
        let placed: Vec<Value> = rows
            .iter()
            .map(|row| {
                Value::Object(placement.moved(widget_placement_of(
                    field,
                    row,
                    may_script,
                    &mut plan.dropped,
                )))
            })
            .collect();
        let value = if values && str_of(field, "kind") != Some("signature") && holds_value(field) {
            field_value_of(field)
        } else {
            None
        };
        sources.push(field);
        plan.creates.push(PlannedFieldCopy {
            source: index,
            draft: field_draft_of(field, &name, actions, placed, &format!("copy: field {index}"))?,
            widgets: rows
                .iter()
                .map(|row| get(row, "ref").cloned().unwrap_or(Value::Null))
                .collect(),
            flags: rows.iter().map(|row| widget_flags_of(row)).collect(),
            value,
        });
    }
    for key in only.iter().flatten() {
        if !copied.contains(key) {
            return Err(Error::engine(
                "InvalidArg",
                format!("copy: widget {key} is a widget of none of the fields"),
            )
            .with("details", json!({ "field": "widgets" })));
        }
    }
    // The order the form gives the sources that kept their calculate script.
    plan.calculation_order = calculation_order
        .iter()
        .filter(|r| !r.is_null())
        .flat_map(|reference| {
            plan.creates
                .iter()
                .enumerate()
                .filter(|(at, create)| {
                    field_matches(sources[*at], reference)
                        && truthy(get(&create.draft, "actions").and_then(|a| get(a, "calculate")))
                })
                .map(|(at, _)| at)
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_takes_the_next_free_number() {
        let taken: HashSet<String> =
            ["email", "email_2", "text_1", "billing.name"].iter().map(|s| s.to_string()).collect();
        let free = |n: &str| field_name_taken(&taken, n);
        assert_eq!(copy_name_of("email", free), "email_3");
        assert_eq!(copy_name_of("text_1", free), "text_2");
        assert_eq!(copy_name_of("billing.name", free), "billing.name_2");
    }

    #[test]
    fn a_name_under_or_over_another_is_taken() {
        let taken: HashSet<String> = ["a.b"].iter().map(|s| s.to_string()).collect();
        assert!(field_name_taken(&taken, "a"));
        assert!(field_name_taken(&taken, "a.b.c"));
        assert!(!field_name_taken(&taken, "ab"));
    }
}
