//! The annotation ops and their reverse
//! (`services/src/features/changes/internal/annotationChanges.ts`).
//!
//! `annotations.create`: its reverse deletes what it made while it still
//! reads as the create left it. `annotations.update`: its reverse puts back,
//! by value, the fields it changed that still show what it set
//! (`objects.revert`, see [`super::record::ObjectsRevertStep`]).
//! `annotations.delete`: it takes the annotation's thread and popups with
//! it, captured first, and its reverse brings them all back
//! (`annotation.restore`), whose reverse deletes them again. So an undone
//! create can be redone. `annotations.reorder`: its reverse puts the rows
//! back beside their old neighbours (`annotation.reorder`).

use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::op::OpResources;
use super::record::{
    AnnotationRemoveStep, AnnotationReorderStep, AnnotationRestoreStep, ObjectsRevertStep,
    ReverseStep, RevertFallback, RevertSubject, UnlinkedPopup,
};
use crate::auth::scope::{CollabAction, CollabEntity, CollabTarget, DocCapability};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::appearance::AnnotationFamily;
use crate::features::annotation::write::context::AnnotationWriteContext;
use crate::features::annotation::write::create::{
    AnnotationMutationMeta, CreateRequest, CreateServices, create_annotation as create,
};
use crate::features::annotation::write::draft::AnnotationActor;
use crate::features::annotation::write::prepare::DraftResources;
use crate::features::annotation::write::update::{
    UpdateRequest, UpdateServices, update_annotation as update,
};
use crate::features::annotation::write::{delete, reorder};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::mutation::list_position::{AnnotationPosition, ListPosition};

/// `PermissionDenied(what, 'target')`.
fn forbidden(what: &str) -> EngineError {
    EngineError::permission_denied(what, Some("target"), None)
}

/// `authorizeAnnotationCreate`: the group the annotation goes in, the
/// caller's own when the data names none, checked as a group assignment,
/// and the create checked against the scope's collab rules for that owner.
/// Answers the actor the create stamps, or `None` when there is no one to
/// name.
fn authorize_create(
    ctx: &ChangeContext<'_>,
    group_id: Option<&str>,
) -> Result<Option<AnnotationActor>, EngineError> {
    let identity = &ctx.authority.identity;
    let group = group_id.map(str::to_owned).or_else(|| identity.group_id.clone());
    if let Some(grants) = &ctx.authority.grants {
        if let Some(group) = group.as_deref()
            && !crate::auth::scope::resolver::check_set_group(
                CollabEntity::Annotations,
                group,
                identity.group_id.as_deref(),
                grants.scope(),
            )
        {
            return Err(forbidden(&format!("annotations:set-group:group={group}")));
        }
        let target = CollabTarget { user_id: identity.user_id.clone(), group_id: group.clone() };
        if !grants.allows_collab(CollabAction::Create, &target) {
            return Err(forbidden("annotations:create"));
        }
    }
    // An empty name names no one (`actor.userId || actor.groupId ||
    // actor.displayName`).
    let named = |value: Option<String>| value.filter(|v| !v.is_empty());
    let actor = AnnotationActor {
        display_name: named(identity.display_name.clone()),
        user_id: named(identity.user_id.clone()),
        group_id: named(group),
    };
    let anyone =
        actor.user_id.is_some() || actor.group_id.is_some() || actor.display_name.is_some();
    Ok(anyone.then_some(actor))
}

/// An object number as sent: a whole number from 1 (`claimObjectNumber`).
pub(super) fn object_number_of(number: &serde_json::Number) -> Result<u32, EngineError> {
    // `Number.isInteger`: 70.0 is a whole number too.
    let whole = number.as_u64().or_else(|| {
        number
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX))
            .map(|n| n as u64)
    });
    whole.and_then(|n| u32::try_from(n).ok()).filter(|n| *n >= 1).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{number} is not an object number"),
        )
        .with_details(serde_json::json!({ "field": "objectNumber" }))
    })
}

/// `annotations.create`; its reverse deletes it, when nobody changed it
/// since (`createAnnotation`).
pub fn create_annotation(
    ctx: &mut ChangeContext<'_>,
    page: PageObjectNumber,
    data: &Map<String, Value>,
    object_number: Option<&serde_json::Number>,
    resources: &OpResources,
) -> Result<Done, EngineError> {
    if data.get("subtype").and_then(Value::as_str) == Some("widget") {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a widget is the form's: create one with doc.forms.create or doc.forms.addWidget",
        )
        .with_details(serde_json::json!({ "field": "subtype" })));
    }
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    let group_id = data.get("groupId").and_then(Value::as_str);
    let actor = authorize_create(ctx, group_id)?;

    let object_number = object_number.map(object_number_of).transpose()?;
    if let Some(number) = object_number {
        ctx.session.claim_object_number(number)?;
    }
    let appearance = resources.appearance.as_ref().map(|r| r.bytes()).transpose()?;
    let file = resources.file.as_ref().map(|r| r.bytes()).transpose()?;
    let session = &*ctx.session;
    let owned = session.document()?;
    let page_index = owned.page_index(page)?;
    let context = AnnotationWriteContext { fonts: ctx.fonts, resources: None };
    let services = CreateServices {
        context: &context,
        drawings: session.drawing_index()?,
        clock: ctx.clock,
        ids: ctx.ids,
    };
    let result = create(
        owned.document(),
        page_index,
        &session.write_stamp()?,
        CreateRequest {
            page,
            draft: data.clone(),
            resources: DraftResources { appearance, file },
            object_number,
            actor,
        },
        &services,
        ctx.cancel,
    )?;
    let annotation = serde_json::to_value(&result.annotation).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("a record did not serialize: {e}"),
        )
    })?;
    let reverse = ReverseStep::AnnotationRemove(AnnotationRemoveStep {
        reference: AnnotationRef::ObjectNumber {
            page,
            annot: match result.meta.changed.first() {
                Some(AnnotationRef::ObjectNumber { annot, .. }) => *annot,
                _ => {
                    return Err(EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "a create named no object",
                    ));
                }
            },
        },
        left: vec![annotation.clone()],
        parent: result.parent_before,
    });
    Ok(Done {
        item: ChangeItem::AnnotationsCreate { page, annotation, meta: result.meta },
        reverse: vec![reverse],
    })
}

/// `PermissionDenied` for a write that covers several annotations: the
/// refs it was refused for in `details.refs`.
fn denied_for(required: &str, refs: &[AnnotationRef]) -> EngineError {
    let count = refs.len();
    EngineError::with_message(
        EngineErrorCode::Forbidden,
        format!(
            "permission denied (target): {required} for {count} annotation{}",
            if count == 1 { "" } else { "s" }
        ),
    )
    .with_details(serde_json::json!({ "required": required, "context": "target", "refs": refs }))
}

/// `authorizeAnnotationDelete`: each member checked against the scope's
/// collab rules for its owner, every check before the first write.
fn authorize_delete(ctx: &ChangeContext<'_>, members: &[Value]) -> Result<(), EngineError> {
    let Some(grants) = &ctx.authority.grants else { return Ok(()) };
    let owner = |member: &Value, name: &str| {
        member.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned)
    };
    let mut refused = Vec::new();
    for member in members {
        let target =
            CollabTarget { user_id: owner(member, "userId"), group_id: owner(member, "groupId") };
        if !grants.allows_collab(CollabAction::Delete, &target) {
            refused.push(delete::member_ref(member)?);
        }
    }
    if refused.is_empty() { Ok(()) } else { Err(denied_for("annotations:delete", &refused)) }
}

/// `annotations.delete`; its reverse brings back everything it removed
/// (`deleteAnnotation`). A widget is the form's, refused naming the form op
/// that removes it.
pub fn delete_annotation(
    ctx: &mut ChangeContext<'_>,
    reference: &AnnotationRef,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    {
        let owned = ctx.session.document()?;
        let page_index = owned.page_index(reference.page())?;
        let document = owned.document();
        let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
            document, page_index, reference,
        )?;
        if let Some(attached) = delete::widget_attachment(document, page_index, index) {
            return Err(delete::widget_delete_refused(attached));
        }
    }
    let current = read_annotation(ctx, reference)?;
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    if let Some(expect) = expect {
        super::applier::assert_expected(at, &current, expect)?;
    }
    write_delete(ctx, reference)
}

/// A delete of `reference` and what goes with it, capturing them first
/// (`writeDelete`). Its reverse brings them back at the same numbers and
/// positions, and links again each annotation that stayed to the popup it
/// showed.
fn write_delete(
    ctx: &mut ChangeContext<'_>,
    reference: &AnnotationRef,
) -> Result<Done, EngineError> {
    let page = reference.page();
    let session = &*ctx.session;
    let owned = session.document()?;
    let page_index = owned.page_index(page)?;
    let document = owned.document();
    // A capture holds objects: whatever was born inline becomes one first.
    delete::promote_inline_annotations(document, page_index, page)?;
    let members = delete::members_of(document, page_index, reference, ctx.cancel)?;
    let refs: Vec<AnnotationRef> =
        members.iter().map(delete::member_ref).collect::<Result<_, _>>()?;
    let going: Vec<String> = refs.iter().map(AnnotationRef::key).collect();
    // A popup that goes without its annotation unlinks it.
    let unlinked: Vec<UnlinkedPopup> = members
        .iter()
        .zip(&refs)
        .filter(|(member, _)| member.get("subtype").and_then(Value::as_str) == Some("popup"))
        .filter_map(|(member, popup)| {
            let parent: AnnotationRef =
                serde_json::from_value(member.get("parent").cloned()?).ok()?;
            (!going.contains(&parent.key()))
                .then(|| UnlinkedPopup { reference: parent, popup: popup.clone() })
        })
        .collect();
    let capture = if members.is_empty() {
        None
    } else {
        let indexes = refs
            .iter()
            .map(|member| {
                crate::features::annotation::write::resolve::resolve_annotation_index_raw(
                    document, page_index, member,
                )
            })
            .collect::<Result<Vec<u32>, _>>()?;
        Some(document.export_annotations_raw(page_index, &indexes).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the capture of the annotations of page {} failed", page.get()),
            )
        })?)
    };
    let result = delete::delete_annotation(
        document,
        page_index,
        reference,
        &|members| authorize_delete(ctx, members),
        &session.write_stamp()?,
        ctx.cancel,
    )?;
    let reverse = ReverseStep::AnnotationRestore(AnnotationRestoreStep {
        page,
        members: result.meta.changed.clone(),
        capture,
        unlinked,
    });
    Ok(Done {
        item: ChangeItem::AnnotationsDelete { page, meta: result.meta },
        reverse: vec![reverse],
    })
}

/// Deletes what a create or a restore brought, when nobody changed it
/// since (`removeAnnotation`); left alone when its page is gone or anything
/// of it changed. Its reverse brings it back, so an undone create can be
/// redone.
pub fn remove_annotation(
    ctx: &mut ChangeContext<'_>,
    step: &AnnotationRemoveStep,
) -> Result<Done, EngineError> {
    let page = step.reference.page();
    let members = {
        let owned = ctx.session.document()?;
        let Ok(page_index) = owned.page_index(page) else {
            return left_alone(ctx, "annotations.delete");
        };
        delete::members_of(owned.document(), page_index, &step.reference, ctx.cancel)?
    };
    let key = |value: &Value| delete::member_ref(value).ok().map(|r| r.key());
    let unchanged = !members.is_empty()
        && members.len() == step.left.len()
        && members.iter().all(|member| {
            let member_key = key(member);
            step.left.iter().any(|left| {
                key(left) == member_key && super::applier::values_equal(Some(left), Some(member))
            })
        });
    if !unchanged {
        return left_alone(ctx, "annotations.delete");
    }
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    let done = write_delete(ctx, &step.reference)?;
    // A popup's parent, when it stays, gets back what the popup's create
    // changed on it.
    if let Some(before) = &step.parent {
        let session = &*ctx.session;
        let owned = session.document()?;
        let page_index = owned.page_index(page)?;
        let document = owned.document();
        if let Ok(at) = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
            document,
            page_index,
            &before.reference,
        ) && let Some(parent) = document.raw_annotation(page_index, at)
        {
            restore_parent(document, page_index, &parent, before)?;
        }
    }
    Ok(done)
}

/// Brings back what a delete removed, at the same numbers and positions
/// (`restoreAnnotations`). Skipped when its page is gone or it is back
/// already. It takes the authority its delete took over each owner of what
/// comes back.
pub fn restore_annotations(
    ctx: &mut ChangeContext<'_>,
    step: &AnnotationRestoreStep,
) -> Result<Done, EngineError> {
    let page = step.page;
    let Some(root) = step.members.first() else {
        return left_alone(ctx, "annotations.restore");
    };
    let session = &*ctx.session;
    let owned = session.document()?;
    let Ok(page_index) = owned.page_index(page) else {
        return left_alone(ctx, "annotations.restore");
    };
    let document = owned.document();
    let listed = |cancel| -> Result<Vec<Value>, EngineError> {
        crate::features::annotation::read::list::read_page_annotation_records(
            document, page_index, page, cancel,
        )?
        .iter()
        .map(|record| {
            serde_json::to_value(record)
                .map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))
        })
        .collect()
    };
    let key_of = |value: &Value| delete::member_ref(value).ok().map(|r| r.key());
    if listed(ctx.cancel)?.iter().any(|annotation| key_of(annotation) == Some(root.key())) {
        return left_alone(ctx, "annotations.restore");
    }
    let Some(capture) = &step.capture else {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "this delete can't be brought back: its record carries no capture of what it removed",
        ));
    };
    // An import runs inside a layer transaction. This engine opens none
    // around its writes yet (Task 8.30), so the import has its own.
    let own_transaction = !document.layer_in_transaction();
    if own_transaction && !document.layer_begin_transaction() {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFLayer_BeginTransaction refused",
        ));
    }
    if !document.import_annotations_raw(page_index, capture) {
        if own_transaction {
            document.layer_abort_transaction();
        }
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFPage_ImportAnnotsRaw refused a capture",
        ));
    }
    if own_transaction && !document.layer_commit_transaction() {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFLayer_CommitTransaction refused",
        ));
    }
    // A popup the delete took from an annotation that stayed links to it
    // again, while that annotation names no popup, as the delete left it.
    for link in &step.unlinked {
        let resolve = |reference| {
            crate::features::annotation::write::resolve::resolve_annotation_index_raw(
                document, page_index, reference,
            )
            .ok()
            .and_then(|at| document.raw_annotation(page_index, at))
        };
        if let (Some(parent), Some(popup)) = (resolve(&link.reference), resolve(&link.popup))
            && matches!(parent.linked_annotation("Popup"), Ok(None))
            && !matches!(parent.set_linked_annotation("Popup", &popup), Ok(true))
        {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "restoring /Popup failed",
            ));
        }
    }
    let after = listed(&CancelToken::never())?;
    let restored = step
        .members
        .iter()
        .map(|member| {
            after
                .iter()
                .find(|annotation| key_of(annotation) == Some(member.key()))
                .cloned()
                .ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("a restored annotation is missing: {}", member.key()),
                    )
                })
        })
        .collect::<Result<Vec<Value>, _>>()?;
    // What comes back is checked once it is read; a refusal takes the
    // import back with the rest of the change.
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    authorize_delete(ctx, &restored)?;
    let meta = AnnotationMutationMeta::of(&session.write_stamp()?, page, step.members.clone());
    Ok(Done {
        item: ChangeItem::AnnotationsRestore { page, annotations: restored.clone(), meta },
        reverse: vec![ReverseStep::AnnotationRemove(AnnotationRemoveStep {
            reference: root.clone(),
            left: restored,
            parent: None,
        })],
    })
}

/// `annotations.reorder`; its reverse puts them back beside their old
/// neighbours (`reorderAnnotations`).
pub fn reorder_annotations(
    ctx: &mut ChangeContext<'_>,
    page: PageObjectNumber,
    refs: &[AnnotationRef],
    position: &AnnotationPosition,
) -> Result<Done, EngineError> {
    let plan = {
        let owned = ctx.session.document()?;
        let page_index = owned.page_index(page)?;
        let stack = reorder::read_page_stack(owned.document(), page_index, page)?;
        reorder::plan_reorder(stack, AnnotationFamily::Annotations, refs, position)?
    };
    authorize_reorder(ctx, plan.family, refs)?;
    reorder::apply_reorder(ctx.session.document()?.document(), &plan)?;
    reordered(ctx, &plan, refs)
}

/// A reorder's item, and the step that puts its rows back (`reordered`).
pub(super) fn reordered(
    ctx: &ChangeContext<'_>,
    plan: &reorder::PlannedReorder,
    refs: &[AnnotationRef],
) -> Result<Done, EngineError> {
    let page = plan.stack.page;
    Ok(Done {
        item: reorder_item(ctx, plan.family, page, plan.after.clone(), refs)?,
        reverse: vec![ReverseStep::AnnotationReorder(AnnotationReorderStep {
            page,
            family: plan.family,
            refs: refs.to_vec(),
            before: plan.before.clone(),
            after: plan.after.clone(),
        })],
    })
}

/// A reorder's item: the family's new order on the page, and the rows that
/// moved (`reorderItem`): an annotation reorder's, or the form's
/// `forms.reorderWidgets` with the form's meta.
fn reorder_item(
    ctx: &ChangeContext<'_>,
    family: AnnotationFamily,
    page: PageObjectNumber,
    order: Vec<AnnotationRef>,
    moved: &[AnnotationRef],
) -> Result<ChangeItem, EngineError> {
    let stamp = ctx.session.write_stamp()?;
    if family == AnnotationFamily::Annotations {
        let meta = AnnotationMutationMeta::of(&stamp, page, moved.to_vec());
        return Ok(ChangeItem::AnnotationsReorder { page, order, meta });
    }
    let widgets = moved
        .iter()
        .map(|reference| {
            Ok(crate::features::forms::write::MetaWidget::Plain(
                crate::features::forms::snapshot::FormWidgetRef {
                    annot_object_number: annotation_object_number(ctx, reference)?,
                    page_object_number: page.get(),
                },
            ))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    Ok(ChangeItem::FormsReorderWidgets {
        page: crate::identity::PageRef::new(page),
        order,
        meta: crate::features::forms::write::form_mutation_meta(&stamp, Vec::new(), widgets, None),
    })
}

/// `objectNumberOf`: the object number of the annotation `reference` names
/// on its page (0 for an inline one).
pub(super) fn annotation_object_number(
    ctx: &ChangeContext<'_>,
    reference: &AnnotationRef,
) -> Result<u32, EngineError> {
    let owned = ctx.session.document()?;
    let page_index = owned.page_index(reference.page())?;
    let document = owned.document();
    let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
        document, page_index, reference,
    )?;
    Ok(document.raw_annotation(page_index, index).map_or(0, |a| a.object_number()))
}

/// A reorder may move these rows: each annotation's kind, or the form's
/// design (`authorizeReorder`).
fn authorize_reorder(
    ctx: &ChangeContext<'_>,
    family: AnnotationFamily,
    refs: &[AnnotationRef],
) -> Result<(), EngineError> {
    if family == AnnotationFamily::Widgets {
        return ctx.authority.require(DocCapability::DocFormsModify);
    }
    for reference in refs {
        read_annotation(ctx, reference)?;
    }
    if !refs.is_empty() {
        ctx.authority.require(DocCapability::DocAnnotateModify)?;
    }
    Ok(())
}

/// Puts the rows of a reorder back beside their old neighbours
/// (`reorderBack`). A row goes back only while it is where the reorder left
/// it, above the same row; it goes above the nearest row that was below it
/// before and is still there, or to the bottom.
pub fn reorder_back(
    ctx: &mut ChangeContext<'_>,
    step: &AnnotationReorderStep,
) -> Result<Done, EngineError> {
    let reports = match step.family {
        AnnotationFamily::Annotations => "annotations.reorder",
        AnnotationFamily::Widgets => "forms.reorderWidgets",
    };
    let read_stack = |ctx: &ChangeContext<'_>| -> Result<reorder::PageStack, EngineError> {
        let owned = ctx.session.document()?;
        let page_index = owned.page_index(step.page)?;
        reorder::read_page_stack(owned.document(), page_index, step.page)
    };
    let Ok(stack) = read_stack(ctx) else {
        return left_alone(ctx, reports);
    };
    let now = reorder::family_order(&stack, step.family);
    let back: Vec<AnnotationRef> = step
        .refs
        .iter()
        .filter(|reference| {
            let below = row_below(&now, reference);
            below.is_some() && below == row_below(&step.after, reference)
        })
        .cloned()
        .collect();
    if back.is_empty() {
        return left_alone(ctx, reports);
    }
    authorize_reorder(ctx, step.family, &back)?;

    let going: Vec<String> = back.iter().map(AnnotationRef::key).collect();
    let mut in_place: Vec<String> =
        now.iter().map(AnnotationRef::key).filter(|key| !going.contains(key)).collect();
    let mut current = stack;
    // Lowest first, so a row's old neighbour that moves too is back before
    // it.
    for (at, reference) in step.before.iter().enumerate() {
        if !going.contains(&reference.key()) {
            continue;
        }
        let neighbour =
            step.before[..at].iter().rev().find(|row| in_place.contains(&row.key())).cloned();
        let position = match neighbour {
            Some(after) => ListPosition::After { after },
            None => ListPosition::START,
        };
        let plan = reorder::plan_reorder(
            current,
            step.family,
            std::slice::from_ref(reference),
            &position,
        )?;
        reorder::apply_reorder(ctx.session.document()?.document(), &plan)?;
        in_place.push(reference.key());
        current = read_stack(ctx)?;
    }
    let after = reorder::family_order(&current, step.family);
    Ok(Done {
        item: reorder_item(ctx, step.family, step.page, after.clone(), &back)?,
        reverse: vec![ReverseStep::AnnotationReorder(AnnotationReorderStep {
            refs: back,
            before: now,
            after,
            ..step.clone()
        })],
    })
}

/// The key of the row right below `reference` in `order`: `Some(None)` at
/// the bottom, `None` when `reference` isn't in it (`rowBelow`).
fn row_below(order: &[AnnotationRef], reference: &AnnotationRef) -> Option<Option<String>> {
    let key = reference.key();
    let at = order.iter().position(|row| row.key() == key)?;
    Some(at.checked_sub(1).map(|below| order[below].key()))
}

/// Put back a popup's parent's `/Popup` and `/Open` as they were before the
/// popup's create linked them.
fn restore_parent(
    document: &pdfium::Document,
    page_index: u32,
    parent: &pdfium::Annotation<'_>,
    before: &crate::features::annotation::write::create::ParentBefore,
) -> Result<(), EngineError> {
    let refused = |what: &str| {
        EngineError::with_message(EngineErrorCode::Unknown, format!("restoring {what} failed"))
    };
    let earlier = before.popup.and_then(|number| {
        let count = document.raw_annotation_count(page_index).unwrap_or(0);
        (0..count)
            .filter_map(|i| document.raw_annotation(page_index, i))
            .find(|a| a.object_number() == number)
    });
    match earlier {
        Some(popup) => {
            if !matches!(parent.set_linked_annotation("Popup", &popup), Ok(true)) {
                return Err(refused("/Popup"));
            }
        }
        None => {
            parent.remove_key("Popup").map_err(|_| refused("/Popup"))?;
        }
    }
    match before.open {
        Some(open) => {
            if !matches!(parent.set_boolean_value("Open", open), Ok(true)) {
                return Err(refused("/Open"));
            }
        }
        None => {
            parent.remove_key("Open").map_err(|_| refused("/Open"))?;
        }
    }
    Ok(())
}

/// `authorizeAnnotationUpdate`: the update checked against the annotation's
/// owner as it reads, and a new group checked as a group assignment. A
/// group is never removed. Answers the actor the update stamps, or `None`
/// when there is no one to name.
fn authorize_update(
    ctx: &ChangeContext<'_>,
    current: &Map<String, Value>,
    patch_group: Option<&Value>,
) -> Result<Option<AnnotationActor>, EngineError> {
    let identity = &ctx.authority.identity;
    let owner = |name: &str| {
        current.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned)
    };
    let target = CollabTarget { user_id: owner("userId"), group_id: owner("groupId") };
    if let Some(grants) = &ctx.authority.grants
        && !grants.allows_collab(CollabAction::Update, &target)
    {
        return Err(forbidden("annotations:update"));
    }
    if patch_group.is_some_and(Value::is_null) && target.group_id.is_some() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "an annotation's group can't be removed",
        ));
    }
    let reassigning = patch_group
        .and_then(Value::as_str)
        .filter(|group| Some(*group) != target.group_id.as_deref())
        .map(str::to_owned);
    if let (Some(group), Some(grants)) = (&reassigning, &ctx.authority.grants)
        && !crate::auth::scope::resolver::check_set_group(
            CollabEntity::Annotations,
            group,
            identity.group_id.as_deref(),
            grants.scope(),
        )
    {
        return Err(forbidden(&format!("annotations:set-group:group={group}")));
    }
    let named = |value: Option<String>| value.filter(|v| !v.is_empty());
    let actor = AnnotationActor {
        display_name: named(identity.display_name.clone()),
        user_id: named(identity.user_id.clone()),
        group_id: reassigning,
    };
    let anyone =
        actor.user_id.is_some() || actor.group_id.is_some() || actor.display_name.is_some();
    Ok(anyone.then_some(actor))
}

/// The annotation `reference` names as it reads now, in the file's
/// coordinates (`readAnnotation`): `NotFound` when there is none. A widget
/// reads as the form reads its row.
pub(super) fn read_annotation(
    ctx: &ChangeContext<'_>,
    reference: &AnnotationRef,
) -> Result<Map<String, Value>, EngineError> {
    let owned = ctx.session.document()?;
    let page = reference.page();
    let page_index = owned.page_index(page)?;
    let document = owned.document();
    let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
        document, page_index, reference,
    )?;
    let annotation = document.raw_annotation(page_index, index).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "an annotation is not where it was")
    })?;
    crate::features::annotation::write::update::read_record_json(document, &annotation, page, index)
}

/// `annotations.update`. Its reverse puts back the fields it changed
/// (`updateAnnotation`).
pub fn update_annotation(
    ctx: &mut ChangeContext<'_>,
    reference: &AnnotationRef,
    patch: &Map<String, Value>,
    resources: &OpResources,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    let current = read_annotation(ctx, reference)?;
    if current.get("subtype").and_then(Value::as_str) == Some("widget") {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a widget is the form's: change its place and look with doc.forms.updateWidget",
        ));
    }
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    if let Some(expect) = expect {
        super::applier::assert_expected(at, &current, expect)?;
    }
    write_update(ctx, reference, patch.clone(), resources, "annotations.update")
}

/// An update of `reference` with `patch`, recording its reverse
/// (`writeUpdate`), reported as `reports`: an annotation's update, or a
/// widget's `forms.updateWidget`.
pub(super) fn write_update(
    ctx: &mut ChangeContext<'_>,
    reference: &AnnotationRef,
    patch: Map<String, Value>,
    resources: &OpResources,
    reports: &str,
) -> Result<Done, EngineError> {
    let before = read_annotation(ctx, reference)?;
    // A widget's patch as its field's family takes it (`fitWidgetToFamily`).
    let patch = if before.get("subtype").and_then(Value::as_str) == Some("widget") {
        let family: crate::features::forms::snapshot::FormFieldFamily =
            serde_json::from_value(before.get("fieldFamily").cloned().unwrap_or_default())
                .unwrap_or(crate::features::forms::snapshot::FormFieldFamily::Unknown);
        crate::features::forms::field::fit_widget_to_family(family, patch)?
    } else {
        patch
    };
    let actor = authorize_update(ctx, &before, patch.get("groupId"))?;
    let appearance = resources.appearance.as_ref().map(|r| r.bytes()).transpose()?;
    let file = resources.file.as_ref().map(|r| r.bytes()).transpose()?;
    let session = &*ctx.session;
    let owned = session.document()?;
    let page = reference.page();
    let page_index = owned.page_index(page)?;
    let context = AnnotationWriteContext { fonts: ctx.fonts, resources: None };
    let services =
        UpdateServices { context: &context, drawings: session.drawing_index()?, clock: ctx.clock };
    let written = patch.clone();
    let result = update(
        owned.document(),
        page_index,
        &session.write_stamp()?,
        UpdateRequest {
            reference: reference.clone(),
            patch,
            resources: DraftResources { appearance, file },
            actor,
        },
        &services,
        ctx.cancel,
    )?;
    let after = result.annotation.clone();
    let reverse = ReverseStep::ObjectsRevert(ObjectsRevertStep {
        reports: reports.to_owned(),
        objects: Vec::new(),
        subject: RevertSubject::Annotation { annotation: reference.clone() },
        appearance_changed: Some(result.appearance.changed),
        fallback: Some(values_changed(&before, &after, &written)),
    });
    Ok(Done { item: updated_item(ctx, reports, reference, after, result)?, reverse: vec![reverse] })
}

/// The item of an in-place update (`updatedItem`): an annotation's, with
/// its page; or a widget's form op, with the form's meta.
fn updated_item(
    ctx: &ChangeContext<'_>,
    reports: &str,
    reference: &AnnotationRef,
    after: Map<String, Value>,
    result: crate::features::annotation::write::update::AnnotationUpdateResult,
) -> Result<ChangeItem, EngineError> {
    use crate::features::forms::write::{MetaWidget, form_mutation_meta};
    let page = reference.page();
    if reports == "annotations.update" {
        return Ok(ChangeItem::AnnotationsUpdate {
            page,
            annotation: Value::Object(after),
            appearance: result.appearance,
            meta: result.meta,
            skipped: Vec::new(),
        });
    }
    if after.get("subtype").and_then(Value::as_str) != Some("widget") {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "a widget update read back no widget",
        ));
    }
    let changed = MetaWidget::Plain(crate::features::forms::snapshot::FormWidgetRef {
        annot_object_number: annotation_object_number(ctx, reference)?,
        page_object_number: page.get(),
    });
    let field: Option<crate::features::forms::snapshot::FormFieldRef> =
        serde_json::from_value(after.get("field").cloned().unwrap_or_default()).ok().flatten();
    // A move keeps the widget's look: its picture is the one it had.
    let redrawn = if result.appearance.changed { vec![changed.clone()] } else { Vec::new() };
    Ok(ChangeItem::FormsUpdateWidget {
        widget: Value::Object(after),
        appearance: result.appearance,
        meta: form_mutation_meta(
            &ctx.session.write_stamp()?,
            field.into_iter().collect(),
            vec![changed],
            Some(redrawn),
        ),
        skipped: Vec::new(),
    })
}

/// `valuesChanged`: the fields the patch named whose value changed, their
/// old values and the values the update left.
fn values_changed(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    patch: &Map<String, Value>,
) -> RevertFallback {
    let keys: Vec<&String> = patch
        .keys()
        .filter(|key| {
            *key != "subtype" && !super::applier::values_equal(before.get(*key), after.get(*key))
        })
        .collect();
    let pick = |from: &Map<String, Value>| {
        keys.iter()
            .filter_map(|key| from.get(*key).map(|value| ((*key).clone(), value.clone())))
            .collect::<Map<String, Value>>()
    };
    RevertFallback::Annotation { restore: pick(before), left: pick(after) }
}

/// Undoes an update (`revertAnnotation`): puts back, by value, each field it
/// changed that still holds what it left, and reports the rest skipped.
/// Left alone when the annotation is gone or none still shows the update.
pub fn revert_annotation(
    ctx: &mut ChangeContext<'_>,
    step: &ObjectsRevertStep,
) -> Result<Done, EngineError> {
    let RevertSubject::Annotation { annotation: reference } = &step.subject else {
        return left_alone(ctx, &step.reports);
    };
    let Ok(current) = read_annotation(ctx, reference) else {
        return left_alone(ctx, &step.reports);
    };
    // `annotationWriteCapability`: a widget is the form's.
    ctx.authority.require_unprotected(
        if current.get("subtype").and_then(Value::as_str) == Some("widget") {
            DocCapability::DocFormsModify
        } else {
            DocCapability::DocAnnotateModify
        },
    )?;
    // re-port (Task 8): 8.30 puts the dictionaries back exactly from their
    // captures while they read as the update left them; until then every
    // revert takes the value path below.
    let Some(RevertFallback::Annotation { restore, left }) = &step.fallback else {
        return left_alone(ctx, &step.reports);
    };
    let keys: Vec<&String> = left.keys().collect();
    let kept: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|key| super::applier::values_equal(current.get(*key), left.get(*key)))
        .collect();
    if kept.is_empty() {
        return left_alone(ctx, &step.reports);
    }
    // A widget's actions are kept as read (trees) and written as a patch.
    let widget = current.get("subtype").and_then(Value::as_str) == Some("widget");
    // A widget's actions are kept as read (trees) and written as a patch
    // (`widgetActionsOf`). A stored value that reads as no actions is a
    // broken record, refused rather than written as "remove them all".
    let writable = |key: &str, value: &Value| -> Result<Value, EngineError> {
        if key != "actions" || !widget {
            return Ok(value.clone());
        }
        let broken = |e: serde_json::Error| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("an update's recorded widget actions do not read: {e}"),
            )
        };
        let actions: Option<crate::features::annotation::read::base::AnnotationActions> =
            serde_json::from_value(value.clone()).map_err(broken)?;
        serde_json::to_value(crate::features::actions::write::widget_actions_of(actions.as_ref()))
            .map_err(broken)
    };
    let patch: Map<String, Value> = kept
        .iter()
        .filter_map(|key| {
            restore.get(*key).map(|value| Ok(((*key).clone(), writable(key, value)?)))
        })
        .collect::<Result<_, EngineError>>()?;
    let mut done = write_update(ctx, reference, patch, &OpResources::default(), &step.reports)?;
    let skipped: Vec<String> =
        keys.into_iter().filter(|key| !kept.contains(key)).cloned().collect();
    if let ChangeItem::AnnotationsUpdate { skipped: item_skipped, .. }
    | ChangeItem::FormsUpdateWidget { skipped: item_skipped, .. } = &mut done.item
    {
        *item_skipped = skipped;
    }
    Ok(done)
}
