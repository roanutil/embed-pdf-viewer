//! The form plugin through uniffi: typed records decoded from the plugin's
//! JSON, one handle, one event enum. Every method hops to the kernel thread
//! through `KernelBound`.
//!
//! Not exported: `resetFormAction`, `resolveSubmitDataset`,
//! `commitScriptFormEffects` and `notifyWidgetEvent` (the actions plugin's
//! hooks, wired in `form_plugin`), `setValueRaw` (skips scripts), and
//! `exportValues` (`fields` returns the same values typed).

use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, FormFieldFamily, FormFieldRef, KernelBound, KernelPost, PageRef,
    PluginErrorInfo, Rect, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::bytes::Bytes;
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

fn annotation_ref(value: &Value) -> Result<Option<AnnotationRef>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    let core: epdf_annotation_core::AnnotationRef = decode(value.clone())?;
    Ok(Some(AnnotationRef::try_from(core)?))
}

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormValidation {
    /// Run the document's keystroke, validate, calculate and format scripts
    /// (needs the actions plugin with scripting on).
    Scripts,
    /// Write values straight to the engine.
    None,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormConfig {
    /// `None` is `Scripts`.
    #[uniffi(default = None)]
    pub validation: Option<FormValidation>,
}

impl From<FormConfig> for crate::FormConfig {
    fn from(c: FormConfig) -> Self {
        crate::FormConfig {
            validation: c.validation.map(|v| match v {
                FormValidation::Scripts => crate::Validation::Scripts,
                FormValidation::None => crate::Validation::None,
            }),
            ..Default::default()
        }
    }
}

// ── the snapshot ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum FormKind {
    /// No `/AcroForm`. Recovered fields may still exist.
    None,
    Acroform,
    /// An XFA form, served read-only. The XFA itself never runs.
    Xfa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldFlags {
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
    /// The raw `/Ff` word.
    pub raw: u32,
}

/// A checkbox or radio widget's state.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormToggleWidget {
    /// The `/AP /N` key that turns this widget on: what `set_checked` takes.
    pub on_state: String,
    /// What FDF and XFDF carry.
    pub export_value: String,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidget {
    /// `None` until the engine placed the widget.
    pub annotation_ref: Option<AnnotationRef>,
    pub annot_object_number: u32,
    pub page: PageRef,
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
    Signature,
    Unknown {
        raw_value: String,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormField {
    pub field_ref: FormFieldRef,
    /// Zero for a field stored as a direct object.
    pub field_object_number: u32,
    /// The fully qualified name, such as `billing.name`.
    pub name: String,
    pub family: FormFieldFamily,
    /// `true` when the field was found only through a page's `/Annots`.
    pub recovered: bool,
    pub flags: FormFieldFlags,
    /// `/TU`, the tooltip and accessible name.
    pub alternate_name: Option<String>,
    /// `/TM`, the export name.
    pub mapping_name: Option<String>,
    pub detail: FormFieldDetail,
    pub widgets: Vec<FormWidget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WidgetDto {
    #[serde(rename = "ref", default)]
    annotation_ref: Value,
    annot_object_number: f64,
    page: epdf_annotation_core::PageRef,
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
    field_object_number: u32,
    name: String,
    family: String,
    origin: String,
    flags: FormFieldFlags,
    #[serde(default)]
    alternate_name: Option<String>,
    #[serde(default)]
    mapping_name: Option<String>,
    #[serde(default)]
    widgets: Vec<WidgetDto>,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    default_value: Option<String>,
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
                default_value: dto.default_value.clone().unwrap_or_default(),
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
                default_value: dto.default_value.clone().unwrap_or_default(),
                edit: dto.edit,
                options: dto.options.clone(),
            },
            FormFieldFamily::Listbox => FormFieldDetail::Listbox {
                selected_values: dto.selected_values.clone(),
                multi_select: dto.multi_select,
                options: dto.options.clone(),
            },
            FormFieldFamily::Pushbutton => FormFieldDetail::Pushbutton,
            FormFieldFamily::Signature => FormFieldDetail::Signature,
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
                    annotation_ref: annotation_ref(&w.annotation_ref)?,
                    annot_object_number: w.annot_object_number as u32,
                    page: w.page.into(),
                    toggle,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(FormField {
            field_ref: FormFieldRef::from_json(&dto.field_ref)?,
            field_object_number: dto.field_object_number,
            name: dto.name,
            family,
            recovered: dto.origin == "recovered",
            flags: dto.flags,
            alternate_name: dto.alternate_name,
            mapping_name: dto.mapping_name,
            detail,
            widgets,
        })
    }
}

fn fields_out(list: Vec<Value>) -> Result<Vec<FormField>, Error> {
    list.iter().map(FormField::from_json).collect()
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormSnapshot {
    pub form_kind: FormKind,
    /// `/NeedAppearances`: the viewer should draw the widgets itself.
    pub needs_appearances: bool,
    pub fields: Vec<FormField>,
    /// `/CO`. `None` keeps a slot naming a field the model lacks.
    pub calculation_order: Vec<Option<FormFieldRef>>,
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
            calculation_order: Vec<Value>,
        }
        let dto: Dto = decode(value.clone())?;
        Ok(FormSnapshot {
            form_kind: dto.form_kind,
            needs_appearances: dto.needs_appearances,
            fields: fields_out(dto.fields)?,
            calculation_order: dto
                .calculation_order
                .iter()
                .map(|r| if r.is_null() { Ok(None) } else { FormFieldRef::from_json(r).map(Some) })
                .collect::<Result<_, Error>>()?,
        })
    }
}

// ── values ───────────────────────────────────────────────────────────────────

/// A value in the shape each family stores. A shape that does not fit the
/// field is refused, not coerced.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormValueInput {
    Text {
        value: String,
    },
    /// The `/AP /N` key to turn on, or `None` to clear the group.
    Toggle {
        state: Option<String>,
    },
    Choice {
        values: Vec<String>,
    },
}

impl FormValueInput {
    fn to_json(&self) -> Value {
        match self {
            FormValueInput::Text { value } => json!({ "type": "text", "value": value }),
            FormValueInput::Toggle { state } => json!({ "type": "toggle", "state": state }),
            FormValueInput::Choice { values } => json!({ "type": "choice", "values": values }),
        }
    }

    fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum Dto {
            Text { value: String },
            Toggle { state: Option<String> },
            Choice { values: Vec<String> },
        }
        Ok(match decode::<Dto>(value.clone())? {
            Dto::Text { value } => FormValueInput::Text { value },
            Dto::Toggle { state } => FormValueInput::Toggle { state },
            Dto::Choice { values } => FormValueInput::Choice { values },
        })
    }
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

/// What a value write did. A script that refused the value is `Rejected`
/// with its diagnostics, not an error.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCommitResult {
    pub status: FormCommitStatus,
    /// Whether the K/V/C/F scripts ran.
    pub scripted: bool,
    /// The scripts' messages (`app.alert`, validation failures), in order.
    pub diagnostics: Vec<String>,
    /// The script error that ended the transaction, if one did.
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
pub struct FormBatchResult {
    pub applied: Vec<FormFieldRef>,
    pub skipped: Vec<FormFieldRef>,
    pub failed: Vec<FormBatchFailure>,
}

impl TryFrom<crate::BatchResult> for FormBatchResult {
    type Error = Error;

    fn try_from(b: crate::BatchResult) -> Result<Self, Error> {
        let refs = |list: &[Value]| {
            list.iter().map(FormFieldRef::from_json).collect::<Result<Vec<_>, Error>>()
        };
        Ok(FormBatchResult {
            applied: refs(&b.applied)?,
            skipped: refs(&b.skipped)?,
            failed: b
                .failed
                .into_iter()
                .map(|f| {
                    let error: PluginErrorInfo = PluginErrorInfo {
                        code: f
                            .error
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .into(),
                        message: f
                            .error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                        capability: f
                            .error
                            .get("capability")
                            .and_then(Value::as_str)
                            .unwrap_or("form")
                            .into(),
                    };
                    FormBatchFailure {
                        field_ref: FormFieldRef::from_json(&f.entry).ok(),
                        name: f.entry.as_str().map(str::to_string),
                        error,
                    }
                })
                .collect(),
        })
    }
}

// ── fill projection ──────────────────────────────────────────────────────────

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
        on_state: String,
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

/// One widget's fill control: what a native form overlay draws on a page.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormFillItem {
    pub field_ref: FormFieldRef,
    pub annot_object_number: u32,
    pub annotation_ref: Option<AnnotationRef>,
    /// Page space.
    pub bounds: Rect,
    /// Read-only field, or a write in flight: draw it, don't accept input.
    pub disabled: bool,
    /// `/TU`, else the fully qualified name.
    pub label: String,
    pub control: FormFillControl,
}

impl TryFrom<&crate::FillItem> for FormFillItem {
    type Error = Error;

    fn try_from(item: &crate::FillItem) -> Result<Self, Error> {
        let flag = |v: &Value| v.as_bool().unwrap_or(false);
        let string = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let control = match &item.control {
            crate::FillControl::Text { value, multiline, password, max_length, comb } => {
                FormFillControl::Text {
                    value: string(value),
                    multiline: flag(multiline),
                    password: flag(password),
                    max_length: max_length.as_u64().and_then(|n| u32::try_from(n).ok()),
                    comb: flag(comb),
                }
            }
            crate::FillControl::Toggle { kind, checked, on_state } => FormFillControl::Toggle {
                radio: *kind == crate::ToggleKind::Radio,
                checked: flag(checked),
                on_state: string(on_state),
            },
            crate::FillControl::Choice { kind, edit, multi, options, selected } => {
                FormFillControl::Choice {
                    list: *kind == crate::ChoiceKind::List,
                    edit: flag(edit),
                    multi: flag(multi),
                    options: decode(options.clone())?,
                    selected: match selected {
                        Value::Array(_) => decode(selected.clone())?,
                        Value::String(s) => vec![s.clone()],
                        _ => Vec::new(),
                    },
                }
            }
            crate::FillControl::Button => FormFillControl::Button,
            crate::FillControl::Signature { signed, .. } => {
                FormFillControl::Signature { signed: *signed }
            }
        };
        Ok(FormFillItem {
            field_ref: FormFieldRef::from_json(&item.field_ref)?,
            annot_object_number: item.annot_object_number as u32,
            annotation_ref: annotation_ref(&item.annotation_ref)?,
            bounds: Rect {
                x: item.bx.x,
                y: item.bx.y,
                width: item.bx.width,
                height: item.bx.height,
            },
            disabled: item.disabled,
            label: string(&item.label),
            control,
        })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidgetHit {
    pub annot_object_number: u32,
    pub field: FormField,
    pub bounds: Rect,
}

// ── design ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormChoiceOptionInput {
    pub label: String,
    pub value: String,
}

/// A new field and its one widget. `family` is one the engine can create:
/// text, checkbox, radio, combobox or listbox.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCreateFieldInput {
    pub family: FormFieldFamily,
    pub page: PageRef,
    /// The widget's page-space box.
    pub bounds: Rect,
    /// A fully qualified name; numbered from the family when `None`.
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub options: Option<Vec<FormChoiceOptionInput>>,
}

impl From<FormCreateFieldInput> for crate::CreateFieldInput {
    fn from(i: FormCreateFieldInput) -> Self {
        crate::CreateFieldInput {
            family: i.family.as_str().into(),
            page: i.page.into(),
            bounds: crate::Box {
                x: i.bounds.x,
                y: i.bounds.y,
                width: i.bounds.width,
                height: i.bounds.height,
            },
            name: i.name,
            appearance: None,
            options: i.options.map(|o| {
                o.into_iter().map(|o| json!({ "label": o.label, "value": o.value })).collect()
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCreatedField {
    pub field: FormField,
    pub widget: Option<FormWidget>,
}

/// A field edit. `None` leaves a member alone; a key in `clear` removes it.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFieldPatch {
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
    /// The engine's `FormFieldPatch`: flags at the top level, a cleared
    /// member as `null`. `family` is the field's, which the engine checks.
    fn to_json(&self, family: FormFieldFamily) -> Result<Value, Error> {
        let mut patch = Map::new();
        patch.insert("family".into(), family.as_str().into());
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

// ── interchange ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormDataFormat {
    Fdf,
    Xfdf,
}

impl FormDataFormat {
    fn as_str(self) -> &'static str {
        match self {
            FormDataFormat::Fdf => "fdf",
            FormDataFormat::Xfdf => "xfdf",
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormDataExport {
    /// The format the engine wrote, which names the file extension.
    pub format: FormDataFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormActivationResult {
    /// The form plugin handled it: a toggle flipped, a value committed.
    Form { result: FormCommitResult },
    /// An action ran instead (a pushbutton's `/A`).
    Dispatched,
}

// ── events ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormEvent {
    ValueChanged {
        field_ref: FormFieldRef,
        field: Option<FormField>,
    },
    FieldCreated {
        field_ref: FormFieldRef,
        field: Option<FormField>,
    },
    FieldUpdated {
        field_ref: FormFieldRef,
        field: Option<FormField>,
    },
    FieldDeleted {
        field_ref: FormFieldRef,
    },
    ValidationRejected {
        field_ref: FormFieldRef,
        messages: Vec<String>,
    },
    /// The snapshot was re-read; every field may have changed.
    Resynced,
}

#[uniffi::export(with_foreign)]
pub trait FormEventListener: Send + Sync {
    fn on_event(&self, event: FormEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

fn optional_field(value: Option<&Value>) -> Result<Option<FormField>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => FormField::from_json(v).map(Some),
    }
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

fn commit(result: crate::FormCommitResult) -> FormCommitResult {
    result.into()
}

#[uniffi::export]
impl FormHandle {
    /// `None` until the field tree is in, or while reading is forbidden.
    pub async fn snapshot(&self) -> Result<Option<FormSnapshot>, ViewerError> {
        self.cap
            .call(|c| open(c)?.get_snapshot().map(|s| FormSnapshot::from_json(&s)).transpose())
            .await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap
            .call(|c| {
                Ok(match open(c)?.get_status() {
                    crate::ResourceStatus::Idle => ResourceStatus::Idle,
                    crate::ResourceStatus::Loading => ResourceStatus::Loading,
                    crate::ResourceStatus::Ready => ResourceStatus::Ready,
                    crate::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
                    crate::ResourceStatus::Error => ResourceStatus::Error,
                })
            })
            .await
    }

    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh()), |()| ()).await
    }

    pub async fn field(&self, field_ref: FormFieldRef) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(move |c| optional_field(open(c)?.get_field(&field_ref.to_json()).as_ref()))
            .await
    }

    pub async fn field_for_widget(
        &self,
        widget: AnnotationRef,
    ) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                let widget =
                    serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget))
                        .expect("a ref");
                optional_field(open(c)?.get_field_for_widget(&widget).as_ref())
            })
            .await
    }

    pub async fn fields(&self, filter: Option<FormFilter>) -> Result<Vec<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(crate::FormFilter::from);
                fields_out(open(c)?.list_fields(filter.as_ref()))
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
                    .get_value(&field_ref.to_json())
                    .as_ref()
                    .map(FormValueInput::from_json)
                    .transpose()
            })
            .await
    }

    pub async fn fill_items(&self, page: PageRef) -> Result<Vec<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.list_fill_items(page.into()).iter().map(FormFillItem::try_from).collect()
            })
            .await
    }

    pub async fn fill_item(
        &self,
        annot_object_number: u32,
    ) -> Result<Option<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_fill_item(f64::from(annot_object_number))
                    .map(|i| FormFillItem::try_from(&*i))
                    .transpose()
            })
            .await
    }

    /// The same projection as `fill_items`, under the TypeScript's other name.
    pub async fn widgets(&self, page: PageRef) -> Result<Vec<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.list_widgets(page.into()).iter().map(FormFillItem::try_from).collect()
            })
            .await
    }

    /// Start loading a page's widget geometry. Returns before it lands.
    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.ensure_loaded(page.into());
                Ok(())
            })
            .await
    }

    /// The widget under a page-space point.
    pub async fn widget_at(
        &self,
        page: PageRef,
        x: f64,
        y: f64,
    ) -> Result<Option<FormWidgetHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_widget_at(page.into(), x, y)
                    .map(|hit| {
                        Ok(FormWidgetHit {
                            annot_object_number: hit.annot_object_number as u32,
                            field: FormField::from_json(&hit.field)?,
                            bounds: Rect {
                                x: hit.bx.x,
                                y: hit.bx.y,
                                width: hit.bx.width,
                                height: hit.bx.height,
                            },
                        })
                    })
                    .transpose()
            })
            .await
    }

    pub async fn page_box(&self, page: PageRef) -> Result<Option<Rect>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_page_box(page.into()).map(Into::into))).await
    }

    pub async fn set_value(
        &self,
        field_ref: FormFieldRef,
        value: FormValueInput,
        cancel: Arc<CancelToken>,
    ) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_value(field_ref.to_json(), value.to_json())),
                commit,
            )
            .await
    }

    pub async fn set_text(
        &self,
        field_ref: FormFieldRef,
        text: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_text(field_ref.to_json(), text.into())),
                commit,
            )
            .await
    }

    /// A checkbox or radio by its on-state (`FormToggleWidget.on_state`);
    /// `None` clears the group.
    pub async fn set_checked(
        &self,
        field_ref: FormFieldRef,
        on_state: Option<String>,
        cancel: Arc<CancelToken>,
    ) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.set_checked(
                        field_ref.to_json(),
                        on_state.map_or(Value::Null, Value::from),
                    ))
                },
                commit,
            )
            .await
    }

    /// Choice options by export value.
    pub async fn set_choice(
        &self,
        field_ref: FormFieldRef,
        values: Vec<String>,
        cancel: Arc<CancelToken>,
    ) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.set_choice(
                        field_ref.to_json(),
                        values.into_iter().map(Value::from).collect(),
                    ))
                },
                commit,
            )
            .await
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
                    Ok(open(c)?.set_values(
                        entries
                            .into_iter()
                            .map(|e| (e.field_ref.to_json(), e.value.to_json()))
                            .collect(),
                    ))
                },
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    /// Values by fully qualified name, as an FDF would carry them.
    pub async fn import_values(
        &self,
        values: std::collections::HashMap<String, FormValueInput>,
        cancel: Arc<CancelToken>,
    ) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?
                        .import_values(values.into_iter().map(|(k, v)| (k, v.to_json())).collect()))
                },
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn reset(
        &self,
        field_ref: FormFieldRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.reset(field_ref.to_json())),
                |()| (),
            )
            .await
    }

    /// Reset every field, or only `fields` (every other field with `exclude`).
    pub async fn reset_all(
        &self,
        fields: Option<Vec<FormFieldRef>>,
        exclude: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let options = crate::ResetAllOptions {
                        fields: fields.map(|f| f.iter().map(FormFieldRef::to_json).collect()),
                        exclude: Some(exclude),
                    };
                    Ok(open(c)?.reset_all(options))
                },
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn create_field(
        &self,
        input: FormCreateFieldInput,
        cancel: Arc<CancelToken>,
    ) -> Result<FormCreatedField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_field(input.into())),
                |created| {
                    let field = FormField::from_json(&created.field)?;
                    let widget = created.widget.as_ref().and_then(|w| {
                        field
                            .widgets
                            .iter()
                            .find(|fw| {
                                Some(fw.annot_object_number as u64)
                                    == w.get("annotObjectNumber").and_then(Value::as_u64)
                            })
                            .cloned()
                    });
                    Ok::<_, ViewerError>(FormCreatedField { field, widget })
                },
            )
            .await?
    }

    pub async fn update_field(
        &self,
        field_ref: FormFieldRef,
        patch: FormFieldPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let cap = open(c)?;
                    let field = optional_field(cap.get_field(&field_ref.to_json()).as_ref())?
                        .ok_or_else(|| {
                            plugin_error(code::NOT_FOUND, "form", "no such field", None, None)
                        })?;
                    Ok(cap.update_field(field_ref.to_json(), patch.to_json(field.family)?))
                },
                |()| (),
            )
            .await
    }

    pub async fn delete_field(
        &self,
        field_ref: FormFieldRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete_field(field_ref.to_json())),
                |()| (),
            )
            .await
    }

    pub async fn detach_widget(
        &self,
        field_ref: FormFieldRef,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget =
                        serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget))
                            .expect("a ref");
                    Ok(open(c)?.detach_widget(field_ref.to_json(), widget))
                },
                |()| (),
            )
            .await
    }

    pub async fn attach_widget(
        &self,
        field_ref: FormFieldRef,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget =
                        serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget))
                            .expect("a ref");
                    Ok(open(c)?.attach_widget(field_ref.to_json(), widget))
                },
                |()| (),
            )
            .await
    }

    /// `None` is XFDF.
    pub async fn export_data(
        &self,
        format: Option<FormDataFormat>,
        cancel: Arc<CancelToken>,
    ) -> Result<FormDataExport, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.export_data(format.map(|f| Value::from(f.as_str())))),
                |result| {
                    let format = match result.get("format").and_then(Value::as_str) {
                        Some("fdf") => FormDataFormat::Fdf,
                        Some("xfdf") => FormDataFormat::Xfdf,
                        other => {
                            return Err(ViewerError::from(
                                Error::named(
                                    "InvalidState",
                                    format!("[form] export format {other:?}"),
                                )
                                .with("code", "invalid-state"),
                            ));
                        }
                    };
                    let bytes =
                        result.get("bytes").and_then(Bytes::from_json).ok_or_else(|| {
                            ViewerError::from(
                                Error::named("InvalidState", "[form] an export without bytes")
                                    .with("code", "invalid-state"),
                            )
                        })?;
                    Ok(FormDataExport { format, bytes: bytes.0 })
                },
            )
            .await?
    }

    /// FDF or XFDF; `None` sniffs the format from the bytes.
    pub async fn import_data(
        &self,
        bytes: Vec<u8>,
        format: Option<FormDataFormat>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.import_data(
                        Bytes(bytes).to_json(),
                        format.map(|f| Value::from(f.as_str())),
                    ))
                },
                |_| (),
            )
            .await
    }

    /// Make recovered fields durable, and bake their appearances when
    /// `bake_appearances`.
    pub async fn repair(
        &self,
        bake_appearances: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.repair(Some(json!({ "bakeAppearances": bake_appearances })))),
                |_| (),
            )
            .await
    }

    /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
    pub async fn activate_widget(
        &self,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<FormActivationResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget =
                        serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget))
                            .expect("a ref");
                    Ok(open(c)?.activate_widget(widget))
                },
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

    pub async fn can_fill(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_fill())).await
    }

    pub async fn can_design(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_design())).await
    }

    pub fn subscribe(&self, listener: Arc<dyn FormEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let value = c.on_value_changed(move |e| {
                        l.on_event(FormEvent::ValueChanged {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(Some(&e.field))?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let created = c.on_field_created(move |e| {
                        l.on_event(FormEvent::FieldCreated {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(e.field.as_ref())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let updated = c.on_field_updated(move |e| {
                        l.on_event(FormEvent::FieldUpdated {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(e.field.as_ref())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = c.on_field_deleted(move |e| {
                        l.on_event(FormEvent::FieldDeleted {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let rejected = c.on_validation_rejected(move |e| {
                        let messages = e
                            .issues
                            .iter()
                            .filter_map(|d| serde_json::to_value(d).ok())
                            .filter_map(|d| {
                                d.get("message").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect();
                        l.on_event(FormEvent::ValidationRejected {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            messages,
                        });
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

/// Every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFilter {
    #[uniffi(default = None)]
    pub family: Option<FormFieldFamily>,
    /// Fields with a widget on this page.
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    /// Exact fully qualified name.
    #[uniffi(default = None)]
    pub name: Option<String>,
}

impl From<FormFilter> for crate::FormFilter {
    fn from(f: FormFilter) -> Self {
        crate::FormFilter {
            family: f.family.map(|f| f.as_str().to_string()),
            page: f.page.map(Into::into),
            name: f.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epdf_acrojs::{ScriptDiagnostic, ScriptDiagnosticCode};

    /// Without the actions plugin's script realm the plugin has no rejected
    /// path (a commit applies, is unchanged, or throws), so the rejected
    /// mapping is pinned here, on the result a validate script produces.
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
            commit(rejected),
            FormCommitResult {
                status: FormCommitStatus::Rejected,
                scripted: true,
                diagnostics: vec!["Enter a number".into()],
                error: None,
            }
        );
    }
}
