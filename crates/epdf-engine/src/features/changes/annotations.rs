//! The annotation ops and their reverse
//! (`services/src/features/changes/internal/annotationChanges.ts`).
//!
//! `annotations.create` is here; its reverse deletes what it made while it
//! still reads as the create left it. Task 8.15 adds the update and 8.16
//! the delete and the reorder, with the restore that undoes a delete: until
//! then the delete an undo of a create runs leaves nothing to redo it with.

use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::op::OpResources;
use super::record::{AnnotationRemoveStep, ReverseStep};
use crate::auth::scope::{CollabAction, CollabEntity, CollabTarget, DocCapability};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::write::context::AnnotationWriteContext;
use crate::features::annotation::write::create::{
    AnnotationMutationMeta, CreateRequest, CreateServices, create_annotation as create,
};
use crate::features::annotation::write::draft::AnnotationActor;
use crate::features::annotation::write::prepare::DraftResources;
use crate::identity::{AnnotationRef, PageObjectNumber};

fn forbidden(what: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Forbidden, format!("permission denied: {what}"))
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
fn object_number_of(number: &serde_json::Number) -> Result<u32, EngineError> {
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

/// The key two refs to one annotation share (`annotationKey`).
fn key_of(value: &Value) -> Option<String> {
    let reference: AnnotationRef = serde_json::from_value(value.clone()).ok()?;
    Some(match reference {
        AnnotationRef::ObjectNumber { page, annot } => format!("{}:o{annot}", page.get()),
        AnnotationRef::BaseIndex { page, base_index } => format!("{}:b{base_index}", page.get()),
        AnnotationRef::Nm { page, nm } => format!("{}:n{nm}", page.get()),
        AnnotationRef::Index { page, index, .. } => format!("{}:i{index}", page.get()),
    })
}

/// `deletedWith`: what deleting `reference` deletes with it, its replies
/// and their popups and its own, children before their parent and a popup
/// before what it shows, the annotation itself last. Empty when nothing on
/// the page is `reference`.
fn deleted_with(annotations: &[Value], reference: &Value) -> Vec<Value> {
    let keys: Vec<Option<String>> =
        annotations.iter().map(|a| a.get("ref").and_then(key_of)).collect();
    let find =
        |key: Option<String>| key.and_then(|key| keys.iter().position(|k| *k == Some(key.clone())));
    let Some(target) = find(key_of(reference)) else { return Vec::new() };
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); annotations.len()];
    let mut popups: Vec<Vec<usize>> = vec![Vec::new(); annotations.len()];
    for (at, annotation) in annotations.iter().enumerate() {
        if let Some(parent) = find(annotation.pointer("/reply/to").and_then(key_of))
            && parent != at
        {
            children[parent].push(at);
        }
        let is_popup = annotation.get("subtype").and_then(Value::as_str) == Some("popup");
        if is_popup
            && let Some(shows) = find(annotation.get("parent").and_then(key_of))
            && shows != at
        {
            popups[shows].push(at);
        }
        if let Some(own) = find(annotation.get("popup").and_then(key_of))
            && own != at
            && annotations[own].get("subtype").and_then(Value::as_str) == Some("popup")
        {
            popups[at].push(own);
        }
    }
    let mut members = Vec::new();
    let mut seen = vec![false; annotations.len()];
    fn visit(
        at: usize,
        children: &[Vec<usize>],
        popups: &[Vec<usize>],
        seen: &mut [bool],
        members: &mut Vec<usize>,
    ) {
        if seen[at] {
            return;
        }
        seen[at] = true;
        for child in &children[at] {
            visit(*child, children, popups, seen, members);
        }
        for popup in &popups[at] {
            if seen[*popup] {
                continue;
            }
            seen[*popup] = true;
            members.push(*popup);
        }
        members.push(at);
    }
    visit(target, &children, &popups, &mut seen, &mut members);
    members.into_iter().map(|at| annotations[at].clone()).collect()
}

/// Deletes what a create made while it reads as the change left it
/// (`removeAnnotation`); left alone when its page is gone or anything of it
/// changed.
pub fn remove_annotation(
    ctx: &mut ChangeContext<'_>,
    step: &AnnotationRemoveStep,
) -> Result<Done, EngineError> {
    let page = step.reference.page();
    let session = &*ctx.session;
    let owned = session.document()?;
    let Ok(page_index) = owned.page_index(page) else {
        return left_alone(ctx, "annotations.delete");
    };
    let document = owned.document();
    let listed = crate::features::annotation::read::list::read_page_annotation_records(
        document, page_index, page, ctx.cancel,
    )?;
    let annotations: Vec<Value> = listed
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))?;
    let reference = serde_json::to_value(step.reference.clone())
        .map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))?;
    let members = deleted_with(&annotations, &reference);
    let unchanged = !members.is_empty()
        && members.len() == step.left.len()
        && members.iter().all(|member| {
            let key = member.get("ref").and_then(key_of);
            step.left.iter().any(|left| left.get("ref").and_then(key_of) == key && left == member)
        });
    if !unchanged {
        return left_alone(ctx, "annotations.delete");
    }
    ctx.authority.require_unprotected(DocCapability::DocAnnotateModify)?;
    // Highest place first, so each removal leaves the others where they are.
    let mut places: Vec<(u32, AnnotationRef)> = Vec::with_capacity(members.len());
    for member in &members {
        let reference: AnnotationRef =
            serde_json::from_value(member.get("ref").cloned().unwrap_or_default())
                .map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))?;
        let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
            document, page_index, &reference,
        )?;
        places.push((index, reference));
    }
    let mut order: Vec<&(u32, AnnotationRef)> = places.iter().collect();
    order.sort_by_key(|place| std::cmp::Reverse(place.0));
    for (index, _) in order {
        if !document.remove_annotation_raw(page_index, *index) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFPage_RemoveAnnotRaw returned false",
            ));
        }
    }
    // A popup's parent, when it stays, gets back what the popup's create
    // changed on it.
    if let Some(before) = &step.parent
        && !places.iter().any(|(_, r)| *r == before.reference)
        && let Ok(at) = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
            document,
            page_index,
            &before.reference,
        )
        && let Some(parent) = document.raw_annotation(page_index, at)
    {
        restore_parent(document, page_index, &parent, before)?;
    }
    // The annotation first, then what went with it.
    let mut changed = vec![step.reference.clone()];
    changed.extend(places.into_iter().map(|(_, r)| r).filter(|r| *r != step.reference));
    let meta = AnnotationMutationMeta::of(&session.write_stamp()?, page, changed);
    Ok(Done { item: ChangeItem::AnnotationsDelete { page, meta }, reverse: Vec::new() })
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
