//! The document's form state at one instant.
//!
//! G8. Ports `services/src/features/forms/internal/readFormSnapshot.ts` and
//! the read half of `FormReader.ts`. Operation: `forms.list`.
//!
//! A snapshot is DETACHED: it is built from a model PDFium walked once, and
//! any mutation makes it stale. Nothing here caches, because deciding what
//! counts as a mutation belongs to whoever owns the session; a caller rebuilds
//! the model and this rebuilds the snapshot.
//!
//! # A field is document-scoped and a widget is page-scoped
//!
//! One logical field holds the value. Its widgets are where that value is
//! drawn, and a radio group is exactly this: one field, several widgets, one
//! of which is checked. So the value lives on the field and the toggle state
//! lives per widget, and the family DTOs differ mostly in which of the two
//! they lean on.
//!
//! # Every family keeps the raw `/V` alongside its convenience
//!
//! `value_entry` is the exact shape of the PDF object, including absent and
//! malformed. `value` and its siblings are derived from it, and they lose
//! information: a scalar reading of an array `/V` is its first element. A
//! write path has to see what was there, so the lossless form travels too.

use pdfium::{FieldActionEvent, FormField, FormModel};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::features::actions::reader::{ActionTree, read_action_model};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;

/// What kind of interactive form the document declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormKind {
    /// No `/AcroForm` at all. Recovered fields may still exist.
    None,
    Acroform,
    /// The `/AcroForm` carries an `/XFA`. The shell is served read-only and
    /// the XFA is never executed.
    Xfa,
}

impl FormKind {
    fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Acroform,
            2 => Self::Xfa,
            _ => Self::None,
        }
    }
}

/// What kind of control a field is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
    /// A field type this build does not model. The forward-compatibility
    /// hatch, like an annotation's `unsupported`.
    Unknown,
}

impl FormFieldFamily {
    /// Decode an `EPDF_FORMFIELD_FAMILY_*` code.
    pub fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Pushbutton,
            2 => Self::Checkbox,
            3 => Self::Radio,
            4 => Self::Text,
            5 => Self::Combobox,
            6 => Self::Listbox,
            7 => Self::Signature,
            _ => Self::Unknown,
        }
    }
}

/// Where a field was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldOrigin {
    /// Reachable from `/AcroForm /Fields`, as the spec requires.
    Acroform,
    /// Reachable only through a page's `/Annots`, which is a common producer
    /// bug. Reconciled on every read so it behaves like any other field, and
    /// invisible to other PDF processors until a repair makes it durable.
    Recovered,
}

/// How a field is addressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormFieldRef {
    #[serde(rename = "objectNumber")]
    ObjectNumber {
        #[serde(rename = "fieldObjectNumber")]
        field: u32,
    },
    /// The fully qualified name, for a field the document stored as a direct
    /// object and which therefore has no object number.
    #[serde(rename = "fqn")]
    Fqn { name: String },
}

/// The `/Ff` bits every family shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldFlags {
    /// The USER must not change the value. Programmatic writes are still
    /// accepted, because a calculated field is read-only and script-written;
    /// enforcing fill policy is the application's.
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
    /// The raw word, for everything not surfaced.
    pub raw: u32,
}

/// `/Ff` bit positions, ISO 32000 Tables 226, 228 and 230.
mod ff {
    pub const READ_ONLY: u32 = 1 << 0;
    pub const REQUIRED: u32 = 1 << 1;
    pub const NO_EXPORT: u32 = 1 << 2;
    pub const MULTILINE: u32 = 1 << 12;
    pub const PASSWORD: u32 = 1 << 13;
    pub const NO_TOGGLE_TO_OFF: u32 = 1 << 14;
    pub const EDIT: u32 = 1 << 18;
    pub const MULTI_SELECT: u32 = 1 << 21;
    pub const COMB: u32 = 1 << 24;
    pub const RADIOS_IN_UNISON: u32 = 1 << 25;
}

/// The exact shape of a field's `/V` or `/DV`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormValueEntry {
    None,
    Scalar {
        value: String,
    },
    Array {
        values: Vec<String>,
    },
    /// Present, and neither a string nor an array of them.
    Unsupported,
}

impl FormValueEntry {
    /// The scalar reading, which is lossy by construction: an array answers
    /// its first element and anything else answers the empty string.
    fn scalar(&self) -> String {
        match self {
            Self::Scalar { value } => value.clone(),
            Self::Array { values } => values.first().cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// Where a widget is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormWidgetRef {
    pub annot_object_number: u32,
    pub page_object_number: u32,
}

/// A checkbox or radio widget, which always carries its appearance-state
/// machinery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleFieldWidget {
    #[serde(flatten)]
    pub widget: FormWidgetRef,
    /// The non-`Off` key of the widget's `/AP /N`, which is the token a
    /// toggle write addresses this widget by.
    pub on_state: String,
    /// The `/Opt` entry when there is one, else the on-state. What FDF and
    /// XFDF carry.
    pub export_value: String,
    pub checked: bool,
}

/// One option of a combo or list box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldOption {
    pub label: String,
    pub value: String,
    pub selected: bool,
}

/// A field's four JavaScript events.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldActions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keystroke: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculate: Option<ActionTree>,
}

impl FieldActions {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The trunk every family shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldBase {
    #[serde(rename = "ref")]
    pub field_ref: FormFieldRef,
    /// Zero for a direct, spec-violating field dictionary.
    pub field_object_number: u32,
    /// The fully qualified name, such as `billing.name`.
    pub name: String,
    pub family: FormFieldFamily,
    pub origin: FormFieldOrigin,
    pub flags: FormFieldFlags,
    /// `/TU`, the tooltip.
    pub alternate_name: Option<String>,
    /// `/TM`, the name an export uses instead of the field's own.
    pub mapping_name: Option<String>,
    pub value_entry: FormValueEntry,
    pub default_value_entry: FormValueEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<FieldActions>,
}

/// The family-specific half.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum FormFieldKind {
    Text {
        value: String,
        default_value: String,
        /// `/MaxLen`. `None` is unlimited, and a write longer than it is
        /// truncated rather than refused.
        max_length: Option<i32>,
        multiline: bool,
        password: bool,
        /// Fixed character cells. Meaningful only with a `max_length`.
        comb: bool,
        widgets: Vec<FormWidgetRef>,
    },
    Checkbox {
        checked: bool,
        /// What is exported while checked. `Off` is never exported.
        export_value: String,
        widgets: Vec<ToggleFieldWidget>,
    },
    Radio {
        /// The checked widget's export value, or `Off` when the group is
        /// clear.
        value: String,
        /// Widgets sharing an export value check together.
        radios_in_unison: bool,
        /// The group cannot be cleared once a choice is made.
        no_toggle_to_off: bool,
        widgets: Vec<ToggleFieldWidget>,
    },
    Combobox {
        /// An option's export value, or free text when `edit` is set.
        value: String,
        default_value: String,
        edit: bool,
        options: Vec<FormFieldOption>,
        widgets: Vec<FormWidgetRef>,
    },
    Listbox {
        /// The selected options' export values, in option order.
        selected_values: Vec<String>,
        multi_select: bool,
        options: Vec<FormFieldOption>,
        widgets: Vec<FormWidgetRef>,
    },
    Pushbutton {
        widgets: Vec<FormWidgetRef>,
    },
    Signature {
        widgets: Vec<FormWidgetRef>,
    },
    Unknown {
        /// The raw `/V` as text, for diagnosis. An array is joined with
        /// commas, which is not a round trip and is not meant to be: the
        /// lossless form is on the base.
        raw_value: String,
        widgets: Vec<FormWidgetRef>,
    },
}

/// One field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldDto {
    #[serde(flatten)]
    pub base: FormFieldBase,
    #[serde(flatten)]
    pub kind: FormFieldKind,
}

/// The whole form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormSnapshot {
    pub form_kind: FormKind,
    /// `/NeedAppearances`, which asks a viewer to draw the widgets itself.
    pub needs_appearances: bool,
    pub fields: Vec<FormFieldDto>,
    /// `/AcroForm /CO`. A `None` preserves a slot naming a field the model
    /// does not have, rather than dropping it and shifting the rest.
    pub calculation_order: Vec<Option<FormFieldRef>>,
}

/// Read a whole model.
pub fn read_form_snapshot(
    model: &FormModel<'_>,
    cancel: &CancelToken,
) -> Result<FormSnapshot, EngineError> {
    cancel.check()?;
    // One budget across the document, for the reason `pages.list` uses one:
    // PDFium's limits are per action model and a form with a script on every
    // field could otherwise build an unbounded snapshot out of individually
    // reasonable ones.
    let mut budget = ActionReadBudgetTracker::default();
    let mut fields = Vec::with_capacity(model.field_count() as usize);

    for index in 0..model.field_count() {
        cancel.check()?;
        let field = model.field(index).expect("the index came from the count");
        fields.push(read_field(model, &field, index, &mut budget)?);
    }

    let mut calculation_order = Vec::new();
    for order_index in 0..model.calculation_order_count() {
        let field_index = model.calculation_order_field(order_index);
        calculation_order.push(
            u32::try_from(field_index)
                .ok()
                .and_then(|i| fields.get(i as usize))
                .map(|field| field.base.field_ref.clone()),
        );
    }

    Ok(FormSnapshot {
        form_kind: FormKind::from_code(model.form_kind()),
        needs_appearances: model.need_appearances(),
        fields,
        calculation_order,
    })
}

/// Read one field into its family's shape.
pub fn read_field(
    model: &FormModel<'_>,
    field: &FormField<'_>,
    index: u32,
    budget: &mut ActionReadBudgetTracker,
) -> Result<FormFieldDto, EngineError> {
    let object_number = field.object_number();
    let name = text(field.name())?.unwrap_or_default();
    let raw_flags = field.flags();
    let family = FormFieldFamily::from_code(field.family_code());
    let value_entry = read_value_entry(field, false)?;
    let default_value_entry = read_value_entry(field, true)?;

    let base = FormFieldBase {
        field_ref: if object_number > 0 {
            FormFieldRef::ObjectNumber { field: object_number }
        } else {
            FormFieldRef::Fqn { name: name.clone() }
        },
        field_object_number: object_number,
        name,
        family,
        origin: if field.origin_code() == 1 {
            FormFieldOrigin::Recovered
        } else {
            FormFieldOrigin::Acroform
        },
        flags: FormFieldFlags {
            read_only: raw_flags & ff::READ_ONLY != 0,
            required: raw_flags & ff::REQUIRED != 0,
            no_export: raw_flags & ff::NO_EXPORT != 0,
            raw: raw_flags,
        },
        alternate_name: text(field.alternate_name())?,
        mapping_name: text(field.mapping_name())?,
        value_entry,
        default_value_entry,
        actions: read_field_actions(model, index, budget)?,
    };

    let max_length = field.max_length();
    let kind = match family {
        FormFieldFamily::Text => FormFieldKind::Text {
            value: base.value_entry.scalar(),
            default_value: base.default_value_entry.scalar(),
            max_length: (max_length > 0).then_some(max_length),
            multiline: raw_flags & ff::MULTILINE != 0,
            password: raw_flags & ff::PASSWORD != 0,
            comb: raw_flags & ff::COMB != 0,
            widgets: read_plain_widgets(field),
        },
        FormFieldFamily::Checkbox => {
            let widgets = read_toggle_widgets(field)?;
            FormFieldKind::Checkbox {
                checked: widgets.iter().any(|w| w.checked),
                // `Yes` is §12.7.4.2.3's conventional on-state, and it is the
                // answer for a checkbox with no widgets at all, which is a
                // field nobody placed rather than a field that is off.
                export_value: widgets
                    .first()
                    .map_or_else(|| "Yes".to_owned(), |w| w.export_value.clone()),
                widgets,
            }
        }
        FormFieldFamily::Radio => {
            let widgets = read_toggle_widgets(field)?;
            FormFieldKind::Radio {
                value: widgets
                    .iter()
                    .find(|w| w.checked)
                    .map_or_else(|| "Off".to_owned(), |w| w.export_value.clone()),
                radios_in_unison: raw_flags & ff::RADIOS_IN_UNISON != 0,
                no_toggle_to_off: raw_flags & ff::NO_TOGGLE_TO_OFF != 0,
                widgets,
            }
        }
        FormFieldFamily::Combobox => FormFieldKind::Combobox {
            value: base.value_entry.scalar(),
            default_value: base.default_value_entry.scalar(),
            edit: raw_flags & ff::EDIT != 0,
            options: read_options(field)?,
            widgets: read_plain_widgets(field),
        },
        FormFieldFamily::Listbox => {
            let options = read_options(field)?;
            FormFieldKind::Listbox {
                selected_values: options
                    .iter()
                    .filter(|o| o.selected)
                    .map(|o| o.value.clone())
                    .collect(),
                multi_select: raw_flags & ff::MULTI_SELECT != 0,
                options,
                widgets: read_plain_widgets(field),
            }
        }
        FormFieldFamily::Pushbutton => {
            FormFieldKind::Pushbutton { widgets: read_plain_widgets(field) }
        }
        FormFieldFamily::Signature => {
            FormFieldKind::Signature { widgets: read_plain_widgets(field) }
        }
        FormFieldFamily::Unknown => FormFieldKind::Unknown {
            raw_value: match &base.value_entry {
                FormValueEntry::Array { values } => values.join(","),
                other => other.scalar(),
            },
            widgets: read_plain_widgets(field),
        },
    };

    Ok(FormFieldDto { base, kind })
}

fn read_value_entry(
    field: &FormField<'_>,
    is_default: bool,
) -> Result<FormValueEntry, EngineError> {
    let kind = if is_default { field.default_value_kind() } else { field.value_kind() };
    // 0 is absent, 3 is a `/V` that is neither a string nor an array of them,
    // and anything else this build has no name for is treated the same way:
    // present and unreadable, which is different from absent.
    if kind == 0 {
        return Ok(FormValueEntry::None);
    }
    if kind != 1 && kind != 2 {
        return Ok(FormValueEntry::Unsupported);
    }

    let count = if is_default { field.default_value_count() } else { field.value_count() };
    let mut values = Vec::with_capacity(count as usize);
    for index in 0..count {
        let raw = if is_default { field.default_value_at(index) } else { field.value_at(index) };
        values.push(text(raw)?.unwrap_or_default());
    }

    Ok(if kind == 1 {
        FormValueEntry::Scalar { value: values.into_iter().next().unwrap_or_default() }
    } else {
        FormValueEntry::Array { values }
    })
}

fn read_plain_widgets(field: &FormField<'_>) -> Vec<FormWidgetRef> {
    (0..field.widget_count())
        .map(|index| FormWidgetRef {
            annot_object_number: field.widget_object_number(index),
            page_object_number: field.widget_page_object_number(index),
        })
        .collect()
}

fn read_toggle_widgets(field: &FormField<'_>) -> Result<Vec<ToggleFieldWidget>, EngineError> {
    let mut widgets = Vec::with_capacity(field.widget_count() as usize);
    for index in 0..field.widget_count() {
        widgets.push(ToggleFieldWidget {
            widget: FormWidgetRef {
                annot_object_number: field.widget_object_number(index),
                page_object_number: field.widget_page_object_number(index),
            },
            on_state: text(field.widget_on_state(index))?.unwrap_or_default(),
            export_value: text(field.widget_export_value(index))?.unwrap_or_default(),
            checked: field.widget_is_checked(index),
        });
    }
    Ok(widgets)
}

fn read_options(field: &FormField<'_>) -> Result<Vec<FormFieldOption>, EngineError> {
    let mut options = Vec::with_capacity(field.option_count() as usize);
    for index in 0..field.option_count() {
        options.push(FormFieldOption {
            label: text(field.option_label(index))?.unwrap_or_default(),
            value: text(field.option_value(index))?.unwrap_or_default(),
            selected: field.option_is_selected(index),
        });
    }
    Ok(options)
}

fn read_field_actions(
    model: &FormModel<'_>,
    index: u32,
    budget: &mut ActionReadBudgetTracker,
) -> Result<Option<FieldActions>, EngineError> {
    let mut read = |event: FieldActionEvent| -> Result<Option<ActionTree>, EngineError> {
        match model.field_action_model(index, event) {
            Some(action) => read_action_model(&action, budget).map(Some),
            None => Ok(None),
        }
    };
    let actions = FieldActions {
        keystroke: read(FieldActionEvent::Keystroke)?,
        format: read(FieldActionEvent::Format)?,
        validate: read(FieldActionEvent::Validate)?,
        calculate: read(FieldActionEvent::Calculate)?,
    };
    Ok((!actions.is_empty()).then_some(actions))
}

/// A string read PDFium refused.
///
/// Every string on a field is optional in the file, so an absent one is
/// `Ok(None)` rather than an error; this is for the runtime declining to
/// answer, which is not a fact about the document.
fn text(
    result: Result<Option<String>, pdfium::PdfiumError>,
) -> Result<Option<String>, EngineError> {
    result.map_err(|e| {
        EngineError::with_message(
            crate::error::EngineErrorCode::RuntimeUnavailable,
            format!("reading a form field string failed: {e}"),
        )
    })
}
