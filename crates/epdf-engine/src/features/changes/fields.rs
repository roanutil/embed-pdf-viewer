//! The form's design ops and their reverse
//! (`services/src/features/changes/internal/formChanges.ts`: `createField`,
//! `updateField`, `deleteField`, `addWidget`, `removeWidget`,
//! `deleteWidget`, `updateWidget`, `reorderWidgets`,
//! `reorderCalculations`, and the steps `removeField`, `restoreField`,
//! `restoreWidget`, `deleteRestoredWidget`, `restoreCalculationOrder` and
//! `revertProperties`).
//!
//! Each takes `doc.forms.modify`; one that writes a script, a submit or a
//! link takes `doc.forms.script` too. A create's reverse deletes the field
//! while nobody changed it since; a delete captures the field first, and its
//! reverse brings it back. An update, an added or removed widget, record the
//! dictionaries they write ([`super::objects`]); an update's reverse falls
//! back to the properties that still show what it set.

use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, assert_expected, left_alone, values_equal};
use super::forms::{
    ReadField, WIDGET_KEYS, field_item, form_item, mutator, objects_of, read_field,
    read_field_by_number, revert_step, rows, try_read_field, try_read_field_by_number, unreadable,
};
use super::item::ChangeItem;
use super::objects::{
    PendingCapture, capture_after, capture_before, revert_objects, unchanged_since,
};
use super::record::{
    CalculationOrderStep, FieldRemoveStep, FieldRestoreStep, ReverseStep, RevertFallback,
    StepRights, WidgetDeleteStep, WidgetRestoreStep,
};
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::write::{FieldActionsPatch, WidgetActionsPatch, scripts_of};
use crate::features::annotation::appearance::AnnotationFamily;
use crate::features::annotation::write::context::AnnotationWriteContext;
use crate::features::forms::calculation::{
    OrderChange, order_back, read_calculation_order, same_order, write_calculation_order,
};
use crate::features::forms::field::{
    self as f, FieldPosition, FieldWriter, FormFieldDraft, FormFieldPatch, WidgetPlacement,
};
use crate::features::forms::snapshot::{FormFieldDto, FormFieldKind, FormFieldRef, FormWidgetRef};
use crate::features::forms::write::{FieldWriteResult, MetaWidget, form_mutation_meta, load_model};
use crate::identity::{AnnotationRef, PageObjectNumber, PageRef};
use crate::mutation::list_position::AnnotationPosition;

/// `forms.create`; its reverse deletes the field, when nobody changed it
/// since (`createField`). Its undo takes it out of the calculation order
/// too.
pub fn create_field(
    ctx: &mut ChangeContext<'_>,
    draft: &Map<String, Value>,
    object_number: Option<&serde_json::Number>,
    widget_object_numbers: Option<&[serde_json::Number]>,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let mut draft: FormFieldDraft =
        serde_json::from_value(Value::Object(draft.clone())).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the draft is not the shape a field create takes: {e}"),
            )
        })?;
    if draft.writes_scripts() {
        ctx.authority.require(DocCapability::DocFormsScript)?;
    }
    draft.group_id = new_field_group(ctx, draft.group_id.take())?;
    let object_number = object_number.map(super::annotations::object_number_of).transpose()?;
    let widget_numbers = widget_object_numbers
        .map(|numbers| {
            numbers.iter().map(super::annotations::object_number_of).collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let plan = f::plan_create(ctx.session.document()?, &draft, widget_numbers.as_deref())?;
    if let Some(number) = object_number {
        ctx.session.claim_object_number(number)?;
    }
    for number in widget_numbers.iter().flatten() {
        ctx.session.claim_object_number(*number)?;
    }
    ctx.cancel.check()?;
    let written = {
        let writer = field_writer(ctx)?;
        f::create_field(
            &writer,
            &draft,
            &plan,
            object_number,
            widget_numbers.as_deref(),
            &ctx.authority.identity,
            ctx.cancel,
        )?
    };
    let field = written.field;
    let left = to_value(&field)?;
    let object_number = field.base.field_object_number();
    let (field, widgets, meta) = field_result(ctx, field)?;
    Ok(Done {
        item: ChangeItem::FormsCreate {
            field,
            widgets,
            calculation_order: order_result(written.calculation_order.as_ref()),
            meta,
        },
        reverse: vec![ReverseStep::FieldRemove(FieldRemoveStep {
            object_number,
            left,
            rights: None,
        })],
    })
}

/// `newFieldGroup`: the group a new field goes in: the one the write names,
/// else the caller's own; a group other than their own takes
/// `fields:set-group` for it.
fn new_field_group(
    ctx: &ChangeContext<'_>,
    named: Option<String>,
) -> Result<Option<String>, EngineError> {
    let group = named.or_else(|| ctx.authority.identity.group_id.clone());
    if let Some(group) = &group {
        ctx.authority.require_field_group(group)?;
    }
    Ok(group)
}

/// The structural writer, over the session's document.
fn field_writer<'a>(ctx: &'a ChangeContext<'_>) -> Result<FieldWriter<'a>, EngineError> {
    let ids = ctx.ids;
    let clock = ctx.clock;
    Ok(FieldWriter {
        owned: ctx.session.document()?,
        mutator: mutator(ctx)?,
        context: AnnotationWriteContext { fonts: ctx.fonts, resources: None },
        new_name: Box::new(move || {
            crate::features::annotation::write::create::uuid_v7(clock.now().epoch_millis, ids)
        }),
        now: crate::clock::format_pdf_timestamp(clock.now()),
    })
}

/// `forms.update`. Its reverse puts the field and its widgets back as they
/// were; when someone changed them since, it puts back each property still
/// holding what the update set (`updateField`).
pub fn update_field(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    patch: &Map<String, Value>,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    if patch_actions(patch)?.is_some_and(|actions| actions.writes_scripts()) {
        ctx.authority.require(DocCapability::DocFormsScript)?;
    }
    let before = read_field(ctx, reference)?;
    if let Some(group) = patch.get("groupId").and_then(Value::as_str)
        && Some(group) != before.field.base.group_id.as_deref()
    {
        ctx.authority.require_field_group(group)?;
    }
    if let Some(expect) = expect {
        assert_expected(at, &properties_of(&before.field), expect)?;
    }
    write_update(ctx, &before, patch)
}

/// A patch's `actions`, read alone, before the field says its family.
fn patch_actions(patch: &Map<String, Value>) -> Result<Option<FieldActionsPatch>, EngineError> {
    patch
        .get("actions")
        .map(|actions| {
            serde_json::from_value(actions.clone()).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("a field's actions are its scripts by event: {e}"),
                )
                .with_details(serde_json::json!({ "field": "actions" }))
            })
        })
        .transpose()
}

/// An update of the field `before` reads, recording the dictionaries it
/// writes and the properties it changes: their old values, and the values
/// it left (`writeUpdate`). What it did to the calculation order goes on
/// its item, and the step that takes it back after the others.
fn write_update(
    ctx: &mut ChangeContext<'_>,
    before: &ReadField,
    patch: &Map<String, Value>,
) -> Result<Done, EngineError> {
    let typed = FormFieldPatch::of_wire(patch, before.field.base.family)?;
    let reference = FormFieldRef::ObjectNumber { field: before.object_number };
    let document = ctx.session.document()?.document();
    let pending = capture_before(document, &objects_of(before));
    let written = f::update_field(&mutator(ctx)?, &reference, &typed, ctx.cancel)?;
    let objects = capture_after(document, pending)?;
    let was = properties_of(&before.field);
    let is = properties_of(&written.field);
    let keys: Vec<&String> = patch
        .keys()
        .filter(|key| *key != "family" && !values_equal(was.get(*key), is.get(*key)))
        .collect();
    let pick = |from: &Map<String, Value>| -> Map<String, Value> {
        keys.iter()
            .map(|key| ((*key).clone(), from.get(*key).cloned().unwrap_or(Value::Null)))
            .collect()
    };
    let fallback = RevertFallback::Properties {
        object_number: before.object_number,
        restore: pick(&was),
        left: pick(&is),
    };
    let mut step = revert_step("forms.update", objects, vec![before.object_number]);
    if let ReverseStep::ObjectsRevert(revert) = &mut step {
        revert.fallback = Some(fallback);
    }
    let item = form_item(ctx, "forms.update", vec![written.field])?;
    Ok(with_order_change(Done { item, reverse: vec![step] }, written.calculation_order))
}

/// `withOrderChange`: `done` with what its write did to the calculation
/// order, when it changed it: the new order on its item, and the step that
/// takes the change back after the others.
fn with_order_change(mut done: Done, order: Option<OrderChange>) -> Done {
    let Some(order) = order else { return done };
    let after = order_result(Some(&order));
    match &mut done.item {
        ChangeItem::FormsUpdate { calculation_order, .. }
        | ChangeItem::FormsCreate { calculation_order, .. }
        | ChangeItem::FormsDelete { calculation_order, .. } => *calculation_order = after,
        _ => return done,
    }
    done.reverse.push(order_step(order));
    done
}

/// `orderResult`: the new calculation order a write's result carries.
fn order_result(order: Option<&OrderChange>) -> Option<Vec<FormFieldRef>> {
    order
        .map(|order| order.after.iter().map(|n| FormFieldRef::ObjectNumber { field: *n }).collect())
}

fn order_step(OrderChange { before, after }: OrderChange) -> ReverseStep {
    ReverseStep::CalculationsRestore(CalculationOrderStep { before, after })
}

/// `revertProperties`: the second chance of a field update's undo: each
/// property that still holds what the update set goes back, through an
/// ordinary update. A rename goes back only while no sibling has taken the
/// old name.
pub fn revert_properties(
    ctx: &mut ChangeContext<'_>,
    object_number: u32,
    restore: &Map<String, Value>,
    left: &Map<String, Value>,
) -> Result<Done, EngineError> {
    let Some(current) = try_read_field_by_number(ctx, object_number) else {
        return left_alone(ctx, "forms.update");
    };
    let now = properties_of(&current.field);
    let keys: Vec<&String> = left.keys().collect();
    let kept: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|key| {
            values_equal(now.get(*key), left.get(*key))
                && (*key != "name"
                    || name_is_free(
                        ctx,
                        &current.field.base.name,
                        restore.get("name").and_then(Value::as_str).unwrap_or_default(),
                    ))
        })
        .collect();
    if kept.is_empty() {
        return left_alone(ctx, "forms.update");
    }
    let patch: Map<String, Value> = kept
        .iter()
        .map(|key| ((*key).clone(), restore.get(*key).cloned().unwrap_or(Value::Null)))
        .collect();
    let mut done = write_update(ctx, &current, &patch)?;
    let skipped: Vec<String> =
        keys.into_iter().filter(|key| !kept.contains(key)).cloned().collect();
    if let ChangeItem::FormsUpdate { skipped: item_skipped, .. } = &mut done.item {
        *item_skipped = skipped;
    }
    Ok(done)
}

/// `nameIsFree`: whether the field `full_name` could be renamed to
/// `segment`: no sibling holds that name.
fn name_is_free(ctx: &ChangeContext<'_>, full_name: &str, segment: &str) -> bool {
    let parent = full_name.rfind('.').map_or("", |at| &full_name[..=at]);
    try_read_field(ctx, &FormFieldRef::Fqn { name: format!("{parent}{segment}") }).is_none()
}

/// `forms.delete`; its reverse brings back the field, its widgets and the
/// parents it pruned (`deleteField`).
pub fn delete_field(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let before = read_field(ctx, reference)?;
    if let Some(expect) = expect {
        assert_expected(at, &object(to_value(&before.field)?)?, expect)?;
    }
    write_delete(ctx, &before, None)
}

/// A delete of the field `before` reads, capturing it first
/// (`writeDelete`). Its undo brings the field back first, then into the
/// calculation order, and takes `rights` too.
fn write_delete(
    ctx: &mut ChangeContext<'_>,
    before: &ReadField,
    rights: Option<StepRights>,
) -> Result<Done, EngineError> {
    let owned = ctx.session.document()?;
    let capture = owned.document().export_field_raw(before.object_number).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the capture of field {} failed", before.object_number),
        )
    })?;
    let reference = FormFieldRef::ObjectNumber { field: before.object_number };
    let deleted = f::delete_field(owned, &mutator(ctx)?, &reference, ctx.cancel)?;
    let pages: Vec<PageRef> = before
        .field
        .widgets()
        .filter_map(|widget| PageObjectNumber::new(widget.page_object_number).ok())
        .map(PageRef::new)
        .collect();
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        vec![deleted.deleted],
        deleted.removed_widgets.into_iter().map(MetaWidget::Plain).collect(),
        None,
    );
    let done = Done {
        item: ChangeItem::FormsDelete { calculation_order: None, meta },
        reverse: vec![ReverseStep::FieldRestore(FieldRestoreStep {
            object_number: before.object_number,
            name: before.field.base.name.clone(),
            pages,
            capture,
            rights,
        })],
    };
    Ok(with_order_change(done, deleted.calculation_order))
}

/// Deletes a field a create made, when nobody changed it since
/// (`removeField`).
pub fn remove_field(
    ctx: &mut ChangeContext<'_>,
    step: &FieldRemoveStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocFormsModify)?;
    if step.rights.is_some() {
        ctx.authority.require(DocCapability::DocFormsImport)?;
    }
    let Some(current) = try_read_field_by_number(ctx, step.object_number) else {
        return left_alone(ctx, "forms.delete");
    };
    if !values_equal(Some(&to_value(&current.field)?), Some(&step.left)) {
        return left_alone(ctx, "forms.delete");
    }
    write_delete(ctx, &current, step.rights)
}

/// Brings back a field a delete removed. Left alone when another field took
/// its name, or a page its widgets were on is gone (`restoreField`).
pub fn restore_field(
    ctx: &mut ChangeContext<'_>,
    step: &FieldRestoreStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocFormsModify)?;
    if step.rights.is_some() {
        ctx.authority.require(DocCapability::DocFormsImport)?;
    }
    if try_read_field(ctx, &FormFieldRef::Fqn { name: step.name.clone() }).is_some() {
        return left_alone(ctx, "forms.restore");
    }
    for page in &step.pages {
        if ctx.session.document()?.page_index(page.object_number()).is_err() {
            return left_alone(ctx, "forms.restore");
        }
    }
    let document = ctx.session.document()?.document();
    if !document.import_field_raw(&step.capture) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFForm_ImportFieldRaw refused a capture",
        ));
    }
    let field = read_field_by_number(ctx, step.object_number)?.field;
    let left = to_value(&field)?;
    let (field, widgets, meta) = field_result(ctx, field)?;
    Ok(Done {
        item: ChangeItem::FormsRestore { field, widgets, meta },
        reverse: vec![ReverseStep::FieldRemove(FieldRemoveStep {
            object_number: step.object_number,
            left,
            rights: step.rights,
        })],
    })
}

/// `forms.addWidget`. Its reverse puts back the field and the pages the
/// widget went onto, which leaves the new widget (and a merged field's split
/// one) reaching nothing (`addWidget`).
pub fn add_widget(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    placement: &Map<String, Value>,
    object_number: Option<&serde_json::Number>,
    split_object_number: Option<&serde_json::Number>,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let placement: WidgetPlacement = serde_json::from_value(Value::Object(placement.clone()))
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the placement is not the shape a widget takes: {e}"),
            )
        })?;
    if placement.actions.as_ref().is_some_and(WidgetActionsPatch::writes_scripts) {
        ctx.authority.require(DocCapability::DocFormsScript)?;
    }
    let before = read_field(ctx, reference)?;
    let mut pages = vec![placement.page.get()];
    for widget in before.field.widgets() {
        if widget.annot_object_number == before.object_number
            && widget.page_object_number > 0
            && !pages.contains(&widget.page_object_number)
        {
            pages.push(widget.page_object_number);
        }
    }
    let plan = f::plan_add_widget(ctx.session.document()?, &mutator(ctx)?, reference, &placement)?;
    let object_number = object_number.map(super::annotations::object_number_of).transpose()?;
    // The split number is used only when the field is merged.
    let split = if plan.splits() {
        split_object_number.map(super::annotations::object_number_of).transpose()?
    } else {
        None
    };
    if let Some(number) = object_number {
        ctx.session.claim_object_number(number)?;
    }
    if let Some(number) = split {
        ctx.session.claim_object_number(number)?;
    }
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let mut wanted = objects_of(&before);
    for page in &pages {
        wanted.push((*page, &[]));
    }
    let pending = capture_before(document, &wanted);
    let (field, _) = {
        let writer = field_writer(ctx)?;
        f::add_widget(&writer, &plan, object_number, split, ctx.cancel)?
    };
    // The new widget, and a merged field's widget half at its new number:
    // both are new to the field, and both are the answer's widgets.
    let known: Vec<u32> = before.field.widgets().map(|w| w.annot_object_number).collect();
    let added: Vec<MetaWidget> = MetaWidget::of_field(&field)
        .into_iter()
        .filter(|widget| !known.contains(&widget.widget().annot_object_number))
        .collect();
    let made: Vec<PendingCapture> = added
        .iter()
        .map(|widget| PendingCapture {
            object_number: widget.widget().annot_object_number,
            deep: WIDGET_KEYS.iter().map(|key| (*key).to_owned()).collect(),
            before: None,
        })
        .collect();
    let objects = capture_after(document, pending.into_iter().chain(made).collect())?;
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        vec![field.base.field_ref.clone()],
        added,
        None,
    );
    let item = field_item(ctx, "forms.addWidget", FieldWriteResult { field, meta })?;
    Ok(Done {
        item,
        reverse: vec![revert_step("forms.removeWidget", objects, vec![before.object_number])],
    })
}

/// `forms.removeWidget`; its reverse attaches the widget again, as it was
/// (`removeWidget`).
pub fn remove_widget(
    ctx: &mut ChangeContext<'_>,
    reference: &FormFieldRef,
    widget: &AnnotationRef,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let before = read_field(ctx, reference)?;
    let widget_number = match widget {
        AnnotationRef::ObjectNumber { annot, .. } => *annot,
        _ => 0,
    };
    let document = ctx.session.document()?.document();
    let pending = capture_before(document, &[(before.object_number, &[]), (widget_number, &[])]);
    let (field, removed) = f::detach_widget(&mutator(ctx)?, reference, widget, ctx.cancel)?;
    let objects = capture_after(document, pending)?;
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        vec![field.base.field_ref.clone()],
        vec![MetaWidget::Plain(removed)],
        None,
    );
    let item = field_item(ctx, "forms.removeWidget", FieldWriteResult { field, meta })?;
    Ok(Done {
        item,
        reverse: vec![revert_step("forms.addWidget", objects, vec![before.object_number])],
    })
}

/// `forms.deleteWidget`: the widget leaves its page, and its field when it
/// has one. Its reverse brings it back at its place and into its field
/// (`deleteWidget`).
pub fn delete_widget(
    ctx: &mut ChangeContext<'_>,
    widget: &AnnotationRef,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let current = super::annotations::read_annotation(ctx, widget)?;
    let subtype = current.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if subtype != "widget" {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{subtype} is not a widget: delete it with page.annotations.delete"),
        )
        .with_details(serde_json::json!({ "field": "widget" })));
    }
    if let Some(expect) = expect {
        assert_expected(at, &current, expect)?;
    }
    write_widget_delete(ctx, &current)
}

/// A widget's delete (`writeWidgetDelete`): out of its field (the field's
/// dictionary and the widget's captured around it), then off its page (the
/// widget captured as it is there), recording the restore.
fn write_widget_delete(
    ctx: &mut ChangeContext<'_>,
    widget: &Map<String, Value>,
) -> Result<Done, EngineError> {
    let reference: AnnotationRef =
        serde_json::from_value(widget.get("ref").cloned().unwrap_or_default())
            .map_err(unreadable)?;
    let page = reference.page();
    let owned = ctx.session.document()?;
    let page_index = owned.page_index(page)?;
    // Captures go by object number: a widget born inline gets one first.
    crate::features::annotation::write::delete::promote_inline_annotations(
        owned.document(),
        page_index,
        page,
    )?;
    let widget_number = super::annotations::annotation_object_number(ctx, &reference)?;
    let owner_ref: Option<FormFieldRef> =
        serde_json::from_value(widget.get("field").cloned().unwrap_or_default()).ok().flatten();
    let owner = owner_ref.as_ref().map(|r| read_field(ctx, r)).transpose()?;
    if let Some(owner) = &owner
        && owner.object_number == widget_number
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "widget {widget_number} is its field's own dictionary (a merged field/widget) - \
                 delete the field with doc.forms.delete"
            ),
        )
        .with_details(serde_json::json!({ "field": "widget" })));
    }
    let document = ctx.session.document()?.document();
    let mut detached = Vec::new();
    if let Some(owner) = &owner {
        let pending = capture_before(document, &[(owner.object_number, &[]), (widget_number, &[])]);
        f::detach_widget(
            &mutator(ctx)?,
            &FormFieldRef::ObjectNumber { field: owner.object_number },
            &reference,
            ctx.cancel,
        )?;
        detached = capture_after(document, pending)?;
    }
    let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
        document, page_index, &reference,
    )?;
    let capture = document.export_annotations_raw(page_index, &[index]).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the capture of widget {widget_number} failed"),
        )
    })?;
    crate::features::annotation::write::delete::delete_released_widget(
        document, page_index, &reference, 0,
    )?;
    let field = owner.as_ref().map(|o| read_field_by_number(ctx, o.object_number)).transpose()?;
    let changed = MetaWidget::Plain(FormWidgetRef {
        annot_object_number: widget_number,
        page_object_number: page.get(),
    });
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        field.iter().map(|f| f.field.base.field_ref.clone()).collect(),
        vec![changed],
        None,
    );
    Ok(Done {
        item: ChangeItem::FormsDeleteWidget {
            widget: reference.clone(),
            page: PageRef::new(page),
            field: field.map(|f| to_value(&f.field)).transpose()?,
            meta,
        },
        reverse: vec![ReverseStep::WidgetRestore(WidgetRestoreStep {
            page: PageRef::new(page),
            widget: reference,
            capture,
            field_object_number: owner.map(|o| o.object_number),
            detached,
        })],
    })
}

/// Brings back a widget a delete removed: onto its page at its place, then
/// into its field, whose dictionary goes back as it was before the widget
/// left it (`restoreWidget`).
pub fn restore_widget(
    ctx: &mut ChangeContext<'_>,
    step: &WidgetRestoreStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let Ok(page_index) = ctx.session.document()?.page_index(step.page.object_number()) else {
        return left_alone(ctx, "forms.restoreWidget");
    };
    if try_widget_row(ctx, &step.widget).is_some() {
        return left_alone(ctx, "forms.restoreWidget");
    }
    // The widget is gone, so only its field shows whether the delete still
    // holds.
    let document = ctx.session.document()?.document();
    let field_objects: Vec<_> = step
        .detached
        .iter()
        .filter(|object| Some(object.object_number) == step.field_object_number)
        .cloned()
        .collect();
    if !unchanged_since(document, &field_objects) {
        return left_alone(ctx, "forms.restoreWidget");
    }
    if !document.import_annotations_raw(page_index, &step.capture) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFPage_ImportAnnotsRaw refused a capture",
        ));
    }
    if !step.detached.is_empty() {
        revert_objects(document, &step.detached)?;
    }
    let row = widget_row(ctx, &step.widget)?;
    let field = step
        .field_object_number
        .map(|n| read_field_by_number(ctx, n))
        .transpose()?
        .map(|f| f.field);
    let changed = MetaWidget::Plain(FormWidgetRef {
        annot_object_number: super::annotations::annotation_object_number(ctx, &step.widget)?,
        page_object_number: step.page.object_number().get(),
    });
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        field.iter().map(|f| f.base.field_ref.clone()).collect(),
        vec![changed],
        None,
    );
    Ok(Done {
        item: ChangeItem::FormsRestoreWidget {
            field: field.as_ref().map(to_value).transpose()?,
            widgets: vec![Value::Object(row.clone())],
            meta,
        },
        reverse: vec![ReverseStep::WidgetDelete(WidgetDeleteStep {
            widget: step.widget.clone(),
            left: Value::Object(row),
        })],
    })
}

/// Deletes a widget a restore brought back, when it is as the restore left
/// it (`deleteRestoredWidget`).
pub fn delete_restored_widget(
    ctx: &mut ChangeContext<'_>,
    step: &WidgetDeleteStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let Some(current) = try_widget_row(ctx, &step.widget) else {
        return left_alone(ctx, "forms.deleteWidget");
    };
    if !values_equal(Some(&Value::Object(current.clone())), Some(&step.left)) {
        return left_alone(ctx, "forms.deleteWidget");
    }
    write_widget_delete(ctx, &current)
}

/// A widget's row as the form reads it now (`widgetRow`).
fn widget_row(
    ctx: &ChangeContext<'_>,
    reference: &AnnotationRef,
) -> Result<Map<String, Value>, EngineError> {
    try_widget_row(ctx, reference).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("{} reads as no widget", reference.key()),
        )
    })
}

/// The same, or `None` when the widget isn't on its page (`tryWidgetRow`).
fn try_widget_row(
    ctx: &ChangeContext<'_>,
    reference: &AnnotationRef,
) -> Option<Map<String, Value>> {
    super::annotations::read_annotation(ctx, reference)
        .ok()
        .filter(|row| row.get("subtype").and_then(Value::as_str) == Some("widget"))
}

/// `forms.updateWidget`: a widget's place and look, written as an annotation
/// update writes them. Its reverse puts back, by value, what still shows the
/// update (`updateWidget`).
pub fn update_widget(
    ctx: &mut ChangeContext<'_>,
    widget: &AnnotationRef,
    patch: &Map<String, Value>,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    if patch.get("actions").is_some_and(writes_scripts) {
        ctx.authority.require(DocCapability::DocFormsScript)?;
    }
    let current = super::annotations::read_annotation(ctx, widget)?;
    let subtype = current.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if subtype != "widget" {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{subtype} is not a widget: change it with doc.annotations.update"),
        )
        .with_details(serde_json::json!({ "field": "widget" })));
    }
    if let Some(expect) = expect {
        assert_expected(at, &current, expect)?;
    }
    let mut patch = patch.clone();
    patch.insert("subtype".into(), Value::String("widget".into()));
    super::annotations::write_update(
        ctx,
        widget,
        patch,
        &super::op::OpResources::default(),
        "forms.updateWidget",
    )
}

/// `forms.reorderWidgets`: the widgets' stacking order on their page, which
/// is also their tab order where the page's `/Tabs` follows it. Its reverse
/// puts them back beside their old neighbours (`reorderWidgets`).
pub fn reorder_widgets(
    ctx: &mut ChangeContext<'_>,
    page: PageObjectNumber,
    widgets: &[AnnotationRef],
    position: &AnnotationPosition,
) -> Result<Done, EngineError> {
    use crate::features::annotation::write::reorder;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let plan = {
        let owned = ctx.session.document()?;
        let page_index = owned.page_index(page)?;
        let stack = reorder::read_page_stack(owned.document(), page_index, page)?;
        reorder::plan_reorder(stack, AnnotationFamily::Widgets, widgets, position)?
    };
    reorder::apply_reorder(ctx.session.document()?.document(), &plan)?;
    super::annotations::reordered(ctx, &plan, widgets)
}

/// `forms.reorderCalculations`; its reverse puts each field back beside its
/// old neighbours (`reorderCalculations`).
pub fn reorder_calculations(
    ctx: &mut ChangeContext<'_>,
    fields: &[FormFieldRef],
    position: &FieldPosition,
) -> Result<Done, EngineError> {
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let document = ctx.session.document()?.document();
    let (order, moved) = f::reorder_calculations(document, fields, position, ctx.cancel)?;
    Ok(Done {
        item: calculations_item(ctx, &order.after, &moved)?,
        reverse: vec![order_step(order)],
    })
}

/// Takes back what a change did to the calculation order (`orderBack`).
/// Left alone when nothing of it is still as the change left it
/// (`restoreCalculationOrder`).
pub fn restore_calculation_order(
    ctx: &mut ChangeContext<'_>,
    step: &CalculationOrderStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let document = ctx.session.document()?.document();
    let now = read_calculation_order(&load_model(document)?);
    let change = OrderChange { before: step.before.clone(), after: step.after.clone() };
    let order = order_back(&now, &change, |n| try_read_field_by_number(ctx, n).is_some());
    if same_order(&now, &order) {
        return left_alone(ctx, "forms.reorderCalculations");
    }
    write_calculation_order(document, &order)?;
    let mut moved: Vec<u32> = Vec::new();
    for n in now.iter().chain(order.iter()) {
        if !moved.contains(n)
            && now.iter().position(|m| m == n) != order.iter().position(|m| m == n)
        {
            moved.push(*n);
        }
    }
    Ok(Done {
        item: calculations_item(ctx, &order, &moved)?,
        reverse: vec![order_step(OrderChange { before: now, after: order })],
    })
}

/// A calculation-order change's item: the whole new order, and the fields
/// that moved (`calculationsItem`).
fn calculations_item(
    ctx: &ChangeContext<'_>,
    order: &[u32],
    moved: &[u32],
) -> Result<ChangeItem, EngineError> {
    let refs = |numbers: &[u32]| -> Vec<FormFieldRef> {
        numbers.iter().map(|n| FormFieldRef::ObjectNumber { field: *n }).collect()
    };
    Ok(ChangeItem::FormsReorderCalculations {
        calculation_order: refs(order),
        meta: form_mutation_meta(&ctx.session.write_stamp()?, refs(moved), Vec::new(), None),
    })
}

/// `fieldResult`: a field read back, the rows of its widgets, and the meta
/// naming it and every widget it has, each drawn again.
fn field_result(
    ctx: &ChangeContext<'_>,
    field: FormFieldDto,
) -> Result<(Value, Vec<Value>, crate::features::forms::write::FormMutationMeta), EngineError> {
    let widgets = MetaWidget::of_field(&field);
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        vec![field.base.field_ref.clone()],
        widgets,
        None,
    );
    let rows = rows(ctx, &meta)?;
    Ok((to_value(&field)?, rows, meta))
}

/// `propertiesOf`: the properties a field has, in the shape `forms.update`
/// takes them: its own name segment, a default that isn't there as null,
/// options as label and value. A field update's `expect` and its undo
/// compare these.
pub fn properties_of(field: &FormFieldDto) -> Map<String, Value> {
    let base = &field.base;
    let mut out = Map::new();
    let name = base.name.rsplit('.').next().unwrap_or(&base.name);
    out.insert("name".into(), Value::String(name.to_owned()));
    out.insert("readOnly".into(), Value::Bool(base.read_only));
    out.insert("required".into(), Value::Bool(base.required));
    out.insert("noExport".into(), Value::Bool(base.no_export));
    out.insert("alternateName".into(), serde_json::json!(base.alternate_name));
    out.insert("mappingName".into(), serde_json::json!(base.mapping_name));
    // A group is changed, never removed: a field in none has nothing to put
    // back.
    if let Some(group) = &base.group_id {
        out.insert("groupId".into(), Value::String(group.clone()));
    }
    out.insert(
        "actions".into(),
        serde_json::to_value(scripts_of(base.actions.as_ref())).unwrap_or(Value::Null),
    );
    let has_default =
        base.default_value_entry != crate::features::forms::snapshot::FormValueEntry::None;
    let options = |list: &[crate::features::forms::snapshot::FormFieldOption]| -> Value {
        Value::Array(
            list.iter()
                .map(|o| serde_json::json!({ "label": o.label, "value": o.value }))
                .collect(),
        )
    };
    match &field.kind {
        FormFieldKind::Text { default_value, max_length, multiline, password, comb, .. } => {
            out.insert(
                "defaultValue".into(),
                if has_default { Value::String(default_value.clone()) } else { Value::Null },
            );
            out.insert("maxLength".into(), serde_json::json!(max_length));
            out.insert("multiline".into(), Value::Bool(*multiline));
            out.insert("password".into(), Value::Bool(*password));
            out.insert("comb".into(), Value::Bool(*comb));
        }
        FormFieldKind::Radio { radios_in_unison, no_toggle_to_off, .. } => {
            out.insert("radiosInUnison".into(), Value::Bool(*radios_in_unison));
            out.insert("noToggleToOff".into(), Value::Bool(*no_toggle_to_off));
        }
        FormFieldKind::Combobox { edit, default_value, options: list, .. } => {
            out.insert("edit".into(), Value::Bool(*edit));
            out.insert(
                "defaultValue".into(),
                if has_default { Value::String(default_value.clone()) } else { Value::Null },
            );
            out.insert("options".into(), options(list));
        }
        FormFieldKind::Listbox { multi_select, default_value, options: list, .. } => {
            out.insert("multiSelect".into(), Value::Bool(*multi_select));
            out.insert(
                "defaultValue".into(),
                if has_default { serde_json::json!(default_value) } else { Value::Null },
            );
            out.insert("options".into(), options(list));
        }
        _ => {}
    }
    out
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, EngineError> {
    serde_json::to_value(value).map_err(unreadable)
}

fn object(value: Value) -> Result<Map<String, Value>, EngineError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(unreadable("a field is an object")),
    }
}

/// `writesScripts`, event by event: an event gets an action write (not a
/// read's tree, sent back) whose tree holds a JavaScript, submit-form or URI
/// action (`needsScriptRight`). Removing an action (`null`) never does.
fn writes_scripts(actions: &Value) -> bool {
    fn needs_script_right(action: &Value) -> bool {
        matches!(
            action.get("type").and_then(Value::as_str),
            Some("javascript" | "submit-form" | "uri")
        ) || action
            .get("next")
            .and_then(Value::as_array)
            .is_some_and(|next| next.iter().any(needs_script_right))
    }
    actions.as_object().is_some_and(|events| {
        events.values().any(|action| {
            action.is_object() && action.get("root").is_none() && needs_script_right(action)
        })
    })
}

/// `forms.import`: a bundle's fields, copied in (`importForm` and
/// `FormImporter.import`). Each field `plan_form_import` keeps is created
/// with its widgets on their mapped pages, their flags and, with `values`,
/// the value the bundle holds; then the calculation order among them,
/// appended to the form's in bundle order; then who made and filled each,
/// restored or stamped. Its reverse deletes each field it made, the last
/// made first, when nobody changed it since; a restoring import's steps
/// take `doc.forms.import` too.
#[allow(clippy::too_many_arguments)]
pub fn import_form(
    ctx: &mut ChangeContext<'_>,
    bundle: &super::op::WireBundle,
    pages: Option<&crate::features::transfer::BundleImportPages>,
    attribution: super::op::Attribution,
    values: bool,
    actor: Option<&crate::features::annotation::write::draft::AnnotationActor>,
    limits: &crate::features::transfer::BundleLimits,
    may_script: bool,
) -> Result<Done, EngineError> {
    use crate::features::transfer::bundle::{BundleKind, bundle_pages_of};
    use crate::features::transfer::plan_form::{FormPlanInput, plan_form_import};
    use crate::features::transfer::resources::{check_wire_bundle, import_targets};
    ctx.cancel.check()?;
    ctx.authority.require(DocCapability::DocFormsModify)?;
    let rights = import_rights(ctx, attribution)?;
    let may_script = may_script
        && ctx.authority.grants.as_ref().is_none_or(|g| g.allows(DocCapability::DocFormsScript));
    let bundle = bundle.bundle()?;
    check_wire_bundle(BundleKind::Form, &bundle, limits)?;
    let list =
        |key: &str| bundle.manifest.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let (fields, widgets, order) = (list("fields"), list("widgets"), list("calculationOrder"));
    let names = field_names(ctx)?;
    let same = crate::features::transfer::BundleImportPages::Same;
    let plan = plan_form_import(FormPlanInput {
        pages: &bundle_pages_of(&bundle.manifest)?,
        fields: &fields,
        widgets: &widgets,
        calculation_order: &order,
        page_mapping: pages.unwrap_or(&same),
        target: &import_targets(ctx.session.document()?),
        name_taken: &|name| name_conflicts(&names, name),
        may_script,
        values,
    })?;

    let identity = actor_identity(actor);
    let imported_by = identity.user_id.clone().filter(|id| !id.is_empty());
    let order_before = f::calculation_order_now(ctx.session.document()?.document())?;
    let mut created = Vec::with_capacity(plan.creates.len());
    for planned in &plan.creates {
        ctx.cancel.check()?;
        let source = &fields[planned.field]["data"];
        let mut box_of =
            crate::features::pages::visible_box_reader(ctx.session.document()?.document());
        let mut draft =
            crate::page_space::forms::pdf_form_field_draft_of(planned.draft.clone(), &mut box_of)?;
        drop(box_of);
        let bundle_group = draft.remove("groupId").and_then(|g| g.as_str().map(str::to_owned));
        // A restoring import keeps each field's group, under the import's
        // rights; a copy is the caller's own create.
        let group =
            if rights.is_some() { bundle_group } else { new_field_group(ctx, bundle_group)? };
        if let Some(group) = group {
            draft.insert("groupId".into(), Value::String(group));
        }
        let draft: FormFieldDraft = serde_json::from_value(Value::Object(draft)).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("import: field {}: {e}", planned.field),
            )
        })?;
        let plan_create = f::plan_create(ctx.session.document()?, &draft, None)?;
        let written = {
            let writer = field_writer(ctx)?;
            f::create_field(&writer, &draft, &plan_create, None, None, &identity, ctx.cancel)?
        };
        let object_number = written.field.base.field_object_number();
        // A create writes no flags: each widget gets its row's.
        for (widget, flags) in written.field.widgets().zip(&planned.flags) {
            write_widget_flags(ctx, widget, flags)?;
        }
        let reference = FormFieldRef::ObjectNumber { field: object_number };
        if let Some(value) = &planned.value {
            let now = crate::clock::format_pdf_timestamp(ctx.clock.now());
            mutator(ctx)?.set_value(&reference, value, &identity, now.as_deref(), ctx.cancel)?;
        }
        if rights.is_some() {
            let document = ctx.session.document()?.document();
            let text = |key: &str| source[key].as_str();
            f::restore_field_creation(
                document,
                object_number,
                text("createdBy"),
                text("createdAt"),
                imported_by.as_deref(),
            )?;
            if planned.value.is_some() {
                f::restore_field_fill(
                    document,
                    object_number,
                    text("filledBy"),
                    text("filledByName"),
                    text("filledAt"),
                    imported_by.as_deref(),
                )?;
            }
        }
        created.push(object_number);
    }
    // Each create with a calculate script joined the order at its end; the
    // bundle's order among them is put back there.
    let calculating: Vec<FormFieldRef> = plan
        .calculation_order
        .iter()
        .map(|at| FormFieldRef::ObjectNumber { field: created[*at] })
        .collect();
    if !calculating.is_empty() {
        f::reorder_calculations(
            ctx.session.document()?.document(),
            &calculating,
            &FieldPosition::END,
            ctx.cancel,
        )?;
    }
    let order_after = f::calculation_order_now(ctx.session.document()?.document())?;

    let read: Vec<FormFieldDto> = created
        .iter()
        .map(|object_number| read_field_by_number(ctx, *object_number).map(|read| read.field))
        .collect::<Result<_, _>>()?;
    let meta = form_mutation_meta(
        &ctx.session.write_stamp()?,
        read.iter().map(|field| field.base.field_ref.clone()).collect(),
        read.iter().flat_map(MetaWidget::of_field).collect(),
        None,
    );
    let rows = rows(ctx, &meta)?;
    let ref_map = plan
        .creates
        .iter()
        .zip(&read)
        .map(|(planned, field)| {
            Ok(serde_json::json!({ "from": fields[planned.field]["data"]["ref"],
                "to": to_value(&field.base.field_ref)? }))
        })
        .collect::<Result<_, EngineError>>()?;
    let reverse = read
        .iter()
        .rev()
        .map(|field| {
            Ok(ReverseStep::FieldRemove(FieldRemoveStep {
                object_number: field.base.field_object_number(),
                left: to_value(field)?,
                rights,
            }))
        })
        .collect::<Result<_, EngineError>>()?;
    let calculation_order = (!same_order(&order_before, &order_after))
        .then(|| order_after.iter().map(|n| FormFieldRef::ObjectNumber { field: *n }).collect());
    Ok(Done {
        item: ChangeItem::FormsImport {
            fields: read.iter().map(to_value).collect::<Result<_, _>>()?,
            widgets: rows,
            ref_map,
            dropped: plan.dropped,
            calculation_order,
            meta,
        },
        reverse,
    })
}

/// A restoring import's rights, checked: `doc.forms.import`, which its undo
/// and redo take too. A stamping import takes none (`importRights`).
pub(super) fn import_rights(
    ctx: &ChangeContext<'_>,
    attribution: super::op::Attribution,
) -> Result<Option<StepRights>, EngineError> {
    if attribution != super::op::Attribution::Restore {
        return Ok(None);
    }
    ctx.authority.require(DocCapability::DocFormsImport)?;
    Ok(Some(StepRights::Import))
}

/// The session an import acts for, as a create and a value write stamp it.
pub(super) fn actor_identity(
    actor: Option<&crate::features::annotation::write::draft::AnnotationActor>,
) -> crate::auth::scope::IdentityClaims {
    let actor = actor.cloned().unwrap_or_default();
    crate::auth::scope::IdentityClaims {
        user_id: actor.user_id,
        group_id: actor.group_id,
        groups: Vec::new(),
        display_name: actor.display_name,
    }
}

/// The full name of every field of the form (`fieldNames`).
fn field_names(ctx: &ChangeContext<'_>) -> Result<Vec<String>, EngineError> {
    let document = ctx.session.document()?.document();
    let Some(model) = document.form_model() else { return Ok(Vec::new()) };
    let mut names = Vec::new();
    for index in 0..model.field_count() {
        if let Some(name) = model
            .field(index)
            .and_then(|field| crate::features::forms::snapshot::text(field.name()).ok().flatten())
        {
            names.push(name);
        }
    }
    Ok(names)
}

/// Whether a new field named `name` can't join a form whose fields are
/// `names`: one has that name, one sits under it, or one is its parent
/// (`nameConflicts`).
fn name_conflicts(names: &[String], name: &str) -> bool {
    names.iter().any(|other| {
        other == name
            || other.starts_with(&format!("{name}."))
            || name.starts_with(&format!("{other}."))
    })
}

/// A new widget's flags, as its row in the bundle has them: a create writes
/// none (`writeFlags`).
fn write_widget_flags(
    ctx: &ChangeContext<'_>,
    widget: &FormWidgetRef,
    flags: &[Option<bool>; 10],
) -> Result<(), EngineError> {
    let Ok(page) = PageObjectNumber::new(widget.page_object_number) else { return Ok(()) };
    let owned = ctx.session.document()?;
    let page_index = owned.page_index(page)?;
    let document = owned.document();
    let reference = AnnotationRef::ObjectNumber { page, annot: widget.annot_object_number };
    let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
        document, page_index, &reference,
    )?;
    let annotation = document.raw_annotation(page_index, index).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "a created widget is not where it was")
    })?;
    crate::features::annotation::write::primitives::set_flags(&annotation, flags)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_script_right_is_asked_per_event_and_a_read_tree_asks_none() {
        let tree = json!({ "root": { "type": "javascript", "script": "x", "subtype": "JavaScript",
            "next": [] }, "incomplete": false, "warningFlags": 0, "warnings": [] });
        let script = json!({ "type": "javascript", "script": "x" });
        assert!(!super::writes_scripts(&json!({ "activate": tree.clone() })));
        assert!(super::writes_scripts(&json!({ "activate": tree, "focus": script })));
        assert!(!super::writes_scripts(&json!({ "focus": null })));
        let chained = json!({ "type": "named", "name": "NextPage",
            "next": [{ "type": "uri", "uri": "https://example.com" }] });
        assert!(super::writes_scripts(&json!({ "blur": chained })));
    }
}
