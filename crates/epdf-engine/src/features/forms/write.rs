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

/// What an import did, and what the form looks like afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormImportResult {
    /// Fields the payload named.
    pub fields_total: u32,
    /// Fields it named that the document has and that took the value.
    pub fields_applied: u32,
    /// Fields it named that the document does not have, or that refused.
    /// A skip is not a failure: an FDF exported from one form and applied
    /// to another legitimately names fields the second one lacks.
    pub fields_skipped: u32,
    pub widgets_changed: u32,
    /// The whole form, read back. An import can touch any number of
    /// fields, so a caller has no useful subset to ask for.
    pub snapshot: super::snapshot::FormSnapshot,
}

/// What a repair pass found and fixed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormRepairResult {
    pub acroform_created: bool,
    pub fields_linked: u32,
    pub widgets_linked: u32,
    pub fields_unrepairable: u32,
    pub appearances_baked: u32,
    pub need_appearances_cleared: bool,
}

/// Apply an FDF or XFDF payload.
///
/// The format is SNIFFED when the caller does not name one, because a
/// declared format that disagrees with the bytes sends the payload to the
/// wrong parser, and both parsers answer a bad payload the same way.
pub fn import_data(
    document: &Document,
    bytes: &[u8],
    format: Option<pdfium::FormDataFormat>,
    cancel: &CancelToken,
) -> Result<FormImportResult, EngineError> {
    cancel.check()?;
    if bytes.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the form data payload is empty",
        ));
    }
    let format = format.unwrap_or_else(|| sniff_format(bytes));
    let counts = match format {
        pdfium::FormDataFormat::Fdf => document.import_fdf(bytes),
        pdfium::FormDataFormat::Xfdf => document.import_xfdf(bytes),
    }
    .ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            match format {
                pdfium::FormDataFormat::Fdf => "the payload is not valid FDF",
                pdfium::FormDataFormat::Xfdf => "the payload is not valid XFDF",
            },
        )
    })?;

    cancel.check()?;
    let model = load_model(document)?;
    Ok(FormImportResult {
        fields_total: counts.fields_total,
        fields_applied: counts.fields_applied,
        fields_skipped: counts.fields_skipped,
        widgets_changed: counts.widgets_changed,
        snapshot: super::snapshot::read_form_snapshot(&model, cancel)?,
    })
}

/// Which parser the bytes want.
///
/// XFDF is XML and FDF is a PDF-shaped file, so the first character that is
/// not whitespace or a byte-order mark decides it. A payload that is neither
/// goes to the FDF parser, which is the one that will say so.
fn sniff_format(bytes: &[u8]) -> pdfium::FormDataFormat {
    for byte in bytes.iter().take(64) {
        match byte {
            // A UTF-8 byte-order mark, which an XFDF exporter may write.
            0xEF | 0xBB | 0xBF => continue,
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'<' => return pdfium::FormDataFormat::Xfdf,
            _ => return pdfium::FormDataFormat::Fdf,
        }
    }
    pdfium::FormDataFormat::Fdf
}

/// Relink a form whose `/AcroForm` and pages disagree.
pub fn repair(
    document: &Document,
    bake_appearances: bool,
    cancel: &CancelToken,
) -> Result<FormRepairResult, EngineError> {
    cancel.check()?;
    let report = document.repair_form(bake_appearances).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the form repair failed")
    })?;
    Ok(FormRepairResult {
        acroform_created: report.acroform_created,
        fields_linked: report.fields_linked,
        widgets_linked: report.widgets_linked,
        fields_unrepairable: report.fields_unrepairable,
        appearances_baked: report.appearances_baked,
        need_appearances_cleared: report.need_appearances_cleared,
    })
}
