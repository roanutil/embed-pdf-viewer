//! An ordered batch of form effects from one client script run.
//!
//! G12. Ports `services/src/features/forms/FormsEffectsApplier.ts`.
//! Operation: `forms.applyEffects`.
//!
//! # Ordered, and not atomic
//!
//! A form script runs in the client, decides what should change, and sends
//! the decisions here as a list. Applying them is NOT a transaction: each
//! effect lands or does not, in order, and an effect that fails stops the
//! ones after it rather than unwinding the ones before it.
//!
//! That is deliberate and it is the only honest shape available. There is no
//! rollback beneath this: the writes go through PDFium, which changes the
//! document in place. A batch that pretended to be atomic would have to undo
//! writes by writing again, and a failure in the undo would leave the
//! document in a state nobody had reasoned about. So the result reports what
//! happened to each effect, one by one, and the caller reconciles.
//!
//! # The five outcomes
//!
//! - `Applied`: it was written and something changed.
//! - `Unchanged`: it was valid and the field was already that way. No write,
//!   no version bump. This is the common case for a script that recomputes
//!   every field on every keystroke.
//! - `Rejected`: preflight refused it, and NOTHING was written for it. Safe
//!   to report to a user as their mistake.
//! - `Failed`: a write was attempted and its outcome is not known. The
//!   session is finalized and the batch stops.
//! - `Skipped`: an earlier effect stopped the batch before this one ran.
//!
//! The difference between `Rejected` and `Failed` is the whole reason for
//! the vocabulary: one means "your request was wrong", the other means "the
//! document may have moved and you should refetch".
//!
//! # Why preflight happens up front for every effect
//!
//! Resolving and validating all of them BEFORE writing any of them means a
//! batch whose third effect is nonsense is caught with two writes done and
//! reported precisely, rather than caught after three writes. It cannot be
//! caught before any: resolving needs the model, and the model is what the
//! earlier writes change. So each effect is re-resolved at apply time too,
//! and a field that disappeared between the two is the one case where the
//! `Rejected`/`Failed` split turns on whether anything has landed yet.

use pdfium::Document;
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::mutation::MutationMeta;

use super::snapshot::{
    FormFieldDto, FormFieldFamily, FormFieldKind, FormFieldRef, FormValueEntry, FormWidgetRef,
    read_field,
};
use super::write::{FormFieldValue, load_model, require_writable, resolve};

/// Acrobat's `Field.display`, as the wire spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldDisplay {
    Visible,
    Hidden,
    NoPrint,
    NoView,
}

impl FormFieldDisplay {
    fn native(self) -> pdfium::FieldDisplay {
        match self {
            Self::Visible => pdfium::FieldDisplay::Visible,
            Self::Hidden => pdfium::FieldDisplay::Hidden,
            Self::NoPrint => pdfium::FieldDisplay::NoPrint,
            Self::NoView => pdfium::FieldDisplay::NoView,
        }
    }
}

/// One decision a script made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum FormEffect {
    SetValue {
        #[serde(rename = "ref")]
        field_ref: FormFieldRef,
        value: FormFieldValue,
    },
    SetDisplay {
        #[serde(rename = "ref")]
        field_ref: FormFieldRef,
        display: FormFieldDisplay,
    },
    /// Redraw the widgets with this text, WITHOUT changing `/V`.
    ///
    /// The sink for an `/AA /F` format script. A number stored as `1234.5`
    /// and shown as `$1,234.50` is one field, and writing the formatted
    /// string into `/V` would make the next calculation read a currency
    /// symbol as a number.
    SetAppearanceText {
        #[serde(rename = "ref")]
        field_ref: FormFieldRef,
        text: String,
    },
    /// Several fields at once, because a reset button names a list.
    Reset {
        refs: Vec<FormFieldRef>,
    },
}

/// What happened to one effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormEffectStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
    Skipped,
}

/// One effect's outcome, with the fields it touched read back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormEffectResult {
    /// Position in the request, so a caller can line results up with what it
    /// sent without matching on content.
    pub index: u32,
    pub status: FormEffectStatus,
    /// The terminal fields this effect named, re-read. Empty when the read
    /// itself failed, which does not make the write un-happen.
    pub fields: Vec<FormFieldDto>,
    pub changed_widgets: Vec<FormWidgetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// What a whole batch did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormEffectsResult {
    pub results: Vec<FormEffectResult>,
    /// Every widget any effect changed, deduplicated across the batch.
    pub changed_widgets: Vec<FormWidgetRef>,
    /// `None` when nothing was written and nothing failed indeterminately.
    ///
    /// A batch that was entirely no-ops or entirely rejected produces no
    /// artifact, no event and no version bump: nothing happened, and saying
    /// otherwise would invalidate every cache downstream for a script run
    /// that changed nothing.
    pub meta: Option<MutationMeta>,
}

fn empty(index: u32, status: FormEffectStatus) -> FormEffectResult {
    FormEffectResult {
        index,
        status,
        fields: Vec::new(),
        changed_widgets: Vec::new(),
        error: None,
    }
}

/// What preflight worked out about one effect.
struct Preflight {
    field_object_numbers: Vec<u32>,
    fields: Vec<FormFieldDto>,
    error: Option<EngineError>,
}

/// Apply a batch.
///
/// Never returns `Err` for an individual effect's failure: those are
/// statuses. It returns `Err` only for cancellation, because a cancelled
/// batch has no meaningful per-effect report.
pub fn apply(
    document: &Document,
    effects: &[FormEffect],
    cancel: &CancelToken,
) -> Result<FormEffectsResult, EngineError> {
    cancel.check()?;

    let mut budget = ActionReadBudgetTracker::default();
    let preflighted: Vec<Preflight> =
        effects.iter().map(|effect| preflight(document, effect, &mut budget)).collect();

    let mut results = Vec::with_capacity(effects.len());
    // Keyed on page and annotation together, because an object number is
    // unique per document and the pair is what a caller matches on.
    let mut all_changed: std::collections::BTreeMap<(u32, u32), FormWidgetRef> =
        std::collections::BTreeMap::new();
    let mut must_finalize = false;
    let mut stop = false;
    let mut budget = ActionReadBudgetTracker::default();

    for (position, (effect, item)) in effects.iter().zip(&preflighted).enumerate() {
        let index = position as u32;
        if stop {
            results.push(empty(index, FormEffectStatus::Skipped));
            continue;
        }
        cancel.check()?;

        if let Some(error) = &item.error {
            results.push(FormEffectResult {
                fields: item.fields.clone(),
                error: Some(error.to_wire()),
                ..empty(index, FormEffectStatus::Rejected)
            });
            continue;
        }

        // Re-read through a fresh model: an earlier effect in this batch may
        // have changed the field, and the preflight snapshot is from before
        // any of them ran.
        let before = match read_fields(document, &item.field_object_numbers, &mut budget) {
            Ok(fields) => fields,
            Err(error) => {
                // No write has happened for THIS effect. A field that went
                // missing between preflight and here is the caller's
                // mistake unless an earlier effect already landed, in which
                // case this batch has moved the document and the caller has
                // to refetch rather than correct anything.
                let status = if must_finalize {
                    stop = true;
                    FormEffectStatus::Failed
                } else {
                    FormEffectStatus::Rejected
                };
                results.push(FormEffectResult {
                    error: Some(error.to_wire()),
                    ..empty(index, status)
                });
                continue;
            }
        };

        if is_no_op(effect, &before) {
            results.push(FormEffectResult {
                fields: before,
                ..empty(index, FormEffectStatus::Unchanged)
            });
            continue;
        }

        let written = apply_native(document, effect, &item.field_object_numbers);
        let after = read_fields_best_effort(document, &item.field_object_numbers, &mut budget);
        let changed = widget_refs(&written.changed, &before, &after);

        if !written.ok {
            // PDFium's false says the call did not succeed and says nothing
            // about whether it wrote anything first. Indeterminate, so the
            // session is finalized and the batch stops: compounding further
            // writes onto a document in an unknown state makes recovery
            // harder, not easier.
            must_finalize = true;
            remember(&mut all_changed, &changed);
            results.push(FormEffectResult {
                index,
                status: FormEffectStatus::Failed,
                fields: after,
                changed_widgets: changed,
                error: Some(
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "a form effect failed in the engine after preflight accepted it",
                    )
                    .to_wire(),
                ),
            });
            stop = true;
            continue;
        }

        if !changed_state(effect, &before, &written.changed) {
            // Accepted and wrote nothing. Reported as unchanged rather than
            // applied, so a caller does not invalidate a cache over it.
            results.push(FormEffectResult {
                fields: after,
                ..empty(index, FormEffectStatus::Unchanged)
            });
            continue;
        }

        must_finalize = true;
        remember(&mut all_changed, &changed);
        results.push(FormEffectResult {
            index,
            status: FormEffectStatus::Applied,
            fields: after,
            changed_widgets: changed,
            error: None,
        });
    }

    Ok(FormEffectsResult {
        results,
        changed_widgets: all_changed.into_values().collect(),
        meta: must_finalize
            .then(|| MutationMeta { affected_pages: Vec::new(), cache_delta: None }),
    })
}

/// Resolve and validate one effect without writing anything.
fn preflight(
    document: &Document,
    effect: &FormEffect,
    budget: &mut ActionReadBudgetTracker,
) -> Preflight {
    match preflight_inner(document, effect, budget) {
        Ok((field_object_numbers, fields)) => {
            Preflight { field_object_numbers, fields, error: None }
        }
        Err(error) => {
            Preflight { field_object_numbers: Vec::new(), fields: Vec::new(), error: Some(error) }
        }
    }
}

fn preflight_inner(
    document: &Document,
    effect: &FormEffect,
    budget: &mut ActionReadBudgetTracker,
) -> Result<(Vec<u32>, Vec<FormFieldDto>), EngineError> {
    let refs: Vec<&FormFieldRef> = match effect {
        FormEffect::Reset { refs } => refs.iter().collect(),
        FormEffect::SetValue { field_ref, .. }
        | FormEffect::SetDisplay { field_ref, .. }
        | FormEffect::SetAppearanceText { field_ref, .. } => vec![field_ref],
    };
    if refs.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a reset effect names no fields",
        ));
    }

    let model = load_model(document)?;
    let mut object_numbers = Vec::with_capacity(refs.len());
    let mut fields = Vec::with_capacity(refs.len());
    for reference in refs {
        let resolved = resolve(&model, reference)?;
        object_numbers.push(require_writable(&resolved)?);
        let field = model.field(resolved.index).expect("the index resolved");
        fields.push(read_field(&model, &field, resolved.index, budget)?);
    }
    validate(effect, &fields)?;
    Ok((object_numbers, fields))
}

/// Everything refusable before a write.
fn validate(effect: &FormEffect, fields: &[FormFieldDto]) -> Result<(), EngineError> {
    match effect {
        FormEffect::Reset { .. } => {
            for field in fields {
                // Neither has a value to go back to. A push button has no
                // `/V` at all, and a signature's is a dictionary that a
                // reset has no business touching.
                if matches!(
                    field.base.family,
                    FormFieldFamily::Pushbutton | FormFieldFamily::Signature
                ) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!("a {:?} field cannot be reset", field.base.family),
                    ));
                }
            }
            Ok(())
        }
        FormEffect::SetAppearanceText { .. } => {
            let field = &fields[0];
            if !matches!(
                field.base.family,
                FormFieldFamily::Text | FormFieldFamily::Combobox
            ) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "appearance text does not apply to a {:?} field",
                        field.base.family
                    ),
                ));
            }
            Ok(())
        }
        // Every family has widgets and every widget has display flags, so
        // there is nothing here a field could refuse.
        FormEffect::SetDisplay { .. } => Ok(()),
        FormEffect::SetValue { value, .. } => validate_value(&fields[0], value),
    }
}

/// Whether this value could be this field's.
///
/// Checked HERE rather than left to PDFium, for the reason
/// [`super::write::set_value`] gives: PDFium refuses a wrong-family value
/// with the same code it uses for a value the field could have held, which
/// tells a caller nothing about which mistake they made.
fn validate_value(field: &FormFieldDto, value: &FormFieldValue) -> Result<(), EngineError> {
    let refuse = |message: String| {
        Err(EngineError::with_message(EngineErrorCode::InvalidArg, message))
    };
    let mismatch = || {
        refuse(format!(
            "a '{}' value does not apply to a {:?} field",
            value.name(),
            field.base.family
        ))
    };

    match (value, &field.kind) {
        (FormFieldValue::Text { .. }, FormFieldKind::Text { .. }) => Ok(()),
        (FormFieldValue::Toggle { state }, FormFieldKind::Checkbox { widgets, .. })
        | (FormFieldValue::Toggle { state }, FormFieldKind::Radio { widgets, .. }) => {
            // A state that no widget offers would be written and then read
            // back as nothing, which looks like the write having silently
            // done the wrong thing.
            if let Some(state) = state
                && !widgets.iter().any(|w| &w.on_state == state)
            {
                return refuse(format!("no widget of this field has the state '{state}'"));
            }
            Ok(())
        }
        (FormFieldValue::Choice { values }, FormFieldKind::Combobox { edit, options, .. }) => {
            check_choice(values, options, *edit, true, false)
        }
        (
            FormFieldValue::Choice { values },
            FormFieldKind::Listbox { multi_select, options, .. },
        ) => check_choice(values, options, false, false, *multi_select),
        _ => mismatch(),
    }
}

fn check_choice(
    values: &[String],
    options: &[super::snapshot::FormFieldOption],
    edit: bool,
    combo: bool,
    multi_select: bool,
) -> Result<(), EngineError> {
    let refuse = |message: &str| {
        Err(EngineError::with_message(EngineErrorCode::InvalidArg, message.to_owned()))
    };
    let unique: std::collections::BTreeSet<&String> = values.iter().collect();
    if unique.len() != values.len() {
        return refuse("a choice value is repeated");
    }
    if combo && values.len() > 1 {
        return refuse("a combo box holds at most one value");
    }
    // Unexercised by the conformance corpus, and kept anyway.
    // `form_fields.pdf` has one list box and it is multi-select, so no
    // vector can reach this branch: removing it would pass every suite and
    // then let a single-select list box hold two selections, which `/V` can
    // represent and no widget can display.
    if !combo && !multi_select && values.len() > 1 {
        return refuse("a single-select list box holds one value");
    }
    // An editable combo box accepts one value that is not an option, which
    // is exactly what "editable" means. Everything else must name an option:
    // a value outside the list is unreachable through the widget and would
    // read back as a selection the user cannot see.
    let free_text = combo && edit && values.len() == 1;
    if !free_text {
        let known: std::collections::BTreeSet<&str> =
            options.iter().map(|o| o.value.as_str()).collect();
        if values.iter().any(|v| !known.contains(v.as_str())) {
            return refuse("a choice value is not one of the field's options");
        }
    }
    Ok(())
}

/// What one native call reported.
struct Written {
    ok: bool,
    changed: Vec<u32>,
}

fn apply_native(
    document: &Document,
    effect: &FormEffect,
    field_object_numbers: &[u32],
) -> Written {
    match effect {
        FormEffect::Reset { .. } => {
            // Each field separately, and a failure stops the rest: a reset
            // of ten fields that fails on the fourth has reset three, and
            // pressing on would widen the indeterminate window for nothing.
            let mut changed = Vec::new();
            for object_number in field_object_numbers {
                match document.reset_field(*object_number) {
                    Some(widgets) => changed.extend(widgets.object_numbers),
                    None => return Written { ok: false, changed },
                }
            }
            Written { ok: true, changed }
        }
        FormEffect::SetDisplay { display, .. } => {
            native(document.set_field_display(field_object_numbers[0], display.native()))
        }
        FormEffect::SetAppearanceText { text, .. } => {
            native(document.set_field_appearance_text(field_object_numbers[0], text))
        }
        FormEffect::SetValue { value, .. } => {
            let object_number = field_object_numbers[0];
            match value {
                FormFieldValue::Text { value } => {
                    native(document.set_field_text_value(object_number, value))
                }
                FormFieldValue::Toggle { state } => native(
                    document
                        .set_field_toggle(object_number, state.as_deref().unwrap_or(""))
                        .unwrap_or(None),
                ),
                FormFieldValue::Choice { values } => {
                    native(document.set_field_choice_values(object_number, values))
                }
            }
        }
    }
}

fn native(changed: Option<pdfium::ChangedWidgets>) -> Written {
    match changed {
        Some(widgets) => Written { ok: true, changed: widgets.object_numbers },
        None => Written { ok: false, changed: Vec::new() },
    }
}

/// Whether the field is already the way this effect would leave it.
///
/// Checked before the write, not after, because the point is to avoid the
/// write: a script that recomputes every field on every keystroke sends the
/// same values over and over, and writing each one would bump the version on
/// every character typed.
fn is_no_op(effect: &FormEffect, fields: &[FormFieldDto]) -> bool {
    match effect {
        // Both redraw widgets. A field with none has nothing to redraw, and
        // the value is untouched either way.
        FormEffect::SetAppearanceText { .. } | FormEffect::SetDisplay { .. } => {
            fields[0].widgets().next().is_none()
        }
        FormEffect::Reset { .. } => fields
            .iter()
            .all(|field| entries_equal(&field.base.value_entry, &field.base.default_value_entry)),
        FormEffect::SetValue { value, .. } => match (value, &fields[0].kind) {
            (FormFieldValue::Text { value }, FormFieldKind::Text { value: current, .. }) => {
                current == value
            }
            (FormFieldValue::Toggle { state }, FormFieldKind::Checkbox { widgets, .. })
            | (FormFieldValue::Toggle { state }, FormFieldKind::Radio { widgets, .. }) => widgets
                .iter()
                .all(|w| w.checked == (Some(&w.on_state) == state.as_ref())),
            (FormFieldValue::Choice { values }, FormFieldKind::Combobox { value, .. }) => {
                match values.first() {
                    None => value.is_empty(),
                    Some(wanted) => value == wanted,
                }
            }
            (
                FormFieldValue::Choice { values },
                FormFieldKind::Listbox { selected_values, .. },
            ) => selected_values == values,
            _ => false,
        },
    }
}

/// Whether the write actually moved anything.
///
/// The two appearance effects are judged ENTIRELY by what PDFium reported,
/// because neither changes a value: their whole footprint is the widgets
/// they redrew, and an empty list means they redrew nothing.
fn changed_state(effect: &FormEffect, before: &[FormFieldDto], changed: &[u32]) -> bool {
    match effect {
        FormEffect::SetDisplay { .. } | FormEffect::SetAppearanceText { .. } => {
            !changed.is_empty()
        }
        _ => !is_no_op(effect, before) || !changed.is_empty(),
    }
}

fn entries_equal(left: &FormValueEntry, right: &FormValueEntry) -> bool {
    match (left, right) {
        (FormValueEntry::Scalar { value: a }, FormValueEntry::Scalar { value: b }) => a == b,
        (FormValueEntry::Array { values: a }, FormValueEntry::Array { values: b }) => a == b,
        (FormValueEntry::None, FormValueEntry::None) => true,
        (FormValueEntry::Unsupported, FormValueEntry::Unsupported) => true,
        _ => false,
    }
}

/// Re-read every field an effect names, failing if any is gone.
fn read_fields(
    document: &Document,
    object_numbers: &[u32],
    budget: &mut ActionReadBudgetTracker,
) -> Result<Vec<FormFieldDto>, EngineError> {
    let model = load_model(document)?;
    let mut fields = Vec::with_capacity(object_numbers.len());
    for object_number in object_numbers {
        let index = model.field_index_by_object_number(*object_number).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidReference,
                format!("form field {object_number} is no longer in the model"),
            )
        })?;
        let field = model.field(index).expect("the index resolved");
        fields.push(read_field(&model, &field, index, budget)?);
    }
    Ok(fields)
}

/// The same, after a write, where a failure must not throw.
///
/// The status already says the effect failed. What must still reach the
/// caller is whatever DID land, and an error here would throw that away.
fn read_fields_best_effort(
    document: &Document,
    object_numbers: &[u32],
    budget: &mut ActionReadBudgetTracker,
) -> Vec<FormFieldDto> {
    read_fields(document, object_numbers, budget).unwrap_or_default()
}

/// Turn PDFium's changed object numbers into widget references.
///
/// Looked up across BOTH the before and after readings, because a widget an
/// effect detached is in one and not the other, and a caller that has to
/// invalidate it still needs to know which page it was on.
fn widget_refs(
    object_numbers: &[u32],
    before: &[FormFieldDto],
    after: &[FormFieldDto],
) -> Vec<FormWidgetRef> {
    let mut known: std::collections::BTreeMap<u32, FormWidgetRef> =
        std::collections::BTreeMap::new();
    for field in before.iter().chain(after) {
        for widget in field.widgets() {
            known.insert(widget.annot_object_number, *widget);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    object_numbers
        .iter()
        .filter(|n| seen.insert(**n))
        .filter_map(|n| known.get(n).copied())
        .collect()
}

fn remember(
    target: &mut std::collections::BTreeMap<(u32, u32), FormWidgetRef>,
    widgets: &[FormWidgetRef],
) {
    for widget in widgets {
        target.insert((widget.page_object_number, widget.annot_object_number), *widget);
    }
}
