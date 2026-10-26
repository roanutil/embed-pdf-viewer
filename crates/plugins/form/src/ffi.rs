//! The form plugin through uniffi: typed records read from the plugin's
//! JSON, one handle, one event enum. Every method hops to the kernel thread
//! through `KernelBound`.
//!
//! Migrated by hand in Task 13.18 to the current plugin and not compiled:
//! `epdf-ffi` does not build at this commit, and Task 13.33 brings it,
//! `epdf-viewer-host` and Swift up together. Refs are written here in the
//! engine's current spelling (`objectNumber`), since `epdf-ffi`'s own
//! `to_json` still writes the retired one.
//!
//! Not exported: the actions plugin's seams (`resetFormAction`,
//! `resolveSubmitDataset`, `commitScriptFormEffects`, `notifyWidgetEvent`),
//! which the plugin wires itself, and the painter reads (`getWidgetPainting`,
//! `getTextEditorLayout`, `listShownWidgets`), which a native layer reaches
//! once 13.33 types the scene. Removed with the TypeScript: `setText`,
//! `setChecked`, `setChoice`, `resetAll`, `attachWidget` (now `add_widget`),
//! `detachWidget` (now `remove_widget`), `exportData` and `importData` (now
//! `export` and `import`, with bundles), `getFillItem`, and `flags.raw`.

use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, FormFieldFamily, FormFieldRef, KernelBound, KernelPost, PageRef,
    PluginErrorInfo, Rect, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::FormCapability;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[form] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

fn invalid_state(message: String) -> Error {
    Error::named("InvalidState", message).with("code", "invalid-state")
}

fn open(cap: &FormCapability) -> Result<&FormCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "form",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

// ── refs, in the engine's current spelling ─────────────────────────────────

fn page_json(page: PageRef) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": page.page_object_number })
}

fn page_of(value: &Value) -> Result<PageRef, Error> {
    let n = value
        .get("objectNumber")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_state(format!("[form] not a page ref: {value}")))?;
    Ok(PageRef { page_object_number: n as u32 })
}

fn field_ref_json(reference: &FormFieldRef) -> Value {
    match reference {
        FormFieldRef::ObjectNumber { field_object_number } => {
            json!({ "kind": "objectNumber", "objectNumber": field_object_number })
        }
        FormFieldRef::Fqn { name } => json!({ "kind": "fqn", "name": name }),
    }
}

fn field_ref_of(value: &Value) -> Result<FormFieldRef, Error> {
    match value.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => Ok(FormFieldRef::ObjectNumber {
            field_object_number: value.get("objectNumber").and_then(Value::as_u64).unwrap_or(0)
                as u32,
        }),
        Some("fqn") => Ok(FormFieldRef::Fqn {
            name: value.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        }),
        _ => Err(invalid_state(format!("[form] not a field ref: {value}"))),
    }
}

fn annotation_ref_json(reference: &AnnotationRef) -> Value {
    match reference {
        AnnotationRef::ObjectNumber { page, annot_object_number } => json!({
            "kind": "objectNumber", "page": page_json(*page), "objectNumber": annot_object_number,
        }),
        AnnotationRef::Nm { page, nm } => {
            json!({ "kind": "nm", "page": page_json(*page), "nm": nm })
        }
        AnnotationRef::Index { page, index, .. } => {
            json!({ "kind": "baseIndex", "page": page_json(*page), "baseIndex": index })
        }
    }
}

fn annotation_ref_of(value: &Value) -> Result<Option<AnnotationRef>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    match value.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => Ok(Some(AnnotationRef::ObjectNumber {
            page: page_of(value.get("page").unwrap_or(&Value::Null))?,
            annot_object_number: value.get("objectNumber").and_then(Value::as_u64).unwrap_or(0)
                as u32,
        })),
        _ => Err(invalid_state(format!("[form] an annotation ref of another kind: {value}"))),
    }
}

fn rect_of(value: &Value) -> Rect {
    let n = |k: &str| value.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Rect { x: n("x"), y: n("y"), width: n("width"), height: n("height") }
}

fn rect_json(rect: Rect) -> Value {
    json!({ "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height })
}

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormValidation {
    /// Run the document's keystroke, validate, calculate and format scripts
    /// (needs the actions plugin with scripting on).
    Scripts,
    /// Accept every value.
    None,
}

/// What the app registers over the defaults (`FormConfig`).
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormConfig {
    /// `None` is `Scripts`.
    #[uniffi(default = None)]
    pub validation: Option<FormValidation>,
}

impl From<FormConfig> for Option<Value> {
    fn from(c: FormConfig) -> Self {
        let validation = c.validation?;
        Some(json!({ "validation": match validation {
            FormValidation::Scripts => "scripts",
            FormValidation::None => "none",
        } }))
    }
}

// ── fields ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum FormKind {
    None,
    Acroform,
    Xfa,
}

/// A checkbox or radio widget's state.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormToggleWidget {
    pub on_state: String,
    /// What the form data holds while it's checked: what `set_value` takes.
    pub export_value: String,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidget {
    /// `None` for a widget on no page, or stored as a direct object.
    pub annotation_ref: Option<AnnotationRef>,
    pub object_number: u32,
    pub page: Option<PageRef>,
    pub toggle: Option<FormToggleWidget>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, uniffi::Record)]
pub struct FormChoiceOption {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub selected: bool,
}

/// The family-specific half of a field.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormFieldDetail {
    Text {
        value: String,
        default_value: String,
        max_length: Option<u32>,
        multiline: bool,
        password: bool,
        comb: bool,
    },
    Checkbox {
        checked: bool,
        export_value: String,
    },
    Radio {
        value: String,
        radios_in_unison: bool,
        no_toggle_to_off: bool,
    },
    Combobox {
        value: String,
        default_value: String,
        edit: bool,
        options: Vec<FormChoiceOption>,
    },
    Listbox {
        selected_values: Vec<String>,
        multi_select: bool,
        options: Vec<FormChoiceOption>,
    },
    Pushbutton,
    Signature {
        signed: bool,
    },
    Unknown {
        raw_value: String,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormField {
    pub field_ref: FormFieldRef,
    /// The fully qualified name, such as `billing.name`.
    pub name: String,
    pub family: FormFieldFamily,
    /// `true` when the field was found only through a page's `/Annots`.
    pub recovered: bool,
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
    /// `/TU`, the tooltip and accessible name.
    pub alternate_name: Option<String>,
    /// `/TM`, the export name.
    pub mapping_name: Option<String>,
    /// Who fills it in: its group, or `None` for every user.
    pub group_id: Option<String>,
    pub detail: FormFieldDetail,
    pub widgets: Vec<FormWidget>,
    /// The engine's field record, whole, as JSON: attribution, the value
    /// entries and the actions this record doesn't type.
    pub fields_json: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WidgetDto {
    #[serde(rename = "ref", default)]
    annotation_ref: Value,
    object_number: f64,
    #[serde(default)]
    page: Value,
    #[serde(default)]
    on_state: Option<String>,
    #[serde(default)]
    export_value: Option<String>,
    #[serde(default)]
    checked: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldDto {
    #[serde(rename = "ref")]
    field_ref: Value,
    name: String,
    family: String,
    origin: String,
    read_only: bool,
    required: bool,
    no_export: bool,
    #[serde(default)]
    alternate_name: Option<String>,
    #[serde(default)]
    mapping_name: Option<String>,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    value_entry: Value,
    #[serde(default)]
    widgets: Vec<WidgetDto>,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    default_value: Value,
    #[serde(default)]
    max_length: Option<i64>,
    #[serde(default)]
    multiline: bool,
    #[serde(default)]
    password: bool,
    #[serde(default)]
    comb: bool,
    #[serde(default)]
    checked: bool,
    #[serde(default)]
    export_value: Option<String>,
    #[serde(default)]
    radios_in_unison: bool,
    #[serde(default)]
    no_toggle_to_off: bool,
    #[serde(default)]
    edit: bool,
    #[serde(default)]
    options: Vec<FormChoiceOption>,
    #[serde(default)]
    selected_values: Vec<String>,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    raw_value: Option<String>,
}

impl FormField {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let dto: FieldDto = decode(value.clone())?;
        let family = FormFieldFamily::parse(&dto.family);
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let detail = match family {
            FormFieldFamily::Text => FormFieldDetail::Text {
                value: text(&dto.value),
                default_value: text(&dto.default_value),
                max_length: dto.max_length.and_then(|n| u32::try_from(n).ok()),
                multiline: dto.multiline,
                password: dto.password,
                comb: dto.comb,
            },
            FormFieldFamily::Checkbox => FormFieldDetail::Checkbox {
                checked: dto.checked,
                export_value: dto.export_value.clone().unwrap_or_default(),
            },
            FormFieldFamily::Radio => FormFieldDetail::Radio {
                value: text(&dto.value),
                radios_in_unison: dto.radios_in_unison,
                no_toggle_to_off: dto.no_toggle_to_off,
            },
            FormFieldFamily::Combobox => FormFieldDetail::Combobox {
                value: text(&dto.value),
                default_value: text(&dto.default_value),
                edit: dto.edit,
                options: dto.options.clone(),
            },
            FormFieldFamily::Listbox => FormFieldDetail::Listbox {
                selected_values: dto.selected_values.clone(),
                multi_select: dto.multi_select,
                options: dto.options.clone(),
            },
            FormFieldFamily::Pushbutton => FormFieldDetail::Pushbutton,
            FormFieldFamily::Signature => FormFieldDetail::Signature {
                signed: dto.value_entry.get("kind").and_then(Value::as_str) != Some("none"),
            },
            FormFieldFamily::Unknown => {
                FormFieldDetail::Unknown { raw_value: dto.raw_value.clone().unwrap_or_default() }
            }
        };
        let widgets = dto
            .widgets
            .into_iter()
            .map(|w| {
                let toggle = w.on_state.map(|on_state| FormToggleWidget {
                    on_state,
                    export_value: w.export_value.unwrap_or_default(),
                    checked: w.checked.unwrap_or(false),
                });
                Ok(FormWidget {
                    annotation_ref: annotation_ref_of(&w.annotation_ref)?,
                    object_number: w.object_number as u32,
                    page: if w.page.is_null() { None } else { Some(page_of(&w.page)?) },
                    toggle,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(FormField {
            field_ref: field_ref_of(&dto.field_ref)?,
            name: dto.name,
            family,
            recovered: dto.origin == "recovered",
            read_only: dto.read_only,
            required: dto.required,
            no_export: dto.no_export,
            alternate_name: dto.alternate_name,
            mapping_name: dto.mapping_name,
            group_id: dto.group_id,
            detail,
            widgets,
            fields_json: value.to_string(),
        })
    }
}

fn fields_out(list: &[Value]) -> Result<Vec<FormField>, Error> {
    list.iter().map(FormField::from_json).collect()
}

fn optional_field(value: Option<&Value>) -> Result<Option<FormField>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => FormField::from_json(v).map(Some),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormSnapshot {
    pub form_kind: FormKind,
    pub needs_appearances: bool,
    pub fields: Vec<FormField>,
    /// `/CO`. `None` keeps a slot naming a field the model lacks.
    pub calculation_order: Vec<Option<FormFieldRef>>,
    /// Every widget's row, as JSON (`WidgetAnnotation[]`).
    pub widgets_json: String,
}

impl FormSnapshot {
    fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Dto {
            form_kind: FormKind,
            needs_appearances: bool,
            fields: Vec<Value>,
            #[serde(default)]
            widgets: Vec<Value>,
            #[serde(default)]
            calculation_order: Vec<Value>,
        }
        let dto: Dto = decode(value.clone())?;
        Ok(FormSnapshot {
            form_kind: dto.form_kind,
            needs_appearances: dto.needs_appearances,
            fields: fields_out(&dto.fields)?,
            calculation_order: dto
                .calculation_order
                .iter()
                .map(|r| if r.is_null() { Ok(None) } else { field_ref_of(r).map(Some) })
                .collect::<Result<_, Error>>()?,
            widgets_json: Value::Array(dto.widgets).to_string(),
        })
    }
}

// ── values ───────────────────────────────────────────────────────────────────

/// A value to write (`FormFieldValue`): a text field's text, a radio
/// group's, checkbox's or dropdown's choice by export value (`None` clears
/// it); a checkbox by `Checked`; a list's option values.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormValueInput {
    Value { value: Option<String> },
    Checked { checked: bool },
    SelectedValues { values: Vec<String> },
}

impl FormValueInput {
    fn to_json(&self) -> Value {
        match self {
            FormValueInput::Value { value } => json!({ "value": value }),
            FormValueInput::Checked { checked } => json!({ "checked": checked }),
            FormValueInput::SelectedValues { values } => json!({ "selectedValues": values }),
        }
    }

    fn from_json(value: &Value) -> Result<Self, Error> {
        if let Some(checked) = value.get("checked") {
            return Ok(FormValueInput::Checked { checked: checked.as_bool().unwrap_or(false) });
        }
        if let Some(values) = value.get("selectedValues") {
            return Ok(FormValueInput::SelectedValues { values: decode(values.clone())? });
        }
        Ok(FormValueInput::Value {
            value: value.get("value").and_then(Value::as_str).map(str::to_string),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormSetValueStatus {
    Applied,
    Unchanged,
    /// The form's own validation refused the value.
    Rejected,
}

/// What `set_value` resolves: the field as it is now, and what the write did.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormSetValueResult {
    pub field: FormField,
    pub status: FormSetValueStatus,
}

fn set_value_result(value: &Value) -> Result<FormSetValueResult, ViewerError> {
    let status = match value.get("status").and_then(Value::as_str) {
        Some("applied") => FormSetValueStatus::Applied,
        Some("rejected") => FormSetValueStatus::Rejected,
        _ => FormSetValueStatus::Unchanged,
    };
    Ok(FormSetValueResult {
        field: FormField::from_json(value.get("field").unwrap_or(&Value::Null))?,
        status,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormCommitStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
}

impl From<crate::FormCommitStatus> for FormCommitStatus {
    fn from(s: crate::FormCommitStatus) -> Self {
        match s {
            crate::FormCommitStatus::Applied => FormCommitStatus::Applied,
            crate::FormCommitStatus::Unchanged => FormCommitStatus::Unchanged,
            crate::FormCommitStatus::Rejected => FormCommitStatus::Rejected,
            crate::FormCommitStatus::Failed => FormCommitStatus::Failed,
        }
    }
}

/// A write through the document's scripts.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCommitResult {
    pub status: FormCommitStatus,
    pub scripted: bool,
    pub diagnostics: Vec<String>,
    pub error: Option<String>,
}

impl From<crate::FormCommitResult> for FormCommitResult {
    fn from(r: crate::FormCommitResult) -> Self {
        FormCommitResult {
            status: r.status.into(),
            scripted: r.scripted,
            diagnostics: r.diagnostics.into_iter().map(|d| d.message).collect(),
            error: r.error.map(|e| e.message),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormBatchFailure {
    pub field_ref: Option<FormFieldRef>,
    /// The entry's name when it was addressed by name (`import_values`).
    pub name: Option<String>,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormBatchSkip {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormBatchResult {
    pub applied: Vec<FormFieldRef>,
    pub skipped: Vec<FormBatchSkip>,
    pub failed: Vec<FormBatchFailure>,
}

fn batch_result(value: &Value) -> Result<FormBatchResult, ViewerError> {
    let list = |key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let text =
        |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Ok(FormBatchResult {
        applied: list("applied").iter().map(field_ref_of).collect::<Result<_, Error>>()?,
        skipped: list("skipped")
            .iter()
            .map(|s| FormBatchSkip { name: text(s, "ref"), reason: text(s, "reason") })
            .collect(),
        failed: list("failed")
            .iter()
            .map(|f| {
                let entry = f.get("ref").unwrap_or(&Value::Null);
                let error = f.get("error").unwrap_or(&Value::Null);
                FormBatchFailure {
                    field_ref: field_ref_of(entry).ok(),
                    name: entry.as_str().map(str::to_string),
                    error: PluginErrorInfo {
                        code: text(error, "code"),
                        message: text(error, "message"),
                        capability: text(error, "capability"),
                    },
                }
            })
            .collect(),
    })
}

// ── the fill projection ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormFillControl {
    Text {
        value: String,
        multiline: bool,
        password: bool,
        max_length: Option<u32>,
        comb: bool,
    },
    Toggle {
        radio: bool,
        checked: bool,
        export_value: String,
    },
    Choice {
        list: bool,
        edit: bool,
        multi: bool,
        options: Vec<FormChoiceOption>,
        selected: Vec<String>,
    },
    Button,
    Signature {
        signed: bool,
    },
}

/// One widget as a fill control draws it (`FormWidgetItem`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormFillItem {
    pub field_ref: FormFieldRef,
    pub annot_object_number: u32,
    pub annotation_ref: Option<AnnotationRef>,
    /// Page space.
    pub bounds: Rect,
    /// The widget's own look (`FormWidgetLook`), as JSON.
    pub look_json: String,
    /// Read-only, no permission to fill, or filling turned off: draw it,
    /// don't take input.
    pub disabled: bool,
    pub label: String,
    pub control: FormFillControl,
}

fn fill_item(item: &Value) -> Result<FormFillItem, Error> {
    let flag = |k: &str| item.get(k).and_then(Value::as_bool).unwrap_or(false);
    let text = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let control = match item.get("control").and_then(Value::as_str) {
        Some("text") => FormFillControl::Text {
            value: text("value"),
            multiline: flag("multiline"),
            password: flag("password"),
            max_length: item.get("maxLength").and_then(Value::as_u64).map(|n| n as u32),
            comb: flag("comb"),
        },
        Some("toggle") => FormFillControl::Toggle {
            radio: item.get("kind").and_then(Value::as_str) == Some("radio"),
            checked: flag("checked"),
            export_value: text("exportValue"),
        },
        Some("choice") => FormFillControl::Choice {
            list: item.get("kind").and_then(Value::as_str) == Some("list"),
            edit: flag("edit"),
            multi: flag("multi"),
            options: decode(item.get("options").cloned().unwrap_or(json!([])))?,
            selected: decode(item.get("selected").cloned().unwrap_or(json!([])))?,
        },
        Some("button") => FormFillControl::Button,
        Some("signature") => FormFillControl::Signature { signed: flag("signed") },
        other => return Err(invalid_state(format!("[form] a fill control {other:?}"))),
    };
    Ok(FormFillItem {
        field_ref: field_ref_of(item.get("fieldRef").unwrap_or(&Value::Null))?,
        annot_object_number: item.get("annotObjectNumber").and_then(Value::as_u64).unwrap_or(0)
            as u32,
        annotation_ref: annotation_ref_of(item.get("annotationRef").unwrap_or(&Value::Null))?,
        bounds: rect_of(item.get("box").unwrap_or(&Value::Null)),
        look_json: item.get("look").cloned().unwrap_or(Value::Null).to_string(),
        disabled: flag("disabled"),
        label: text("label"),
        control,
    })
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidgetHit {
    pub annot_object_number: u32,
    pub field: FormField,
    pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormValidationResult {
    pub valid: bool,
    /// The required fields you may fill in that are empty, in page order.
    pub missing: Vec<FormField>,
}

// ── design ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormChoiceOptionInput {
    pub label: String,
    pub value: String,
}

/// A new field with one widget (`FormFieldDraft`); `draft_json` is the
/// engine's whole draft when the typed members don't say enough.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCreateFieldInput {
    pub family: FormFieldFamily,
    /// A fully qualified name, such as `billing.name`.
    pub name: String,
    pub page: PageRef,
    /// The widget's page-space box.
    pub bounds: Rect,
    #[uniffi(default = None)]
    pub options: Option<Vec<FormChoiceOptionInput>>,
    /// A radio button's or checkbox's export value.
    #[uniffi(default = None)]
    pub export_value: Option<String>,
}

impl FormCreateFieldInput {
    fn to_json(&self) -> Value {
        let mut widget = Map::new();
        widget.insert("page".into(), page_json(self.page));
        widget.insert("rect".into(), rect_json(self.bounds));
        if let Some(export_value) = &self.export_value {
            widget.insert("exportValue".into(), json!(export_value));
        }
        let mut draft = Map::new();
        draft.insert("family".into(), json!(self.family.as_str()));
        draft.insert("name".into(), json!(self.name));
        draft.insert("widgets".into(), json!([widget]));
        if let Some(options) = &self.options {
            let options: Vec<Value> =
                options.iter().map(|o| json!({ "label": o.label, "value": o.value })).collect();
            draft.insert("options".into(), Value::Array(options));
        }
        Value::Object(draft)
    }
}

/// Where a field's new widget goes (`Partial<WidgetPlacement>`): without a
/// box, next in line after the field's widgets.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormWidgetPlacement {
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    #[uniffi(default = None)]
    pub bounds: Option<Rect>,
    #[uniffi(default = None)]
    pub export_value: Option<String>,
}

impl FormWidgetPlacement {
    fn to_json(&self) -> Value {
        let mut placement = Map::new();
        if let Some(page) = self.page {
            placement.insert("page".into(), page_json(page));
        }
        if let Some(bounds) = self.bounds {
            placement.insert("rect".into(), rect_json(bounds));
        }
        if let Some(export_value) = &self.export_value {
            placement.insert("exportValue".into(), json!(export_value));
        }
        Value::Object(placement)
    }
}

/// What `add_widget` resolves: the field, and its new widget's row as JSON.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidgetAdded {
    pub field: FormField,
    pub widget_json: String,
}

/// A field edit (`FormFieldPatch`). `None` leaves a member alone; a key in
/// `clear` removes it.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFieldPatch {
    /// The field's own segment of its dotted name.
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub read_only: Option<bool>,
    #[uniffi(default = None)]
    pub required: Option<bool>,
    #[uniffi(default = None)]
    pub no_export: Option<bool>,
    #[uniffi(default = None)]
    pub alternate_name: Option<String>,
    #[uniffi(default = None)]
    pub mapping_name: Option<String>,
    #[uniffi(default = None)]
    pub default_value: Option<String>,
    #[uniffi(default = None)]
    pub max_length: Option<u32>,
    #[uniffi(default = None)]
    pub options: Option<Vec<FormChoiceOptionInput>>,
    #[uniffi(default = [])]
    pub clear: Vec<FormFieldPatchKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormFieldPatchKey {
    AlternateName,
    MappingName,
    DefaultValue,
    MaxLength,
}

impl FormFieldPatch {
    fn to_json(&self) -> Result<Value, Error> {
        let mut patch = Map::new();
        let set: [(&str, Option<Value>); 9] = [
            ("name", self.name.clone().map(Value::from)),
            ("readOnly", self.read_only.map(Value::from)),
            ("required", self.required.map(Value::from)),
            ("noExport", self.no_export.map(Value::from)),
            ("alternateName", self.alternate_name.clone().map(Value::from)),
            ("mappingName", self.mapping_name.clone().map(Value::from)),
            ("defaultValue", self.default_value.clone().map(Value::from)),
            ("maxLength", self.max_length.map(Value::from)),
            (
                "options",
                self.options.as_ref().map(|o| {
                    Value::Array(
                        o.iter().map(|o| json!({ "label": o.label, "value": o.value })).collect(),
                    )
                }),
            ),
        ];
        for (key, value) in set {
            if let Some(value) = value {
                patch.insert(key.into(), value);
            }
        }
        for key in &self.clear {
            let name = match key {
                FormFieldPatchKey::AlternateName => "alternateName",
                FormFieldPatchKey::MappingName => "mappingName",
                FormFieldPatchKey::DefaultValue => "defaultValue",
                FormFieldPatchKey::MaxLength => "maxLength",
            };
            if patch.insert(name.into(), Value::Null).is_some_and(|v| !v.is_null()) {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    "form",
                    format!("field patch both sets and clears \"{name}\""),
                    None,
                    None,
                ));
            }
        }
        Ok(Value::Object(patch))
    }
}

/// What `import` did (`FormImportResult`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormImportResult {
    pub fields: Vec<FormField>,
    /// Each imported field's ref in the bundle and here, as JSON.
    pub ref_map_json: String,
    /// What was left out, and why, as JSON.
    pub dropped_json: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormActivationResult {
    /// The form's own scripts ran.
    Form { result: FormCommitResult },
    /// The widget's action ran.
    Dispatched,
}

// ── events ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormEvent {
    ValueChanged {
        field: FormField,
        origin_json: String,
    },
    FieldCreated {
        field: FormField,
        origin_json: String,
    },
    FieldUpdated {
        field: FormField,
        origin_json: String,
    },
    FieldDeleted {
        field_ref: FormFieldRef,
        origin_json: String,
    },
    ValidationRejected {
        field: FormField,
        messages: Vec<String>,
    },
    /// The whole form was read.
    Resynced,
}

#[uniffi::export(with_foreign)]
pub trait FormEventListener: Send + Sync {
    fn on_event(&self, event: FormEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

// ── the handle ───────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct FormHandle {
    cap: KernelBound<FormCapability>,
    post: KernelPost,
}

impl FormHandle {
    /// On the kernel thread.
    pub fn new(cap: FormCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(FormHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

fn field_result(value: &Value) -> Result<FormField, ViewerError> {
    Ok(FormField::from_json(value.get("field").unwrap_or(&Value::Null))?)
}

#[uniffi::export]
impl FormHandle {
    /// `None` until the form is read, or while reading is forbidden.
    pub async fn snapshot(&self) -> Result<Option<FormSnapshot>, ViewerError> {
        self.cap
            .call(|c| open(c)?.get_snapshot().map(|s| FormSnapshot::from_json(&s)).transpose())
            .await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap
            .call(|c| {
                use epdf_kernel::mirror::ResourceStatus as Kernel;
                Ok(match open(c)?.get_status() {
                    Kernel::Idle => ResourceStatus::Idle,
                    Kernel::Loading => ResourceStatus::Loading,
                    Kernel::Ready => ResourceStatus::Ready,
                    Kernel::Forbidden => ResourceStatus::Forbidden,
                    Kernel::Error => ResourceStatus::Error,
                })
            })
            .await
    }

    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh(None)), |()| ()).await
    }

    pub async fn field(&self, field_ref: FormFieldRef) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(move |c| optional_field(open(c)?.get(&field_ref_json(&field_ref)).as_ref()))
            .await
    }

    pub async fn field_for_widget(
        &self,
        widget: AnnotationRef,
    ) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                optional_field(
                    open(c)?.get_field_for_widget(&annotation_ref_json(&widget)).as_ref(),
                )
            })
            .await
    }

    pub async fn fields(&self, filter: Option<FormFilter>) -> Result<Vec<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(|f| f.to_json());
                fields_out(&open(c)?.list(filter.as_ref()))
            })
            .await
    }

    pub async fn value(
        &self,
        field_ref: FormFieldRef,
    ) -> Result<Option<FormValueInput>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_value(&field_ref_json(&field_ref))
                    .as_ref()
                    .map(FormValueInput::from_json)
                    .transpose()
            })
            .await
    }

    /// Every value by full name, as plain data, JSON.
    pub async fn export_values(&self) -> Result<String, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.export_values().to_string())).await
    }

    pub async fn validate(&self) -> Result<FormValidationResult, ViewerError> {
        self.cap
            .call(|c| {
                let result = open(c)?.validate();
                Ok(FormValidationResult {
                    valid: result.get("valid").and_then(Value::as_bool).unwrap_or(false),
                    missing: fields_out(
                        result.get("missing").and_then(Value::as_array).map_or(&[], Vec::as_slice),
                    )?,
                })
            })
            .await
    }

    /// Each widget on a page, as a fill control draws it.
    pub async fn widgets(&self, page: PageRef) -> Result<Vec<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.list_widgets(&page_json(page)).iter().map(fill_item).collect())
            .await
    }

    /// Every page's widgets come with the form: resolves once it is read.
    pub async fn ensure_loaded(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.ensure_loaded(&page_json(page))),
                |()| (),
            )
            .await
    }

    pub async fn widget_at(
        &self,
        page: PageRef,
        x: f64,
        y: f64,
    ) -> Result<Option<FormWidgetHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_widget_at(&page_json(page), x, y)
                    .map(|hit| {
                        Ok(FormWidgetHit {
                            annot_object_number: hit.annot_object_number as u32,
                            field: FormField::from_json(&hit.field)?,
                            bounds: rect_of(&hit.bx),
                        })
                    })
                    .transpose()
            })
            .await
    }

    pub async fn page_box(&self, page: PageRef) -> Result<Option<Rect>, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.get_page_box(&page_json(page)).map(|b| Rect {
                    x: b.x,
                    y: b.y,
                    width: b.width,
                    height: b.height,
                }))
            })
            .await
    }

    pub async fn set_value(
        &self,
        field_ref: FormFieldRef,
        value: FormValueInput,
        cancel: Arc<CancelToken>,
    ) -> Result<FormSetValueResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_value(field_ref_json(&field_ref), value.to_json(), None)),
                |result| set_value_result(&result),
            )
            .await?
    }

    pub async fn set_values(
        &self,
        entries: Vec<FormValueEntry>,
        cancel: Arc<CancelToken>,
    ) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let entries = entries
                        .iter()
                        .map(|e| (field_ref_json(&e.field_ref), e.value.to_json()))
                        .collect();
                    Ok(open(c)?.set_values(entries, None))
                },
                |result| batch_result(&result),
            )
            .await?
    }

    /// Plain values by full name (`exportValues()`' JSON).
    pub async fn import_values(
        &self,
        values_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let values: Value = decode(
                        serde_json::from_str(&values_json)
                            .map_err(|e| invalid_state(e.to_string()))?,
                    )?;
                    Ok(open(c)?.import_values(values, None))
                },
                |result| batch_result(&result),
            )
            .await?
    }

    /// Each field's default, for the fields given, or every field you may
    /// fill in. Resolves the fields it changed.
    pub async fn reset(
        &self,
        fields: Option<Vec<FormFieldRef>>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<FormField>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.reset(fields.map(|f| f.iter().map(field_ref_json).collect()), None))
                },
                |result| {
                    Ok::<_, ViewerError>(fields_out(
                        result.get("fields").and_then(Value::as_array).map_or(&[], Vec::as_slice),
                    )?)
                },
            )
            .await?
    }

    pub async fn create(
        &self,
        input: FormCreateFieldInput,
        cancel: Arc<CancelToken>,
    ) -> Result<FormField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create(input.to_json(), None)),
                |result| field_result(&result),
            )
            .await?
    }

    /// The field in one more place (`addWidget`).
    pub async fn add_widget(
        &self,
        field_ref: FormFieldRef,
        placement: FormWidgetPlacement,
        cancel: Arc<CancelToken>,
    ) -> Result<FormWidgetAdded, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.add_widget(field_ref_json(&field_ref), placement.to_json(), None))
                },
                |result| {
                    Ok::<_, ViewerError>(FormWidgetAdded {
                        field: field_result(&result)?,
                        widget_json: result
                            .get("widget")
                            .cloned()
                            .unwrap_or(Value::Null)
                            .to_string(),
                    })
                },
            )
            .await?
    }

    pub async fn update(
        &self,
        field_ref: FormFieldRef,
        patch: FormFieldPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<FormField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update(field_ref_json(&field_ref), patch.to_json()?, None)),
                |result| field_result(&result),
            )
            .await?
    }

    pub async fn delete(
        &self,
        field_ref: FormFieldRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete(field_ref_json(&field_ref), None)),
                |_| (),
            )
            .await
    }

    /// One widget out of a field: it stays on its page, in no field.
    pub async fn remove_widget(
        &self,
        field_ref: FormFieldRef,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<FormField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.remove_widget(
                        field_ref_json(&field_ref),
                        annotation_ref_json(&widget),
                        None,
                    ))
                },
                |result| field_result(&result),
            )
            .await?
    }

    /// Fields out as a bundle (`FormBundle`), JSON: every field, or the
    /// ones named.
    pub async fn export(
        &self,
        fields: Option<Vec<FormFieldRef>>,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let selection = fields.map(
                        |f| json!({ "fields": f.iter().map(field_ref_json).collect::<Vec<_>>() }),
                    );
                    Ok(open(c)?.export(selection, None))
                },
                |bundle| bundle.to_string(),
            )
            .await
    }

    /// A bundle's fields copied into this document. `options_json` is
    /// `FormImportOptions` (`{}` for the defaults).
    pub async fn import(
        &self,
        bundle_json: String,
        options_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormImportResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let bundle: Value = serde_json::from_str(&bundle_json)
                        .map_err(|e| invalid_state(e.to_string()))?;
                    let options: Value = serde_json::from_str(&options_json)
                        .map_err(|e| invalid_state(e.to_string()))?;
                    Ok(open(c)?.import(bundle, options, None))
                },
                |result| {
                    Ok::<_, ViewerError>(FormImportResult {
                        fields: fields_out(
                            result
                                .get("fields")
                                .and_then(Value::as_array)
                                .map_or(&[], Vec::as_slice),
                        )?,
                        ref_map_json: result
                            .get("refMap")
                            .cloned()
                            .unwrap_or(Value::Null)
                            .to_string(),
                        dropped_json: result
                            .get("dropped")
                            .cloned()
                            .unwrap_or(Value::Null)
                            .to_string(),
                    })
                },
            )
            .await?
    }

    /// Fix a form other apps read differently. Resolves the repair's result
    /// as JSON (`FormRepairResult`).
    pub async fn repair(
        &self,
        bake_appearances: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.repair(Some(json!({ "bakeAppearances": bake_appearances })), None))
                },
                |result| result.to_string(),
            )
            .await
    }

    /// What a click on a widget does.
    pub async fn activate_widget(
        &self,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<FormActivationResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.activate_widget(annotation_ref_json(&widget), None)),
                |r| match r {
                    crate::WidgetActivationResult::Form(result) => {
                        FormActivationResult::Form { result: result.into() }
                    }
                    crate::WidgetActivationResult::Dispatched(_) => {
                        FormActivationResult::Dispatched
                    }
                },
            )
            .await
    }

    pub async fn can_read(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read())).await
    }

    pub async fn can_fill(&self, field_ref: FormFieldRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_fill(&field_ref_json(&field_ref)))).await
    }

    pub async fn can_design(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_design())).await
    }

    pub async fn can_restore_attribution(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_restore_attribution())).await
    }

    pub async fn can_write_scripts(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_write_scripts())).await
    }

    pub fn subscribe(&self, listener: Arc<dyn FormEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let origin =
                        |e: &Value| e.get("origin").cloned().unwrap_or(Value::Null).to_string();
                    let field =
                        |e: &Value| FormField::from_json(e.get("field").unwrap_or(&Value::Null));
                    let l = listener.clone();
                    let value = c.on_value_changed(move |e| {
                        l.on_event(FormEvent::ValueChanged {
                            field: field(e)?,
                            origin_json: origin(e),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let created = c.on_field_created(move |e| {
                        l.on_event(FormEvent::FieldCreated {
                            field: field(e)?,
                            origin_json: origin(e),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let updated = c.on_field_updated(move |e| {
                        l.on_event(FormEvent::FieldUpdated {
                            field: field(e)?,
                            origin_json: origin(e),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = c.on_field_deleted(move |e| {
                        l.on_event(FormEvent::FieldDeleted {
                            field_ref: field_ref_of(e.get("ref").unwrap_or(&Value::Null))?,
                            origin_json: origin(e),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let rejected = c.on_validation_rejected(move |e| {
                        let messages = e
                            .get("issues")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|d| {
                                d.get("message").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect();
                        l.on_event(FormEvent::ValidationRejected { field: field(e)?, messages });
                        Ok(())
                    });
                    let l = listener.clone();
                    let resynced = c.on_resynced(move |_| {
                        l.on_event(FormEvent::Resynced);
                        Ok(())
                    });
                    let offs = [value, created, updated, deleted, rejected, resynced];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}

/// One entry of `set_values`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormValueEntry {
    pub field_ref: FormFieldRef,
    pub value: FormValueInput,
}

/// Which fields `fields` returns: every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFilter {
    #[uniffi(default = None)]
    pub family: Option<FormFieldFamily>,
    /// Fields with a widget on this page.
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    /// One full name.
    #[uniffi(default = None)]
    pub name: Option<String>,
}

impl FormFilter {
    fn to_json(&self) -> Value {
        let mut filter = Map::new();
        if let Some(family) = self.family {
            filter.insert("family".into(), json!(family.as_str()));
        }
        if let Some(page) = self.page {
            filter.insert("page".into(), page_json(page));
        }
        if let Some(name) = &self.name {
            filter.insert("name".into(), json!(name));
        }
        Value::Object(filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epdf_acrojs::{ScriptDiagnostic, ScriptDiagnosticCode};

    /// A refusal by the form's own scripts is a result, not an error.
    #[test]
    fn a_rejected_commit_is_a_result_not_an_error() {
        let rejected = crate::FormCommitResult {
            diagnostics: vec![ScriptDiagnostic {
                code: ScriptDiagnosticCode::InvalidFieldValue,
                message: "Enter a number".into(),
            }],
            ..crate::FormCommitResult::bare(crate::FormCommitStatus::Rejected, true)
        };
        assert_eq!(
            FormCommitResult::from(rejected),
            FormCommitResult {
                status: FormCommitStatus::Rejected,
                scripted: true,
                diagnostics: vec!["Enter a number".into()],
                error: None,
            }
        );
    }

    #[test]
    fn refs_are_written_in_the_current_spelling() {
        let field = FormFieldRef::ObjectNumber { field_object_number: 7 };
        assert_eq!(field_ref_json(&field), json!({ "kind": "objectNumber", "objectNumber": 7 }));
        assert_eq!(field_ref_of(&field_ref_json(&field)).unwrap(), field);
    }
}
