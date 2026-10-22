//! The form value ops and their reverse
//! (`services/src/features/changes/internal/formChanges.ts`: `setValue`,
//! `setDisplay`, `setAppearanceText`, `reset`, `revertForm`, and the fill
//! checks `authorizeFill`, `authorizeScriptOutput`, `authorizeSomeFill`,
//! `authorizeRevertedFills`, `opsFillOwnField` and `stepsFillOwnField`).
//!
//! Each op captures the dictionaries it may write (the field and each of
//! its widgets, with `/AP` and `/MK`) before it writes, and how it left them
//! after ([`super::objects`]). Its reverse puts them back while they still
//! read as it left them; a value write's reverse, when someone changed them
//! since, puts back by value each field still holding what it set.

use serde_json::Value;

use super::applier::{ChangeContext, Done, assert_expected, left_alone};
use super::item::ChangeItem;
use super::objects::{capture_after, capture_before, revert_objects, unchanged_since};
use super::record::{
    CapturedObject, FieldValueChange, ObjectsRevertStep, ReverseStep, RevertFallback, RevertSubject,
};
use crate::auth::scope::{DocCapability, FieldWriteAction};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::forms::snapshot::{FormFieldDto, FormFieldRef, FormWidgetRef};
use crate::features::forms::value::{FormFieldValue, value_of};
use crate::features::forms::write::{
    FieldWriteResult, FormFieldDisplay, FormMutationMeta, FormMutator, MetaWidget,
    form_mutation_meta, holds_value, load_model, read_at, redrawn_by_value, resolve,
};

/// What a value write may write in a widget: its appearance and its look
/// (`WIDGET_KEYS`).
pub(super) const WIDGET_KEYS: &[&str] = &["AP", "MK"];

/// The ops that fill a form; the rest change its design (`FILLS`).
const FILLS: &[&str] = &[
    "forms.setValue",
    "forms.setDisplay",
    "forms.setAppearanceText",
    "forms.reset",
    "forms.setSignatureAppearance",
];

/// A field as an op found it (`ReadField`).
pub(super) struct ReadField {
    pub(super) object_number: u32,
    pub(super) field: FormFieldDto,
}

/// `forms.setValue`; its reverse puts back the value, and the widgets as
/// they were.
pub fn set_value(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    value: &FormFieldValue,
    expect: Option<&FormFieldValue>,
    op_index: usize,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    let before = read_field(ctx, reference)?;
    authorize_fill(ctx, &before)?;
    if let Some(expect) = expect {
        assert_expected(op_index, &object(&value_of(&before.field))?, &object(expect)?)?;
    }
    write_value(ctx, &before, value)
}

/// `forms.setDisplay`: a script's visibility effect.
pub fn set_display(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    display: FormFieldDisplay,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    let before = read_field(ctx, reference)?;
    authorize_script_output(ctx, &before)?;
    write_effect(ctx, "forms.setDisplay", &before, |mutator, cancel| {
        mutator.set_display(reference, display, cancel)
    })
}

/// `forms.setAppearanceText`: a script's formatted text, drawn without
/// changing the value.
pub fn set_appearance_text(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    text: &str,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    let before = read_field(ctx, reference)?;
    authorize_fill(ctx, &before)?;
    write_effect(ctx, "forms.setAppearanceText", &before, |mutator, cancel| {
        mutator.set_appearance_text(reference, text, cancel)
    })
}

/// `forms.reset`; its reverse puts back each field it changed.
pub fn reset(
    ctx: &mut ChangeContext<'_>,
    fields: Option<&[FormFieldRef]>,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    authorize_some_fill(ctx)?;
    let targets = match fields {
        Some(fields) => {
            fields.iter().map(|reference| read_field(ctx, reference)).collect::<Result<_, _>>()?
        }
        None => all_fields(ctx)?
            .into_iter()
            .filter(|target| holds_value(target.field.base.family))
            .collect::<Vec<_>>(),
    };
    // A reset fills in every field it puts back: each must be the caller's
    // to fill.
    for target in &targets {
        ctx.authority
            .require_field_write(target.field.base.group_id.as_deref(), FieldWriteAction::Fill)?;
    }
    let document = ctx.session.document()?.document();
    let wanted: Vec<(u32, &[&str])> = targets.iter().flat_map(objects_of).collect();
    let pending = capture_before(document, &wanted);
    let result = mutator(ctx)?.reset(fields, ctx.cancel)?;
    let objects = capture_after(document, pending)?;
    let changed: Vec<u32> =
        result.fields.iter().map(|field| field.base.field_object_number()).collect();
    let item = ChangeItem::FormsReset {
        fields: to_values(&result.fields)?,
        widgets: rows(ctx, &result.meta)?,
        meta: result.meta,
        skipped: Vec::new(),
    };
    // Every field was at its default already: nothing was written to undo.
    if changed.is_empty() {
        return Ok(Done { item, reverse: Vec::new() });
    }
    let fallback = targets
        .iter()
        .filter(|target| changed.contains(&target.object_number))
        .filter_map(|target| {
            let left = result
                .fields
                .iter()
                .find(|f| f.base.field_object_number() == target.object_number)?;
            Some(FieldValueChange {
                object_number: target.object_number,
                before: value_of(&target.field),
                left: value_of(left),
            })
        })
        .collect();
    Ok(Done {
        item,
        reverse: vec![ReverseStep::ObjectsRevert(ObjectsRevertStep {
            reports: "forms.reset".into(),
            objects,
            subject: RevertSubject::Fields { fields: changed },
            appearance_changed: None,
            fallback: Some(RevertFallback::Value { fields: fallback }),
            rights: None,
        })],
    })
}

/// Undo a form write (`revertForm`): the dictionaries back as they were,
/// when each still reads as the write left it. When one doesn't, a value
/// write puts back each field still showing the value it set; any other
/// write is left alone.
pub fn revert_form(
    ctx: &mut ChangeContext<'_>,
    step: &ObjectsRevertStep,
) -> Result<Done, EngineError> {
    let RevertSubject::Fields { fields } = &step.subject else {
        return left_alone(ctx, &step.reports);
    };
    if FILLS.contains(&step.reports.as_str()) {
        authorize_reverted_fills(ctx, fields)?;
    } else {
        ctx.authority.require(DocCapability::DocFormsModify)?;
    }
    if step.rights.is_some() {
        ctx.authority.require(DocCapability::DocFormsImport)?;
    }
    let document = ctx.session.document()?.document();
    if unchanged_since(document, &step.objects) {
        let redo = revert_objects(document, &step.objects)?;
        let read = fields
            .iter()
            .map(|object_number| read_field_by_number(ctx, *object_number).map(|read| read.field))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Done {
            item: form_item(ctx, &step.reports, read)?,
            reverse: vec![ReverseStep::ObjectsRevert(ObjectsRevertStep {
                objects: redo,
                fallback: step.fallback.as_ref().map(swap_fallback),
                ..step.clone()
            })],
        });
    }
    if let Some(RevertFallback::Properties { object_number, restore, left }) = &step.fallback {
        return super::fields::revert_properties(ctx, *object_number, restore, left);
    }
    let Some(RevertFallback::Value { fields: changes }) = &step.fallback else {
        return left_alone(ctx, &step.reports);
    };
    let mut written: Vec<Done> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for change in changes {
        let Some(current) = try_read_field_by_number(ctx, change.object_number) else {
            continue;
        };
        if value_of(&current.field) == change.left {
            written.push(write_value(ctx, &current, &change.before)?);
        } else {
            skipped.push(current.field.base.name.clone());
        }
    }
    if written.is_empty() {
        return left_alone(ctx, &step.reports);
    }
    if step.reports == "forms.setValue" && written.len() == 1 {
        return Ok(written.remove(0));
    }
    let mut fields = Vec::new();
    let mut reverse = Vec::new();
    for done in written {
        if let ChangeItem::FormsSetValue { field, .. } = done.item {
            fields.push(serde_json::from_value(field).map_err(unreadable)?);
        }
        reverse.extend(done.reverse);
    }
    let mut item = reset_result(ctx, fields)?;
    if let ChangeItem::FormsReset { skipped: item_skipped, .. } = &mut item {
        *item_skipped = skipped;
    }
    Ok(Done { item, reverse })
}

/// Whether the change fills in a field the caller may fill
/// (`ctx.fillsOwnField()`): its value or its displayed text, or, for an
/// undo, a fill it puts back. Answered once.
fn fills_own_field(ctx: &ChangeContext<'_>) -> bool {
    if let Some(answer) = ctx.own_fill.answer.get() {
        return answer;
    }
    let answer = ctx.own_fill.refs.iter().any(|reference| {
        try_read_field(ctx, reference).is_some_and(|target| {
            ctx.authority
                .allows_field_write(FieldWriteAction::Fill, target.field.base.group_id.as_deref())
        })
    });
    ctx.own_fill.answer.set(Some(answer));
    answer
}

/// `authorizeFill`: the field's group is the caller's to fill; or a script
/// calculates it and the change also fills in a field of their own, so the
/// script's output travels with its input.
fn authorize_fill(ctx: &ChangeContext<'_>, target: &ReadField) -> Result<(), EngineError> {
    let group = target.field.base.group_id.as_deref();
    if ctx.authority.allows_field_write(FieldWriteAction::Fill, group) {
        return Ok(());
    }
    if is_calculated(ctx, target)? && fills_own_field(ctx) {
        return ctx.authority.require_unprotected(DocCapability::DocFormsFill);
    }
    ctx.authority.require_field_write(group, FieldWriteAction::Fill)
}

/// `authorizeScriptOutput`: a script's visibility output, which the caller
/// may write when they may fill the field in, or the change fills in a
/// field of their own.
fn authorize_script_output(ctx: &ChangeContext<'_>, target: &ReadField) -> Result<(), EngineError> {
    let group = target.field.base.group_id.as_deref();
    if ctx.authority.allows_field_write(FieldWriteAction::Fill, group) {
        return Ok(());
    }
    if fills_own_field(ctx) {
        return ctx.authority.require_unprotected(DocCapability::DocFormsFill);
    }
    ctx.authority.require_field_write(group, FieldWriteAction::Fill)
}

/// `authorizeSomeFill`: the caller may fill in some field, before the write
/// knows which.
fn authorize_some_fill(ctx: &ChangeContext<'_>) -> Result<(), EngineError> {
    if ctx.authority.allows_some_field_write(FieldWriteAction::Fill) {
        return Ok(());
    }
    ctx.authority.require(DocCapability::DocFormsFill)
}

/// `authorizeRevertedFills`: each field an undo or redo of a fill puts
/// back, as the fill would be.
fn authorize_reverted_fills(ctx: &ChangeContext<'_>, fields: &[u32]) -> Result<(), EngineError> {
    ctx.authority.require_unprotected(DocCapability::DocFormsFill)?;
    for object_number in fields {
        if let Some(target) = try_read_field_by_number(ctx, *object_number) {
            authorize_fill(ctx, &target)?;
        }
    }
    Ok(())
}

/// `isCalculated`: the field has a calculate script and is in the
/// calculation order.
fn is_calculated(ctx: &ChangeContext<'_>, target: &ReadField) -> Result<bool, EngineError> {
    if target.field.base.actions.as_ref().is_none_or(|actions| actions.calculate.is_none()) {
        return Ok(false);
    }
    let document = ctx.session.document()?.document();
    let model = load_model(document)?;
    // `/CO` by field index; only the field's own number is read.
    Ok((0..model.calculation_order_count()).any(|at| {
        u32::try_from(model.calculation_order_field(at))
            .ok()
            .and_then(|index| model.field(index))
            .is_some_and(|field| field.object_number() == target.object_number)
    }))
}

/// A value write to the field `before` reads, recording what it writes
/// (`writeValue`).
fn write_value(
    ctx: &mut ChangeContext<'_>,
    before: &ReadField,
    value: &FormFieldValue,
) -> Result<Done, EngineError> {
    let reference = FormFieldRef::ObjectNumber { field: before.object_number };
    let document = ctx.session.document()?.document();
    let pending = capture_before(document, &objects_of(before));
    let now = crate::clock::format_pdf_timestamp(ctx.clock.now());
    let result = mutator(ctx)?.set_value(
        &reference,
        value,
        &ctx.authority.identity,
        now.as_deref(),
        ctx.cancel,
    )?;
    let objects = capture_after(document, pending)?;
    let left = value_of(&result.field);
    let unchanged = result.meta.changed_widgets.is_empty() && value_of(&before.field) == left;
    let item = field_item(ctx, "forms.setValue", result)?;
    // The value it already held: nothing was written to undo.
    if unchanged {
        return Ok(Done { item, reverse: Vec::new() });
    }
    Ok(Done {
        item,
        reverse: vec![ReverseStep::ObjectsRevert(ObjectsRevertStep {
            reports: "forms.setValue".into(),
            objects,
            subject: RevertSubject::Fields { fields: vec![before.object_number] },
            appearance_changed: None,
            fallback: Some(RevertFallback::Value {
                fields: vec![FieldValueChange {
                    object_number: before.object_number,
                    before: value_of(&before.field),
                    left,
                }],
            }),
            rights: None,
        })],
    })
}

/// A script's display or formatted text on one field, as one write
/// (`writeEffect`): widgets that already showed so were left as they were,
/// with nothing to undo.
fn write_effect(
    ctx: &mut ChangeContext<'_>,
    reports: &str,
    before: &ReadField,
    write: impl FnOnce(
        &FormMutator<'_>,
        &crate::cancel::CancelToken,
    ) -> Result<FieldWriteResult, EngineError>,
) -> Result<Done, EngineError> {
    let document = ctx.session.document()?.document();
    let pending = capture_before(document, &objects_of(before));
    let wrote = !write(&mutator(ctx)?, ctx.cancel)?.meta.changed_widgets.is_empty();
    let field = read_field_by_number(ctx, before.object_number)?.field;
    let objects = capture_after(document, pending)?;
    let item = form_item(ctx, reports, vec![field])?;
    if !wrote {
        return Ok(Done { item, reverse: Vec::new() });
    }
    Ok(Done { item, reverse: vec![revert_step(reports, objects, vec![before.object_number])] })
}

pub(super) fn revert_step(
    reports: &str,
    objects: Vec<CapturedObject>,
    fields: Vec<u32>,
) -> ReverseStep {
    ReverseStep::ObjectsRevert(ObjectsRevertStep {
        reports: reports.to_owned(),
        objects,
        subject: RevertSubject::Fields { fields },
        appearance_changed: None,
        fallback: None,
        rights: None,
    })
}

/// A revert's fallback the other way round: what its redo puts back
/// (`swapFallback`).
fn swap_fallback(fallback: &RevertFallback) -> RevertFallback {
    match fallback {
        RevertFallback::Value { fields } => RevertFallback::Value {
            fields: fields
                .iter()
                .map(|change| FieldValueChange {
                    object_number: change.object_number,
                    before: change.left.clone(),
                    left: change.before.clone(),
                })
                .collect(),
        },
        RevertFallback::Annotation { restore, left } => {
            RevertFallback::Annotation { restore: left.clone(), left: restore.clone() }
        }
        RevertFallback::Properties { object_number, restore, left } => RevertFallback::Properties {
            object_number: *object_number,
            restore: left.clone(),
            left: restore.clone(),
        },
        RevertFallback::Outline { restore, left } => {
            RevertFallback::Outline { restore: left.clone(), left: restore.clone() }
        }
    }
}

/// The item a form write reports, with the fields read back (`formItem`).
pub(super) fn form_item(
    ctx: &ChangeContext<'_>,
    reports: &str,
    mut fields: Vec<FormFieldDto>,
) -> Result<ChangeItem, EngineError> {
    if reports == "forms.reset" {
        return reset_result(ctx, fields);
    }
    if fields.is_empty() {
        return Err(unreadable("a form write read back no field"));
    }
    let field = fields.remove(0);
    let widgets = MetaWidget::of_field(&field);
    let redrawn = redrawn_by(reports, &field, &widgets);
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        vec![field.base.field_ref.clone()],
        widgets,
        Some(redrawn),
    );
    field_item(ctx, reports, FieldWriteResult { field, meta })
}

/// A reset's item: the fields it wrote, read back (`resetResult`).
fn reset_result(
    ctx: &ChangeContext<'_>,
    fields: Vec<FormFieldDto>,
) -> Result<ChangeItem, EngineError> {
    let refs = fields.iter().map(|field| field.base.field_ref.clone()).collect();
    let widgets: Vec<MetaWidget> = fields.iter().flat_map(MetaWidget::of_field).collect();
    let redrawn = fields
        .iter()
        .flat_map(|field| redrawn_by("forms.reset", field, &MetaWidget::of_field(field)))
        .collect();
    let meta = form_mutation_meta(&ctx.session.write_stamp()?, refs, widgets, Some(redrawn));
    Ok(ChangeItem::FormsReset {
        fields: to_values(&fields)?,
        widgets: rows(ctx, &meta)?,
        meta,
        skipped: Vec::new(),
    })
}

/// The widgets of `field` a write of `reports` draws again (`redrawnBy`): a
/// value write redraws a text or choice field's, and only switches a
/// checkbox's or radio group's state; showing or hiding draws nothing.
fn redrawn_by(reports: &str, field: &FormFieldDto, widgets: &[MetaWidget]) -> Vec<MetaWidget> {
    match reports {
        "forms.setDisplay" => Vec::new(),
        "forms.setValue" | "forms.reset" => redrawn_by_value(field.base.family, widgets),
        _ => widgets.to_vec(),
    }
}

/// One field's item, with the rows of the widgets it changed, read now
/// (`rows`).
pub(super) fn field_item(
    ctx: &ChangeContext<'_>,
    reports: &str,
    result: FieldWriteResult,
) -> Result<ChangeItem, EngineError> {
    let field = serde_json::to_value(&result.field).map_err(unreadable)?;
    let widgets = rows(ctx, &result.meta)?;
    let meta = result.meta;
    Ok(match reports {
        "forms.setDisplay" => ChangeItem::FormsSetDisplay { field, widgets, meta },
        "forms.setAppearanceText" => ChangeItem::FormsSetAppearanceText { field, widgets, meta },
        "forms.update" => ChangeItem::FormsUpdate {
            field,
            widgets,
            calculation_order: None,
            meta,
            skipped: Vec::new(),
        },
        "forms.addWidget" => ChangeItem::FormsAddWidget { field, widgets, meta },
        "forms.removeWidget" => ChangeItem::FormsRemoveWidget { field, widgets, meta },
        _ => ChangeItem::FormsSetValue { field, widgets, meta },
    })
}

/// The rows of the widgets `meta` names as changed (`withWidgetRows`).
pub(super) fn rows(
    ctx: &ChangeContext<'_>,
    meta: &FormMutationMeta,
) -> Result<Vec<Value>, EngineError> {
    let widgets: Vec<FormWidgetRef> =
        meta.changed_widgets.iter().map(|widget| *widget.widget()).collect();
    let rows =
        crate::features::forms::widgets::read_widget_rows(ctx.session.document()?, &widgets)?;
    to_values(&rows)
}

pub(super) fn mutator<'a>(ctx: &'a ChangeContext<'_>) -> Result<FormMutator<'a>, EngineError> {
    Ok(FormMutator {
        document: ctx.session.document()?.document(),
        signed_document_policy: ctx.session.signed_document_policy(),
        stamp: ctx.session.write_stamp()?,
    })
}

/// The dictionaries a field write may change: the field and each widget,
/// with their appearances (`objectsOf`).
pub(super) fn objects_of(target: &ReadField) -> Vec<(u32, &'static [&'static str])> {
    std::iter::once((target.object_number, WIDGET_KEYS))
        .chain(target.field.widgets().map(|widget| (widget.annot_object_number, WIDGET_KEYS)))
        .collect()
}

pub(super) fn read_field(
    ctx: &ChangeContext<'_>,
    reference: &FormFieldRef,
) -> Result<ReadField, EngineError> {
    let document = ctx.session.document()?.document();
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let field = read_at(&model, resolved.index)?;
    Ok(ReadField { object_number: resolved.object_number, field })
}

pub(super) fn read_field_by_number(
    ctx: &ChangeContext<'_>,
    object_number: u32,
) -> Result<ReadField, EngineError> {
    read_field(ctx, &FormFieldRef::ObjectNumber { field: object_number })
}

pub(super) fn try_read_field(
    ctx: &ChangeContext<'_>,
    reference: &FormFieldRef,
) -> Option<ReadField> {
    read_field(ctx, reference).ok()
}

pub(super) fn try_read_field_by_number(
    ctx: &ChangeContext<'_>,
    object_number: u32,
) -> Option<ReadField> {
    try_read_field(ctx, &FormFieldRef::ObjectNumber { field: object_number })
}

/// Every field of the form with an object number (`allFields`).
fn all_fields(ctx: &ChangeContext<'_>) -> Result<Vec<ReadField>, EngineError> {
    let document = ctx.session.document()?.document();
    let model = load_model(document)?;
    let mut fields = Vec::new();
    for index in 0..model.field_count() {
        let field = read_at(&model, index)?;
        let object_number = field.base.field_object_number();
        if object_number > 0 {
            fields.push(ReadField { object_number, field });
        }
    }
    Ok(fields)
}

pub(super) fn to_values<T: serde::Serialize>(values: &[T]) -> Result<Vec<Value>, EngineError> {
    values.iter().map(|value| serde_json::to_value(value).map_err(unreadable)).collect()
}

fn object(value: &FormFieldValue) -> Result<serde_json::Map<String, Value>, EngineError> {
    match serde_json::to_value(value).map_err(unreadable)? {
        Value::Object(map) => Ok(map),
        _ => Err(unreadable("a form value is an object")),
    }
}

pub(super) fn unreadable(e: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("a form write's item: {e}"))
}

/// `forms.importValues`: a bundle's values, filled into the fields of the
/// same name (`importFormValues`, `FormImporter.planValues` and
/// `writeValue`). Everything left out is decided before the first write: a
/// field the document doesn't have, one of another family, a value the
/// field can't take, a field a signature locked, a field the caller may not
/// fill in. A write that changes nothing is not made. Each field's reverse
/// puts back its value and who filled it, as a `setValue`'s does; a
/// restoring import's steps take `doc.forms.import` too.
pub fn import_form_values(
    ctx: &mut ChangeContext<'_>,
    bundle: &super::op::WireBundle,
    attribution: super::op::Attribution,
    actor: Option<&crate::features::annotation::write::draft::AnnotationActor>,
    limits: &crate::features::transfer::BundleLimits,
) -> Result<Done, EngineError> {
    use crate::features::forms::value::{field_value_write_of, is_no_op_write};
    use crate::features::transfer::bundle::BundleKind;
    use crate::features::transfer::plan_form::plan_form_values_import;
    ctx.cancel.check()?;
    authorize_some_fill(ctx)?;
    let rights = super::fields::import_rights(ctx, attribution)?;
    let bundle = bundle.bundle()?;
    crate::features::transfer::resources::check_wire_bundle(BundleKind::Form, &bundle, limits)?;
    let fields =
        bundle.manifest.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let identity = super::fields::actor_identity(actor);
    let imported_by = identity.user_id.clone().filter(|id| !id.is_empty());

    // Every field of the form, as it is now, direct ones included.
    let targets: Vec<FormFieldDto> = {
        let document = ctx.session.document()?.document();
        let model = load_model(document)?;
        (0..model.field_count()).map(|index| read_at(&model, index)).collect::<Result<_, _>>()?
    };
    let target_values = to_values(&targets)?;
    let locked = mutator(ctx)?.lock_check();
    let mut refusal =
        |at: usize, value: &FormFieldValue| -> Result<Option<&'static str>, EngineError> {
            let target = &targets[at];
            // A field stored as a direct object can't be written.
            if target.base.field_object_number() == 0 {
                return Ok(Some("value-not-allowed"));
            }
            match field_value_write_of(target, value) {
                Ok(_) => {}
                Err(e) if e.code == EngineErrorCode::InvalidArg => {
                    return Ok(Some("value-not-allowed"));
                }
                Err(e) => return Err(e),
            }
            if locked(&target.base.name) {
                return Ok(Some("locked"));
            }
            let group = target.base.group_id.as_deref();
            Ok((!ctx.authority.allows_field_write(FieldWriteAction::Fill, group))
                .then_some("fill-not-allowed"))
        };
    let plan = plan_form_values_import(&fields, &target_values, &mut refusal)?;
    let writes: Vec<_> = plan
        .writes
        .into_iter()
        .filter(|write| {
            let target = &targets[write.target];
            field_value_write_of(target, &write.value).is_ok_and(|w| !is_no_op_write(target, &w))
        })
        .collect();

    let mut filled = Vec::with_capacity(writes.len());
    let mut reverse = Vec::with_capacity(writes.len());
    for write in &writes {
        ctx.cancel.check()?;
        let before = ReadField {
            object_number: targets[write.target].base.field_object_number(),
            field: targets[write.target].clone(),
        };
        let reference = FormFieldRef::ObjectNumber { field: before.object_number };
        let document = ctx.session.document()?.document();
        let pending = capture_before(document, &objects_of(&before));
        let now = crate::clock::format_pdf_timestamp(ctx.clock.now());
        mutator(ctx)?.set_value(&reference, &write.value, &identity, now.as_deref(), ctx.cancel)?;
        if rights.is_some() {
            let source = &fields[write.field]["data"];
            let text = |key: &str| source[key].as_str();
            crate::features::forms::field::restore_field_fill(
                document,
                before.object_number,
                text("filledBy"),
                text("filledByName"),
                text("filledAt"),
                imported_by.as_deref(),
            )?;
        }
        let objects = capture_after(document, pending)?;
        let field = read_field_by_number(ctx, before.object_number)?.field;
        reverse.push(ReverseStep::ObjectsRevert(ObjectsRevertStep {
            reports: "forms.setValue".into(),
            objects,
            subject: RevertSubject::Fields { fields: vec![before.object_number] },
            appearance_changed: None,
            fallback: Some(RevertFallback::Value {
                fields: vec![FieldValueChange {
                    object_number: before.object_number,
                    before: value_of(&before.field),
                    left: value_of(&field),
                }],
            }),
            rights,
        }));
        filled.push(field);
    }
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        filled.iter().map(|field| field.base.field_ref.clone()).collect(),
        filled.iter().flat_map(MetaWidget::of_field).collect(),
        None,
    );
    reverse.reverse();
    Ok(Done {
        item: ChangeItem::FormsImportValues {
            fields: to_values(&filled)?,
            widgets: rows(ctx, &meta)?,
            dropped: plan.dropped,
            meta,
        },
        reverse,
    })
}
