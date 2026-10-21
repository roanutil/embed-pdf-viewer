//! Changing what a form says.
//!
//! G12. Ports the value half of
//! `services/src/features/forms/FormMutator.ts` (`setValue`, `setDisplay`,
//! `setAppearanceText`, `reset`, `repair`), its `internal/fieldValues.ts`,
//! `formMutationMeta.ts`, `signatureLocks.ts` and the fill stamps of
//! `fieldAttribution.ts`. The change ops of
//! [`crate::features::changes::forms`] run them.
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

use super::snapshot::{
    FormFieldDto, FormFieldFamily, FormFieldKind, FormFieldRef, FormValueEntry, FormWidgetRef,
    ToggleFieldWidget, read_field,
};
use super::value::FormFieldValue;
use crate::auth::scope::IdentityClaims;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::signature::types::SignedDocumentPolicy;
use crate::identity::{PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::session::document::WriteStamp;

/// Where in the model a caller's reference landed.
pub(crate) struct ResolvedField {
    pub(crate) index: u32,
    pub(crate) object_number: u32,
}

/// Resolve a reference against a model.
///
/// An object number goes through the model's reverse index. A name is
/// compared against every field's, which is linear and is what the
/// TypeScript does: a name reference exists FOR the fields that have no
/// object number, so there is no index to consult.
pub(crate) fn resolve(
    model: &FormModel<'_>,
    reference: &FormFieldRef,
) -> Result<ResolvedField, EngineError> {
    match reference {
        FormFieldRef::ObjectNumber { field } => model
            .field_index_by_object_number(*field)
            .map(|index| ResolvedField { index, object_number: *field })
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("form field not found: object {field}"),
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
                format!("form field not found: \"{name}\""),
            ))
        }
    }
}

/// A field stored as a direct object cannot be written.
///
/// PDFium's write API addresses a field by object number, and a direct
/// dictionary has none. The read side still reports such a field, by name,
/// because a document that contains one is not broken; it is only unwritable.
pub(super) fn require_writable(resolved: &ResolvedField) -> Result<u32, EngineError> {
    if resolved.object_number == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the form field is stored as a direct object and cannot be written",
        ));
    }
    Ok(resolved.object_number)
}

/// Resolve a reference, refuse a direct-object field, and read the field
/// through a fresh model: `resolveFieldRef`, `assertWritable`, `readFieldAt`
/// for a caller outside this module (the signature visual fill).
pub(crate) fn read_writable_field(
    document: &Document,
    reference: &FormFieldRef,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    require_unlocked(document, &model, &resolved)?;
    drop(model);
    read_field_back(document, object_number, cancel)
}

/// `FormMutator.assertNotLockedBySignature`: a field an earlier signature
/// froze (its FieldMDP, or the `/Lock` of a signed field) refuses every
/// write, `ProtectedDocument`. A document whose signature model cannot be
/// built is not known to be locked.
pub(super) fn require_unlocked(
    document: &Document,
    model: &FormModel<'_>,
    resolved: &ResolvedField,
) -> Result<(), EngineError> {
    use crate::features::signature::protection::field_lock_for;
    use crate::features::signature::types::FieldLockSource;
    let Ok(protection) = crate::features::signature::reader::read_protection(document) else {
        return Ok(());
    };
    if protection.field_locks.is_empty() {
        return Ok(());
    }
    let name = model
        .field(resolved.index)
        .and_then(|f| super::snapshot::text(f.name()).ok().flatten())
        .unwrap_or_default();
    if let Some(lock) = field_lock_for(&protection, &name) {
        return Err(EngineError::with_message(
            EngineErrorCode::ProtectedDocument,
            format!(
                "form field \"{name}\" is locked by signature {} ({})",
                lock.signature_index,
                if lock.source == FieldLockSource::Fieldmdp { "FieldMDP" } else { "/Lock" }
            ),
        ));
    }
    Ok(())
}

/// Load a model, or say the document has no form machinery at all.
pub(crate) fn load_model(document: &Document) -> Result<FormModel<'_>, EngineError> {
    document.form_model().ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the form model could not be loaded")
    })
}

/// Read a field back through a FRESH model.
///
/// Shared with the structural operations, which all end the same way: the
/// model that resolved the reference was a snapshot taken before the write.
pub(super) fn read_field_back(
    document: &Document,
    object_number: u32,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let index = model.field_index_by_object_number(object_number).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "the form field vanished after the write",
        )
    })?;
    let field = model.field(index).expect("the index resolved");
    read_field(&model, &field, index, &mut ActionReadBudgetTracker::default())
}

/// A form write's meta (`FormMutationMeta`): the page envelope plus what
/// changed, the fields written and every widget whose look changed, and of
/// those the ones drawn again. A field's widgets can sit on several pages,
/// so `changed_widgets` is what invalidates renders. An idempotent write
/// lists no widgets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormMutationMeta {
    #[serde(flatten)]
    pub meta: MutationMeta,
    pub changed_fields: Vec<FormFieldRef>,
    pub changed_widgets: Vec<MetaWidget>,
    /// The widgets of `changed_widgets` whose look was drawn again (a text
    /// or choice value, a new look). A checkbox or radio button whose value
    /// only switched its state isn't one, nor is a widget shown or hidden.
    pub redrawn_widgets: Vec<MetaWidget>,
}

/// A widget a form write's meta names. A write builds each with
/// `formWidget` (`{ref, objectNumber, page}`); an item that names every
/// widget of its field (`fieldResult`, `resetResult`) hands over the field's
/// own widget records as they are, so a checkbox's or radio button's carry
/// its `onState`, `exportValue` and `checked` too, as upstream's answer
/// does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetaWidget {
    Toggle(ToggleFieldWidget),
    Plain(FormWidgetRef),
}

impl MetaWidget {
    pub fn widget(&self) -> &FormWidgetRef {
        match self {
            Self::Toggle(toggle) => &toggle.widget,
            Self::Plain(widget) => widget,
        }
    }

    /// Every widget record of `field`, as the field holds them.
    pub fn of_field(field: &FormFieldDto) -> Vec<MetaWidget> {
        match &field.kind {
            FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
                widgets.iter().cloned().map(Self::Toggle).collect()
            }
            _ => field.widgets().copied().map(Self::Plain).collect(),
        }
    }
}

/// The meta of a form write (`formMutationMeta`): the fields and widgets it
/// changed, the ones of those it drew again (all of them unless `redrawn`
/// says fewer), and the pages those widgets sit on, once each in the order
/// they come. An unplaced widget names no page. Form writes never change
/// the page list, so there is no cache delta.
pub fn form_mutation_meta(
    stamp: &WriteStamp,
    changed_fields: Vec<FormFieldRef>,
    changed_widgets: Vec<MetaWidget>,
    redrawn: Option<Vec<MetaWidget>>,
) -> FormMutationMeta {
    let mut pages: Vec<PageRef> = Vec::new();
    for widget in changed_widgets.iter().map(MetaWidget::widget) {
        if let Ok(page) = PageObjectNumber::new(widget.page_object_number) {
            let page = PageRef::new(page);
            if !pages.contains(&page) {
                pages.push(page);
            }
        }
    }
    FormMutationMeta {
        meta: MutationMeta::local(pages, stamp),
        changed_fields,
        redrawn_widgets: redrawn.unwrap_or_else(|| changed_widgets.clone()),
        changed_widgets,
    }
}

/// The widgets of a field a value write draws again (`redrawnByValue`): a
/// checkbox's or radio group's value only switches each widget's state
/// between the pictures it has.
pub fn redrawn_by_value(family: FormFieldFamily, widgets: &[MetaWidget]) -> Vec<MetaWidget> {
    match family {
        FormFieldFamily::Checkbox | FormFieldFamily::Radio => Vec::new(),
        _ => widgets.to_vec(),
    }
}

/// Acrobat's `Field.display`, as the wire spells it (`FormFieldDisplay`,
/// `core/src/forms/effects.ts`).
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

/// One field written, read back, and the write's meta: `FormSetValueResult`
/// without its widget rows, which the change that ran it reads
/// (`FieldWriteResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldWriteResult {
    pub field: FormFieldDto,
    pub meta: FormMutationMeta,
}

/// The fields a reset changed, read back, and one meta for all of them
/// (`FormResetResult` without its widget rows). No fields: every field was
/// at its default already.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetResult {
    pub fields: Vec<FormFieldDto>,
    pub meta: FormMutationMeta,
}

/// The families that hold a value: the ones a reset puts back
/// (`VALUE_FAMILIES`).
pub fn holds_value(family: FormFieldFamily) -> bool {
    matches!(
        family,
        FormFieldFamily::Text
            | FormFieldFamily::Checkbox
            | FormFieldFamily::Radio
            | FormFieldFamily::Combobox
            | FormFieldFamily::Listbox
    )
}

/// The value half of `FormMutator`: each write resolves its field against a
/// fresh model, checks it can be written, writes what the field's rules
/// (`super::value`) leave it holding that it doesn't hold now, and reads it
/// back through another fresh model.
pub struct FormMutator<'a> {
    pub document: &'a Document,
    /// Whether the signatures' field locks hold (`signedDocumentPolicy:
    /// 'protect'`); under `'permit'` nothing is locked.
    pub signed_document_policy: SignedDocumentPolicy,
    /// What every meta carries: the write's `opId` and whether it can be
    /// undone.
    pub stamp: WriteStamp,
}

impl FormMutator<'_> {
    /// Set a field's value, as `actor` filled it in (`setValue`): what the
    /// field's rules (`fieldAfterValue`) leave it holding. A write that
    /// changes the field names who filled it in, at `now`; one that leaves
    /// it as it was writes nothing.
    pub fn set_value(
        &self,
        reference: &FormFieldRef,
        value: &FormFieldValue,
        actor: &IdentityClaims,
        now: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<FieldWriteResult, EngineError> {
        cancel.check()?;
        let model = load_model(self.document)?;
        let resolved = resolve(&model, reference)?;
        self.assert_writable(&model, &resolved)?;
        let before = read_at(&model, resolved.index)?;
        let after = super::value::field_after_value(&before, value)?;
        drop(model);
        let changed = self.write_field_after(resolved.object_number, &before, &after, false)?;
        if changed.is_some() {
            stamp_field_fill(self.document, resolved.object_number, actor, now)?;
        }
        self.read_back(resolved.object_number, &changed.unwrap_or_default(), None, cancel)
    }

    /// Show, hide or keep from printing every widget of a field, as a
    /// script's `Field.display` does (`setDisplay`): only the four flags it
    /// names change, and no picture is drawn.
    pub fn set_display(
        &self,
        reference: &FormFieldRef,
        display: FormFieldDisplay,
        cancel: &CancelToken,
    ) -> Result<FieldWriteResult, EngineError> {
        cancel.check()?;
        let model = load_model(self.document)?;
        let resolved = resolve(&model, reference)?;
        self.assert_writable(&model, &resolved)?;
        drop(model);
        let changed = self
            .document
            .set_field_display(resolved.object_number, display.native())
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the field could not be shown or hidden",
                )
            })?;
        self.read_back(resolved.object_number, &changed.object_numbers, Some(false), cancel)
    }

    /// Draw a text field's or dropdown's widgets with `text` (a format
    /// script's result) without changing its value (`setAppearanceText`).
    pub fn set_appearance_text(
        &self,
        reference: &FormFieldRef,
        text: &str,
        cancel: &CancelToken,
    ) -> Result<FieldWriteResult, EngineError> {
        cancel.check()?;
        let model = load_model(self.document)?;
        let resolved = resolve(&model, reference)?;
        self.assert_writable(&model, &resolved)?;
        let family = read_at(&model, resolved.index)?.base.family;
        drop(model);
        if !matches!(family, FormFieldFamily::Text | FormFieldFamily::Combobox) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "appearance text does not apply to a {} field",
                    serde_json::to_value(family)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default()
                ),
            ));
        }
        let changed = self
            .document
            .set_field_appearance_text(resolved.object_number, text)
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the appearance text could not be drawn",
                )
            })?;
        self.read_back(resolved.object_number, &changed.object_numbers, Some(true), cancel)
    }

    /// Put fields back to their default value (`reset`): the ones named, or,
    /// with `refs` absent, every field of the form. Fields that hold no
    /// value are skipped; so, in a whole-form reset, are fields stored
    /// inline or locked by a signature (named, they are refused). Every
    /// field is checked before the first write. Answers the fields that
    /// changed.
    pub fn reset(
        &self,
        refs: Option<&[FormFieldRef]>,
        cancel: &CancelToken,
    ) -> Result<ResetResult, EngineError> {
        cancel.check()?;
        let model = load_model(self.document)?;
        let targets = match refs {
            None => self.whole_form_targets(&model)?,
            Some(refs) => self.named_targets(&model, refs)?,
        };
        drop(model);
        cancel.check()?;

        // Each field as its rules (`fieldAfterReset`) put it back; one
        // already at its default is left alone.
        let mut written = Vec::new();
        for before in targets.into_iter().filter(|field| !super::value::is_at_default(field)) {
            let after = super::value::field_after_reset(&before)?;
            let object_number = before.base.field_object_number();
            let Some(changed) = self.write_field_after(object_number, &before, &after, false)?
            else {
                continue;
            };
            // Back at its default, the value was filled in by nobody.
            clear_field_fill(self.document, object_number)?;
            written.push((before, changed));
        }

        let mut fields = Vec::new();
        let mut changed_widgets = Vec::new();
        let mut redrawn_widgets = Vec::new();
        for (before, changed) in written {
            let after = read_field_back(self.document, before.base.field_object_number(), cancel)?;
            if changed.is_empty()
                && super::value::value_entries_equal(
                    &before.base.value_entry,
                    &after.base.value_entry,
                )
            {
                continue;
            }
            let own: Vec<MetaWidget> = after
                .widgets()
                .filter(|widget| changed.contains(&widget.annot_object_number))
                .copied()
                .map(MetaWidget::Plain)
                .collect();
            redrawn_widgets.extend(redrawn_by_value(after.base.family, &own));
            changed_widgets.extend(own);
            fields.push(after);
        }
        let changed_fields = fields.iter().map(|field| field.base.field_ref.clone()).collect();
        Ok(ResetResult {
            fields,
            meta: form_mutation_meta(
                &self.stamp,
                changed_fields,
                changed_widgets,
                Some(redrawn_widgets),
            ),
        })
    }

    /// Every field a whole-form reset puts back: those with a value, written
    /// directly, unlocked (`wholeFormTargets`).
    fn whole_form_targets(&self, model: &FormModel<'_>) -> Result<Vec<FormFieldDto>, EngineError> {
        let locks = self.field_locks();
        let mut targets = Vec::new();
        for index in 0..model.field_count() {
            let field = read_at(model, index)?;
            if !holds_value(field.base.family) || field.base.field_object_number() == 0 {
                continue;
            }
            if locks.as_ref().is_some_and(|locks| locks.lock_for(&field.base.name).is_some()) {
                continue;
            }
            targets.push(field);
        }
        Ok(targets)
    }

    /// The named fields with a value, each once; one that can't be written
    /// is refused (`namedTargets`).
    fn named_targets(
        &self,
        model: &FormModel<'_>,
        refs: &[FormFieldRef],
    ) -> Result<Vec<FormFieldDto>, EngineError> {
        let mut seen = Vec::new();
        let mut targets = Vec::new();
        for reference in refs {
            let resolved = resolve(model, reference)?;
            let field = read_at(model, resolved.index)?;
            if !holds_value(field.base.family) || seen.contains(&resolved.object_number) {
                continue;
            }
            self.assert_writable(model, &resolved)?;
            seen.push(resolved.object_number);
            targets.push(field);
        }
        Ok(targets)
    }

    /// `assertWritable`: a field stored as a direct object can't be written,
    /// and one an earlier signature locked refuses every write.
    fn assert_writable(
        &self,
        model: &FormModel<'_>,
        resolved: &ResolvedField,
    ) -> Result<(), EngineError> {
        if resolved.object_number == 0 {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "form field is stored as a direct object and cannot be written",
            ));
        }
        let Some(locks) = self.field_locks() else {
            return Ok(());
        };
        let name = model
            .field(resolved.index)
            .and_then(|f| super::snapshot::text(f.name()).ok().flatten())
            .unwrap_or_default();
        locks.assert_not_locked(&name)
    }

    /// `readFieldLocks`: the field locks earlier signatures declared, read
    /// once for a write; `None` when there are none, under `'permit'`, or
    /// for a document whose signature model cannot be built.
    fn field_locks(&self) -> Option<FieldLocks> {
        if self.signed_document_policy != SignedDocumentPolicy::Protect {
            return None;
        }
        let protection = crate::features::signature::reader::read_protection(self.document).ok()?;
        (!protection.field_locks.is_empty()).then_some(FieldLocks(protection))
    }

    /// Write what `after` holds that `before` doesn't (`writeFieldAfter`):
    /// `/V`; a checkbox's or radio group's widgets on or off; a choice
    /// field's selected options (`/I`); and a text or choice field's new
    /// pictures (also with `redraw`). Answers the widgets whose pictures
    /// changed, or `None` when the field already held it all and nothing
    /// was written.
    fn write_field_after(
        &self,
        object_number: u32,
        before: &FormFieldDto,
        after: &FormFieldDto,
        redraw: bool,
    ) -> Result<Option<Vec<u32>>, EngineError> {
        let toggles = match &after.kind {
            FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
                Some(widgets)
            }
            _ => None,
        };
        let value_changes =
            !super::value::value_entries_equal(&before.base.value_entry, &after.base.value_entry);
        let was_checked = |at: usize| match &before.kind {
            FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
                widgets.get(at).map(|widget| widget.checked)
            }
            _ => None,
        };
        let states_change = toggles.is_some_and(|widgets| {
            widgets.iter().enumerate().any(|(at, widget)| Some(widget.checked) != was_checked(at))
        });
        if !value_changes && !states_change && !redraw {
            return Ok(None);
        }

        let refused = || {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("'{}' could not take the value", after.base.name),
            )
        };
        if value_changes && !write_entry(self.document, object_number, &after.base.value_entry) {
            return Err(refused());
        }
        if let FormFieldKind::Combobox { options, .. } | FormFieldKind::Listbox { options, .. } =
            &after.kind
        {
            let selected: Vec<i32> = options
                .iter()
                .enumerate()
                .filter(|(_, option)| option.selected)
                .map(|(at, _)| at as i32)
                .collect();
            if !self.document.set_field_selected_indices(object_number, &selected) {
                return Err(refused());
            }
        }
        if toggles.is_some_and(|widgets| widgets.is_empty()) {
            return Ok(Some(Vec::new()));
        }
        let changed = match toggles {
            Some(widgets) => {
                let checked: Vec<bool> = widgets.iter().map(|widget| widget.checked).collect();
                self.document.set_field_widgets_checked(object_number, &checked)
            }
            None => self.document.redraw_field_widgets(object_number),
        };
        changed.map(|changed| Some(changed.object_numbers)).ok_or_else(refused)
    }

    /// Read the field back through a fresh model, and its changed widgets
    /// (`readBack`). `redraws`: whether the write drew the widgets again;
    /// `None` for a value write, which decides by the field's family.
    fn read_back(
        &self,
        object_number: u32,
        changed: &[u32],
        redraws: Option<bool>,
        cancel: &CancelToken,
    ) -> Result<FieldWriteResult, EngineError> {
        let field = read_field_back(self.document, object_number, cancel).map_err(|e| {
            if e.code == EngineErrorCode::Unknown {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "form field vanished after write",
                )
            } else {
                e
            }
        })?;
        let changed_widgets: Vec<MetaWidget> = field
            .widgets()
            .filter(|widget| changed.contains(&widget.annot_object_number))
            .copied()
            .map(MetaWidget::Plain)
            .collect();
        let redrawn = match redraws {
            None => redrawn_by_value(field.base.family, &changed_widgets),
            Some(true) => changed_widgets.clone(),
            Some(false) => Vec::new(),
        };
        let meta = form_mutation_meta(
            &self.stamp,
            vec![field.base.field_ref.clone()],
            changed_widgets,
            Some(redrawn),
        );
        Ok(FieldWriteResult { field, meta })
    }
}

/// The locks earlier signatures put on fields, by fully qualified name.
struct FieldLocks(crate::features::signature::types::DocumentProtection);

impl FieldLocks {
    fn lock_for(
        &self,
        name: &str,
    ) -> Option<&crate::features::signature::types::DocumentFieldLock> {
        crate::features::signature::protection::field_lock_for(&self.0, name)
    }

    /// `assertFieldNotLocked`: `ProtectedDocument` when a signature locked
    /// the field.
    fn assert_not_locked(&self, name: &str) -> Result<(), EngineError> {
        use crate::features::signature::types::FieldLockSource;
        let Some(lock) = self.lock_for(name) else {
            return Ok(());
        };
        Err(EngineError::with_message(
            EngineErrorCode::ProtectedDocument,
            format!(
                "form field \"{name}\" is locked by signature {} ({})",
                lock.signature_index,
                if lock.source == FieldLockSource::Fieldmdp { "FieldMDP" } else { "/Lock" }
            ),
        ))
    }
}

/// One field of a loaded model, read whole.
pub(crate) fn read_at(model: &FormModel<'_>, index: u32) -> Result<FormFieldDto, EngineError> {
    let field = model.field(index).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the field index did not resolve")
    })?;
    read_field(model, &field, index, &mut ActionReadBudgetTracker::default())
}

/// `writeFieldEntry(…, 'value')`: `/V` as `entry` holds it. `false` when
/// the fork refuses it, or for an entry no write can hold.
fn write_entry(document: &Document, object_number: u32, entry: &FormValueEntry) -> bool {
    let write = match entry {
        FormValueEntry::None => pdfium::FieldValueWrite::None,
        FormValueEntry::Scalar { value } => pdfium::FieldValueWrite::Scalar(value),
        FormValueEntry::Array { values } => pdfium::FieldValueWrite::Array(values),
        FormValueEntry::Unsupported => return false,
    };
    document.set_field_value(object_number, write)
}

/// `/EMBD_Metadata` keys of a field's attribution (`fieldAttribution.ts`).
const KEY_SCHEMA_VERSION: &str = "SchemaVersion";
const KEY_FILLED_BY: &str = "FilledBy";
const KEY_FILLED_BY_NAME: &str = "FilledByName";
const KEY_FILLED_AT: &str = "FilledAt";

/// `stampFieldFill`: a field whose value a write changed names who filled
/// it in, and when. An anonymous session clears the last filler instead.
fn stamp_field_fill(
    document: &Document,
    object_number: u32,
    actor: &IdentityClaims,
    now: Option<&str>,
) -> Result<(), EngineError> {
    let Some(user_id) = actor.user_id.as_deref() else {
        return clear_field_fill(document, object_number);
    };
    let failed = || attribution_failed(object_number);
    let wrote = document
        .set_field_embed_metadata_number(
            object_number,
            KEY_SCHEMA_VERSION,
            crate::features::annotation::write::primitives::EMBD_METADATA_SCHEMA_VERSION,
        )
        .map_err(|_| failed())?;
    if !wrote {
        return Err(failed());
    }
    for (key, value) in [
        (KEY_FILLED_BY, Some(user_id)),
        (KEY_FILLED_BY_NAME, actor.display_name.as_deref()),
        (KEY_FILLED_AT, now),
    ] {
        let wrote = match value {
            Some(value) => document.set_field_embed_metadata_string(object_number, key, value),
            None => document.clear_field_embed_metadata_key(object_number, key),
        };
        if !wrote.map_err(|_| failed())? {
            return Err(failed());
        }
    }
    Ok(())
}

/// `clearFieldFill`: who filled a field in is cleared; a reset put its value
/// back to its default.
fn clear_field_fill(document: &Document, object_number: u32) -> Result<(), EngineError> {
    for key in [KEY_FILLED_BY, KEY_FILLED_BY_NAME, KEY_FILLED_AT] {
        if !document
            .clear_field_embed_metadata_key(object_number, key)
            .map_err(|_| attribution_failed(object_number))?
        {
            return Err(attribution_failed(object_number));
        }
    }
    Ok(())
}

fn attribution_failed(object_number: u32) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::Unknown,
        format!("the metadata of field {object_number} could not be written"),
    )
}

/// What a repair pass found and fixed (`FormRepairResult`). Repair is
/// validate-then-apply and idempotent: with nothing to fix, every counter
/// is zero and the document is untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormRepairResult {
    pub acroform_created: bool,
    pub fields_linked: u32,
    pub widgets_linked: u32,
    pub fields_unrepairable: u32,
    pub appearances_baked: u32,
    /// `/NeedAppearances` was cleared after re-baking (7bccc4f35 spells it
    /// `needsAppearancesCleared`).
    pub needs_appearances_cleared: bool,
    /// No page and no cache delta: a repair changes no page list
    /// (`emptyMeta`).
    pub meta: MutationMeta,
}

/// Relink a form whose `/AcroForm` and pages disagree (`FormMutator.repair`),
/// stamped as the write `stamp` names.
pub fn repair(
    document: &Document,
    bake_appearances: bool,
    stamp: &WriteStamp,
    cancel: &CancelToken,
) -> Result<FormRepairResult, EngineError> {
    cancel.check()?;
    let report = document
        .repair_form(bake_appearances)
        .ok_or_else(|| EngineError::with_message(EngineErrorCode::Unknown, "form repair failed"))?;
    Ok(FormRepairResult {
        acroform_created: report.acroform_created,
        fields_linked: report.fields_linked,
        widgets_linked: report.widgets_linked,
        fields_unrepairable: report.fields_unrepairable,
        appearances_baked: report.appearances_baked,
        needs_appearances_cleared: report.need_appearances_cleared,
        meta: MutationMeta::local(Vec::new(), stamp),
    })
}

/// Which serialization a form import or export speaks.
///
/// A wire enum over [`pdfium::FormDataFormat`], on the same terms as
/// [`crate::features::save::SaveMode`]: the `pdfium` crate is the FFI
/// boundary and carries no serde, so the spellings that cross the wire live
/// here. They are `FormDataFormat` in `core/src/forms/value.ts`, lower case,
/// and a caller that asked for one gets the same string back beside the
/// bytes so it knows which extension to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormDataFormat {
    Fdf,
    Xfdf,
}

impl FormDataFormat {
    pub fn native(self) -> pdfium::FormDataFormat {
        match self {
            Self::Fdf => pdfium::FormDataFormat::Fdf,
            Self::Xfdf => pdfium::FormDataFormat::Xfdf,
        }
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn the_wire_names_are_the_typescript_ones() {
        // Two strings that cross the wire, so a rename here is a protocol
        // change rather than a refactor.
        assert_eq!(serde_json::to_value(FormDataFormat::Fdf).unwrap(), "fdf");
        assert_eq!(serde_json::to_value(FormDataFormat::Xfdf).unwrap(), "xfdf");
    }
}
