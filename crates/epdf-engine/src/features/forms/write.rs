//! Changing what a form says.
//!
//! G12. Ports the value half of
//! `services/src/features/forms/FormMutator.ts`: `forms.setValue` and
//! `forms.reset`.
//!
//! # The model is a snapshot and a write invalidates it
//!
//! Every operation here loads a model, resolves the caller's reference
//! against it, writes through the DOCUMENT, and then loads a SECOND model to
//! read the result back. The second load is not caution: `EPDFForm_LoadModel`
//! walks the field tree once and the handle is correct forever and stale
//! after any mutation, so reading the field back through the model that
//! resolved it would report what the field used to be.
//!
//! # A value write is not structural
//!
//! No page's annotation list changes and no index moves, so nothing here
//! bumps a revision or asks a client to re-read a page. What it does report
//! is the set of WIDGETS whose appearance changed, which is a different
//! question: a caller holding rasters keyed by annotation needs that set, and
//! it cannot derive it from the value. A radio group changes two widgets for
//! one write, the one turning on and the one turning off.

use serde::{Deserialize, Serialize};

use pdfium::{Document, FormModel};

use super::snapshot::{FormFieldDto, FormFieldFamily, FormFieldRef, FormWidgetRef, read_field};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;

/// What a caller is setting a field to.
///
/// Three shapes rather than one string, because the three families store
/// different things: a toggle addresses a widget by its appearance state and
/// a list box holds several values at once. A shape that does not apply to
/// the field is refused rather than coerced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum FormFieldValue {
    Text {
        value: String,
    },
    /// The `/AP /N` key to turn on, or `null` to clear the group. Clearing
    /// is not the same as naming `Off`: one removes `/V` and the other
    /// selects the off appearance.
    Toggle {
        state: Option<String>,
    },
    Choice {
        values: Vec<String>,
    },
}

impl FormFieldValue {
    /// The families this shape applies to.
    fn families(&self) -> &'static [FormFieldFamily] {
        match self {
            Self::Text { .. } => &[FormFieldFamily::Text],
            Self::Toggle { .. } => &[FormFieldFamily::Checkbox, FormFieldFamily::Radio],
            Self::Choice { .. } => &[FormFieldFamily::Combobox, FormFieldFamily::Listbox],
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Toggle { .. } => "toggle",
            Self::Choice { .. } => "choice",
        }
    }
}

/// What a value write did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormSetValueResult {
    /// The field, read back through a fresh model.
    pub field: FormFieldDto,
    /// The widgets whose appearance the write re-baked, as annotation
    /// references a caller can key a cache by.
    pub changed_widgets: Vec<FormWidgetRef>,
}

/// Where in the model a caller's reference landed.
struct ResolvedField {
    index: u32,
    object_number: u32,
}

/// Resolve a reference against a model.
///
/// An object number goes through the model's reverse index. A name is
/// compared against every field's, which is linear and is what the
/// TypeScript does: a name reference exists FOR the fields that have no
/// object number, so there is no index to consult.
fn resolve(model: &FormModel<'_>, reference: &FormFieldRef) -> Result<ResolvedField, EngineError> {
    match reference {
        FormFieldRef::ObjectNumber { field } => model
            .field_index_by_object_number(*field)
            .map(|index| ResolvedField { index, object_number: *field })
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("no form field with object number {field}"),
                )
            }),
        FormFieldRef::Fqn { name } => {
            for index in 0..model.field_count() {
                let field = model.field(index).expect("the index came from the count");
                if super::snapshot::text(field.name())?.as_deref() == Some(name.as_str()) {
                    return Ok(ResolvedField { index, object_number: field.object_number() });
                }
            }
            Err(EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no form field named '{name}'"),
            ))
        }
    }
}

/// A field stored as a direct object cannot be written.
///
/// PDFium's write API addresses a field by object number, and a direct
/// dictionary has none. The read side still reports such a field, by name,
/// because a document that contains one is not broken; it is only unwritable.
fn require_writable(resolved: &ResolvedField) -> Result<u32, EngineError> {
    if resolved.object_number == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the form field is stored as a direct object and cannot be written",
        ));
    }
    Ok(resolved.object_number)
}

/// Set a field's value.
pub fn set_value(
    document: &Document,
    reference: &FormFieldRef,
    value: &FormFieldValue,
    cancel: &CancelToken,
) -> Result<FormSetValueResult, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;

    // The family check happens HERE rather than being left to PDFium. A
    // toggle state on a text field is the caller having confused two fields,
    // and PDFium would refuse it with the same code it uses for a value the
    // field could have held, which tells a caller nothing about which.
    let family = FormFieldFamily::from_code(
        model.field(resolved.index).expect("the index resolved").family_code(),
    );
    if !value.families().contains(&family) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a '{}' value does not apply to a {family:?} field", value.name()),
        ));
    }
    // The model goes before the write. It is a snapshot taken before it,
    // and holding one across a mutation is how a later read reports what
    // the field used to be.
    drop(model);
    cancel.check()?;

    let changed = match value {
        FormFieldValue::Text { value } => document.set_field_text_value(object_number, value),
        // An absent state is the empty string, which is the C API's null and
        // clears the group.
        //
        // Not observable on this fixture: PDFium writes `/V /Off` for an
        // empty state on a checkbox, which is the same entry naming `Off`
        // produces, so `checkbox-off` records the same value either way.
        // The empty string is still what this sends, because the two mean
        // different things to the API even where they agree in the file:
        // one says "no state" and the other names one.
        FormFieldValue::Toggle { state } => document
            .set_field_toggle(object_number, state.as_deref().unwrap_or(""))
            .map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("the toggle state is not a name a PDF can hold: {e}"),
                )
            })?,
        FormFieldValue::Choice { values } => {
            document.set_field_choice_values(object_number, values)
        }
    }
    .ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the form field refused the value: an unknown toggle state, a string past \
             its maximum length, or a choice that is not one of its options",
        )
    })?;

    read_back(document, object_number, &changed, cancel)
}

/// Put a field back to its default value.
pub fn reset(
    document: &Document,
    reference: &FormFieldRef,
    cancel: &CancelToken,
) -> Result<FormSetValueResult, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    drop(model);
    cancel.check()?;

    let changed = document.reset_field(object_number).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "the form field cannot be reset")
    })?;
    read_back(document, object_number, &changed, cancel)
}

/// Load a model, or say the document has no form machinery at all.
fn load_model(document: &Document) -> Result<FormModel<'_>, EngineError> {
    document.form_model().ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the form model could not be loaded")
    })
}

/// Read the field back through a FRESH model, and turn the changed object
/// numbers into references a caller can use.
///
/// The changed list comes back as bare object numbers, and a caller keying a
/// cache needs the page too. Joining them against the field's own widgets is
/// what supplies it, and it also drops any number that is no longer one of
/// the field's widgets, which is the honest answer for a reference that
/// cannot be resolved.
fn read_back(
    document: &Document,
    object_number: u32,
    changed: &pdfium::ChangedWidgets,
    cancel: &CancelToken,
) -> Result<FormSetValueResult, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let index = model.field_index_by_object_number(object_number).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "the form field vanished after the write",
        )
    })?;
    let field = model.field(index).expect("the index resolved");
    let dto = read_field(&model, &field, index, &mut ActionReadBudgetTracker::default())?;
    let changed_widgets = dto
        .widgets()
        .filter(|widget| changed.object_numbers.contains(&widget.annot_object_number))
        .cloned()
        .collect();
    Ok(FormSetValueResult { field: dto, changed_widgets })
}
