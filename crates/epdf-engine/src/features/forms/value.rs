//! What a value write is, and what it leaves the field holding.
//!
//! G12. Ports `core/src/forms/value.ts` (`FormFieldValue`, `fieldValueOf`),
//! `core/src/forms/write.ts` (`fieldValueWriteOf`, `isAtDefault`,
//! `valueEntriesEqual`) and the field half of `core/src/forms/predict.ts`
//! (`fieldAfterValue`, `fieldAfterReset`). The engine checks a value against
//! its field here, before anything is written, and works out the field it
//! leaves; [`super::write`] then writes what changed.
//!
//! Upstream's prediction also returns each widget's row as a read would
//! show it afterwards. The writer passes it no rows (`{ field, widgets: [] }`
//! in `FormMutator.setValue` and `reset`), so only the field is ported.

use serde::{Deserialize, Serialize};

use super::snapshot::{
    FormFieldDto, FormFieldFamily, FormFieldKind, FormFieldOption, FormValueEntry,
    ToggleFieldWidget,
};
use crate::error::{EngineError, EngineErrorCode};
use crate::page_space::Coordinates;

/// A value to write to one field, in the fields a read of it returns
/// (`FormFieldValue`):
///
/// - `{ value }`: a text field's text; a radio group's or a checkbox's
///   choice, by the export value of the widget to check (`'Off'` or `null`
///   clears it); a dropdown's option value (or free text when it allows
///   editing; `null` clears it). `null` empties a text field.
/// - `{ checked }`: a checkbox. `true` checks its first widget.
/// - `{ selectedValues }`: a list's option values; `[]` clears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormFieldValue {
    Value(Option<String>),
    Checked(bool),
    SelectedValues(Vec<String>),
}

impl Serialize for FormFieldValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        match self {
            Self::Value(value) => map.serialize_entry("value", value)?,
            Self::Checked(checked) => map.serialize_entry("checked", checked)?,
            Self::SelectedValues(values) => map.serialize_entry("selectedValues", values)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FormFieldValue {
    /// The shape is told apart by its key, as upstream's `'checked' in value`
    /// tells it: `checked`, then `selectedValues`, then `value`.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let object = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        if let Some(checked) = object.get("checked") {
            return checked
                .as_bool()
                .map(Self::Checked)
                .ok_or_else(|| D::Error::custom("checked is a boolean"));
        }
        if let Some(values) = object.get("selectedValues") {
            return Vec::<String>::deserialize(values)
                .map(Self::SelectedValues)
                .map_err(|_| D::Error::custom("selectedValues is a list of strings"));
        }
        match object.get("value") {
            Some(serde_json::Value::Null) => Ok(Self::Value(None)),
            Some(serde_json::Value::String(value)) => Ok(Self::Value(Some(value.clone()))),
            Some(_) => Err(D::Error::custom("value is a string or null")),
            None => Err(D::Error::custom(
                "a form value is { value }, { checked } or { selectedValues }",
            )),
        }
    }
}

impl FormFieldValue {
    /// `shapeOf`: how a refusal names the shape.
    fn shape(&self) -> &'static str {
        match self {
            Self::Checked(_) => "{ checked }",
            Self::SelectedValues(_) => "{ selectedValues }",
            Self::Value(_) => "{ value }",
        }
    }
}

/// The value `field` holds, as a value write takes it: writing it to the
/// same field changes nothing. `None` for a field whose value a write can't
/// set (a push button, a signature, a family the engine doesn't know)
/// (`fieldValueOf`).
pub fn field_value_of<C: Coordinates>(field: &FormFieldDto<C>) -> Option<FormFieldValue> {
    match &field.kind {
        FormFieldKind::Checkbox { checked, .. } => Some(FormFieldValue::Checked(*checked)),
        FormFieldKind::Listbox { selected_values, .. } => {
            Some(FormFieldValue::SelectedValues(selected_values.clone()))
        }
        FormFieldKind::Radio { value, .. } => {
            Some(FormFieldValue::Value((value != "Off").then(|| value.clone())))
        }
        FormFieldKind::Text { value, .. } | FormFieldKind::Combobox { value, .. } => {
            Some(FormFieldValue::Value(Some(value.clone())))
        }
        _ => None,
    }
}

/// `valueOf` in `formChanges.ts`: the value a field holds, `{ value: null }`
/// for one a write can't set.
pub fn value_of<C: Coordinates>(field: &FormFieldDto<C>) -> FormFieldValue {
    field_value_of(field).unwrap_or(FormFieldValue::Value(None))
}

/// A value write, as the field's family takes it (`FieldValueWrite`): a
/// text field's text, a toggle's on-state (`None` clears the group), or a
/// choice field's option values (free text for a dropdown that allows
/// editing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValueWrite {
    Text(String),
    Toggle(Option<String>),
    Choice(Vec<String>),
}

/// The write `value` makes for `field`, checked before anything is written
/// (`fieldValueWriteOf`): the shape must be one the family takes, a
/// toggle's value must be the export value of one of its widgets (a radio
/// group that must keep a choice can't be cleared), and a choice must be
/// option values (free text for a dropdown that allows editing).
/// `InvalidArg` otherwise.
pub fn field_value_write_of<C: Coordinates>(
    field: &FormFieldDto<C>,
    value: &FormFieldValue,
) -> Result<FieldValueWrite, EngineError> {
    let name = &field.base.name;
    match (&field.kind, value) {
        (FormFieldKind::Text { .. }, FormFieldValue::Value(text)) => {
            return Ok(FieldValueWrite::Text(text.clone().unwrap_or_default()));
        }
        (FormFieldKind::Checkbox { widgets, .. }, FormFieldValue::Checked(checked)) => {
            if !checked {
                return Ok(FieldValueWrite::Toggle(None));
            }
            let first = widgets
                .first()
                .ok_or_else(|| invalid_value(format!("'{name}' has no widget to check")))?;
            return Ok(FieldValueWrite::Toggle(Some(first.on_state.clone())));
        }
        (FormFieldKind::Checkbox { widgets, .. }, FormFieldValue::Value(export)) => {
            return Ok(FieldValueWrite::Toggle(toggle_state_of(name, widgets, export.as_deref())?));
        }
        (FormFieldKind::Radio { widgets, no_toggle_to_off, .. }, FormFieldValue::Value(export)) => {
            let state = toggle_state_of(name, widgets, export.as_deref())?;
            if state.is_none() && *no_toggle_to_off {
                return Err(invalid_value(format!(
                    "'{name}' must keep a choice: it can't be cleared"
                )));
            }
            return Ok(FieldValueWrite::Toggle(state));
        }
        (FormFieldKind::Combobox { .. }, FormFieldValue::Value(choice)) => {
            return choice_of(field, choice.iter().cloned().collect());
        }
        (FormFieldKind::Listbox { .. }, FormFieldValue::SelectedValues(values)) => {
            return choice_of(field, values.clone());
        }
        (
            FormFieldKind::Text { .. }
            | FormFieldKind::Checkbox { .. }
            | FormFieldKind::Radio { .. }
            | FormFieldKind::Combobox { .. }
            | FormFieldKind::Listbox { .. },
            _,
        ) => {}
        _ => {
            return Err(invalid_value(format!(
                "{} fields hold no value you can set",
                family_name(field.base.family)
            )));
        }
    }
    let takes = match field.base.family {
        FormFieldFamily::Text => "{ value }",
        FormFieldFamily::Checkbox => "{ checked } or { value }",
        FormFieldFamily::Listbox => "{ selectedValues }",
        _ => "{ value }",
    };
    Err(invalid_value(format!(
        "a {} field takes {takes}, not {}",
        family_name(field.base.family),
        value.shape()
    )))
}

/// Whether a reset would leave `field` as it is: its value is its default,
/// and a checkbox's or radio group's buttons show that default
/// (`isAtDefault`).
pub fn is_at_default<C: Coordinates>(field: &FormFieldDto<C>) -> bool {
    let base = &field.base;
    if base.value_entry == FormValueEntry::Unsupported {
        return false;
    }
    if !value_entries_equal(&base.value_entry, &base.default_value_entry) {
        return false;
    }
    let widgets = match &field.kind {
        FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => widgets,
        _ => return true,
    };
    let state = match &base.default_value_entry {
        FormValueEntry::Scalar { value } => Some(value.as_str()),
        _ => None,
    };
    widgets.iter().all(|widget| widget.checked == (Some(widget.on_state.as_str()) == state))
}

/// Whether two value entries hold the same value (`valueEntriesEqual`).
pub fn value_entries_equal(left: &FormValueEntry, right: &FormValueEntry) -> bool {
    match (left, right) {
        (FormValueEntry::Scalar { value: a }, FormValueEntry::Scalar { value: b }) => a == b,
        (FormValueEntry::Array { values: a }, FormValueEntry::Array { values: b }) => a == b,
        (FormValueEntry::None, FormValueEntry::None)
        | (FormValueEntry::Unsupported, FormValueEntry::Unsupported) => true,
        _ => false,
    }
}

/// `forms.setValue` (`fieldAfterValue`): a text field holds the text, cut
/// to its maximum length (in characters); a checkbox checks every widget
/// with the target's export value and on-state; a radio group checks the
/// first button with the value, or every one with it when its radios move
/// in unison; a choice field holds its option values in option order, or a
/// dropdown's free text. Refuses what [`field_value_write_of`] refuses.
pub fn field_after_value<C: Coordinates>(
    field: &FormFieldDto<C>,
    value: &FormFieldValue,
) -> Result<FormFieldDto<C>, EngineError> {
    match field_value_write_of(field, value)? {
        FieldValueWrite::Text(text) => {
            let FormFieldKind::Text { value: current, max_length, .. } = &field.kind else {
                return Ok(field.clone());
            };
            let text = first_characters(&text, *max_length);
            if &text == current {
                return Ok(field.clone());
            }
            let mut after = field.clone();
            after.base.value_entry = FormValueEntry::Scalar { value: text.clone() };
            if let FormFieldKind::Text { value, .. } = &mut after.kind {
                *value = text;
            }
            Ok(after)
        }
        FieldValueWrite::Toggle(state) => {
            let widgets = match &field.kind {
                FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
                    widgets
                }
                _ => return Ok(field.clone()),
            };
            let target = match &state {
                None => None,
                Some(state) => widgets.iter().position(|widget| &widget.on_state == state),
            };
            Ok(toggled(field, target))
        }
        FieldValueWrite::Choice(values) => match &field.kind {
            FormFieldKind::Combobox { .. } | FormFieldKind::Listbox { .. } => {
                Ok(choice_field_with(field, &values))
            }
            _ => Ok(field.clone()),
        },
    }
}

/// `forms.reset` (`fieldAfterReset`): back to the default value. A text
/// field holds its default (or nothing); a choice field its default
/// options; a checkbox or radio group checks the widget its default names,
/// or none. A default the field can't take is `Unknown`, as upstream's
/// `notReset`.
pub fn field_after_reset<C: Coordinates>(
    field: &FormFieldDto<C>,
) -> Result<FormFieldDto<C>, EngineError> {
    let defaults = &field.base.default_value_entry;
    match &field.kind {
        FormFieldKind::Text { .. } => {
            let (value, entry) = match defaults {
                FormValueEntry::Scalar { value } => (value.clone(), defaults.clone()),
                FormValueEntry::None => (String::new(), FormValueEntry::None),
                _ => return Err(not_reset(field)),
            };
            let mut after = field.clone();
            after.base.value_entry = entry;
            if let FormFieldKind::Text { value: current, .. } = &mut after.kind {
                *current = value;
            }
            Ok(after)
        }
        FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
            let state = match defaults {
                FormValueEntry::Scalar { value } => Some(value.as_str()),
                FormValueEntry::None => None,
                _ => return Err(not_reset(field)),
            };
            match state {
                None | Some("Off") => Ok(toggled(field, None)),
                Some(state) => {
                    let target = widget_named_by(widgets, state).ok_or_else(|| not_reset(field))?;
                    Ok(toggled(field, Some(target)))
                }
            }
        }
        FormFieldKind::Combobox { options, .. } | FormFieldKind::Listbox { options, .. } => {
            let mut values = match defaults {
                FormValueEntry::Scalar { value } => vec![value.clone()],
                FormValueEntry::Array { values } => values.clone(),
                FormValueEntry::None => Vec::new(),
                FormValueEntry::Unsupported => return Err(not_reset(field)),
            };
            // An empty default selects nothing, unless an option's value is
            // empty.
            if values.len() == 1 && values[0].is_empty() && !choice_matches(options, &values) {
                values.clear();
            }
            if !choice_matches(options, &values) && !free_text_allowed(field, &values) {
                return Err(not_reset(field));
            }
            Ok(choice_field_with(field, &values))
        }
        _ => Err(not_reset(field)),
    }
}

/// The first `limit` characters of `text` (characters, as the file counts
/// them); all without a limit (`firstCharacters`). JavaScript counts code
/// points with `Array.from`, as `chars` does.
fn first_characters(text: &str, limit: Option<i32>) -> String {
    match limit {
        Some(limit) if limit > 0 => text.chars().take(limit as usize).collect(),
        _ => text.to_owned(),
    }
}

/// The widget a checkbox's or radio group's default names: the one whose
/// on-state it is, or, when none is, the one at that position (a field with
/// `/Opt` written by a tool that names its widgets by position)
/// (`widgetNamedBy`).
fn widget_named_by(widgets: &[ToggleFieldWidget], state: &str) -> Option<usize> {
    if let Some(named) = widgets.iter().position(|widget| widget.on_state == state) {
        return Some(named);
    }
    if state.is_empty() || !state.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    state.parse::<usize>().ok().filter(|at| *at < widgets.len())
}

/// A checkbox or radio group with widget `target` checked (`None`: none),
/// and the widgets that go with it: every widget with its export value and
/// on-state for a checkbox or radios in unison, otherwise that one. Its value
/// names the checked widget's on-state (`/V` names the appearance state that
/// is on), or `Off` (`toggled`).
fn toggled<C: Coordinates>(field: &FormFieldDto<C>, target: Option<usize>) -> FormFieldDto<C> {
    let mut after = field.clone();
    let (widgets, unison) = match &mut after.kind {
        FormFieldKind::Checkbox { widgets, .. } => (widgets, true),
        FormFieldKind::Radio { widgets, radios_in_unison, .. } => {
            let unison = *radios_in_unison;
            (widgets, unison)
        }
        _ => return after,
    };
    let on = target.and_then(|at| widgets.get(at)).cloned();
    for (at, widget) in widgets.iter_mut().enumerate() {
        widget.checked = match &on {
            None => false,
            Some(on) if unison => {
                widget.export_value == on.export_value && widget.on_state == on.on_state
            }
            Some(_) => Some(at) == target,
        };
    }
    let value_entry = FormValueEntry::Scalar {
        value: on.as_ref().map_or_else(|| "Off".to_owned(), |on| on.on_state.clone()),
    };
    match &mut after.kind {
        FormFieldKind::Checkbox { widgets, checked, .. } => {
            *checked = widgets.iter().any(|widget| widget.checked);
        }
        FormFieldKind::Radio { widgets, value, .. } => {
            *value = widgets
                .iter()
                .find(|widget| widget.checked)
                .map_or_else(|| "Off".to_owned(), |widget| widget.export_value.clone());
        }
        _ => {}
    }
    after.base.value_entry = value_entry;
    after
}

/// Whether every value is one of the options' values.
fn choice_matches(options: &[FormFieldOption], values: &[String]) -> bool {
    values.iter().all(|value| options.iter().any(|option| &option.value == value))
}

fn free_text_allowed<C: Coordinates>(field: &FormFieldDto<C>, values: &[String]) -> bool {
    matches!(field.kind, FormFieldKind::Combobox { edit: true, .. }) && values.len() == 1
}

/// The option indexes `values` name, each the first option with that
/// value, in option order (`matchedIndexes`).
fn matched_indexes(options: &[FormFieldOption], values: &[String]) -> Vec<usize> {
    let mut indexes: Vec<usize> = values
        .iter()
        .filter_map(|value| options.iter().position(|option| &option.value == value))
        .collect();
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

/// A choice field holding `values`: its options' selection, its value and
/// `/V` (`choiceFieldWith`).
fn choice_field_with<C: Coordinates>(
    field: &FormFieldDto<C>,
    values: &[String],
) -> FormFieldDto<C> {
    let mut after = field.clone();
    let options = match &mut after.kind {
        FormFieldKind::Combobox { options, .. } | FormFieldKind::Listbox { options, .. } => options,
        _ => return after,
    };
    let matched =
        if choice_matches(options, values) { matched_indexes(options, values) } else { Vec::new() };
    let free_text = !values.is_empty() && matched.is_empty();
    for (at, option) in options.iter_mut().enumerate() {
        option.selected = matched.contains(&at);
    }
    let chosen: Vec<String> = if free_text {
        values.to_vec()
    } else {
        matched.iter().map(|at| options[*at].value.clone()).collect()
    };
    after.base.value_entry = choice_entry_of(&chosen);
    match &mut after.kind {
        FormFieldKind::Combobox { value, .. } => {
            *value = chosen.first().cloned().unwrap_or_default();
        }
        FormFieldKind::Listbox { selected_values, .. } => *selected_values = chosen,
        _ => {}
    }
    after
}

/// How `/V` (or `/DV`) holds choice values: none, one string, or an array
/// (`choiceEntryOf`).
fn choice_entry_of(values: &[String]) -> FormValueEntry {
    match values {
        [] => FormValueEntry::None,
        [one] => FormValueEntry::Scalar { value: one.clone() },
        _ => FormValueEntry::Array { values: values.to_vec() },
    }
}

/// A checkbox's or radio group's on-state for an export value; `'Off'` and
/// `None` clear it (`toggleStateOf`).
fn toggle_state_of(
    name: &str,
    widgets: &[ToggleFieldWidget],
    export_value: Option<&str>,
) -> Result<Option<String>, EngineError> {
    let Some(export_value) = export_value.filter(|value| *value != "Off") else {
        return Ok(None);
    };
    widgets
        .iter()
        .find(|widget| widget.export_value == export_value)
        .map(|widget| Some(widget.on_state.clone()))
        .ok_or_else(|| {
            invalid_value(format!("no widget of '{name}' has the export value '{export_value}'"))
        })
}

/// `choiceOf`: option values, none twice, one unless the list box takes
/// several, free text only for a dropdown that allows editing.
fn choice_of<C: Coordinates>(
    field: &FormFieldDto<C>,
    values: Vec<String>,
) -> Result<FieldValueWrite, EngineError> {
    let name = &field.base.name;
    let unique: std::collections::BTreeSet<&String> = values.iter().collect();
    if unique.len() != values.len() {
        return Err(invalid_value("the values must not repeat".to_owned()));
    }
    let options = match &field.kind {
        FormFieldKind::Listbox { multi_select, options, .. } => {
            if !multi_select && values.len() > 1 {
                return Err(invalid_value(format!(
                    "'{name}' takes one value (it has no multiSelect)"
                )));
            }
            options
        }
        FormFieldKind::Combobox { options, .. } => options,
        _ => return Ok(FieldValueWrite::Choice(values)),
    };
    let free_text = free_text_allowed(field, &values);
    if !free_text
        && let Some(unknown) =
            values.iter().find(|value| !options.iter().any(|option| &option.value == *value))
    {
        return Err(invalid_value(format!("'{unknown}' is not an option value of '{name}'")));
    }
    Ok(FieldValueWrite::Choice(values))
}

/// A family as upstream spells it in a message.
fn family_name(family: FormFieldFamily) -> String {
    serde_json::to_value(family)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn invalid_value(message: String) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
        .with_details(serde_json::json!({ "field": "value" }))
}

fn not_reset<C: Coordinates>(field: &FormFieldDto<C>) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::Unknown,
        format!("'{}' could not be reset", field.base.name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_value_is_told_apart_by_its_key() {
        let read = |value: serde_json::Value| serde_json::from_value::<FormFieldValue>(value);
        assert_eq!(read(json!({ "value": "x" })).unwrap(), FormFieldValue::Value(Some("x".into())));
        assert_eq!(read(json!({ "value": null })).unwrap(), FormFieldValue::Value(None));
        assert_eq!(read(json!({ "checked": true })).unwrap(), FormFieldValue::Checked(true));
        assert_eq!(
            read(json!({ "selectedValues": ["a"] })).unwrap(),
            FormFieldValue::SelectedValues(vec!["a".into()])
        );
        assert!(read(json!({})).is_err());
        assert!(read(json!({ "value": 3 })).is_err());
        assert!(read(json!({ "type": "text", "value": "x" })).is_ok(), "the value is the key");
        for value in
            [json!({ "value": null }), json!({ "checked": false }), json!({ "selectedValues": [] })]
        {
            assert_eq!(serde_json::to_value(read(value.clone()).unwrap()).unwrap(), value);
        }
    }

    #[test]
    fn a_text_write_counts_characters_not_bytes() {
        assert_eq!(first_characters("héllo", Some(2)), "hé");
        assert_eq!(first_characters("abc", Some(0)), "abc");
        assert_eq!(first_characters("abc", None), "abc");
    }

    #[test]
    fn a_default_names_its_widget_by_on_state_or_position() {
        let widget = |on: &str| ToggleFieldWidget {
            widget: super::super::snapshot::FormWidgetRef {
                annot_object_number: 1,
                page_object_number: 2,
            },
            on_state: on.into(),
            export_value: on.into(),
            checked: false,
        };
        let widgets = [widget("Red"), widget("Blue")];
        assert_eq!(widget_named_by(&widgets, "Blue"), Some(1));
        assert_eq!(widget_named_by(&widgets, "1"), Some(1));
        assert_eq!(widget_named_by(&widgets, "2"), None);
        assert_eq!(widget_named_by(&widgets, "Green"), None);
    }

    #[test]
    fn choice_values_are_held_as_none_one_or_several() {
        assert_eq!(choice_entry_of(&[]), FormValueEntry::None);
        assert_eq!(choice_entry_of(&["a".into()]), FormValueEntry::Scalar { value: "a".into() });
        assert_eq!(
            choice_entry_of(&["a".into(), "b".into()]),
            FormValueEntry::Array { values: vec!["a".into(), "b".into()] }
        );
    }
}
