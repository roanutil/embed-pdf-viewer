//! The outline ops and the steps that reverse them
//! (`services/src/features/changes/internal/outlineChanges.ts`).
//!
//! `outline.create`, `outline.update`, `outline.move` and `outline.delete`,
//! each checked against `doc.outline.modify` as it runs. An update captures
//! the item's dictionary, with its `/A` and `/Dest`, so its undo puts it
//! back exactly; a delete captures the item and the items under it
//! (`EPDFOutline_ExportItemRawToOwnedBuffer`), so its undo brings back the
//! same objects at the same numbers.

use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, assert_expected, left_alone, values_equal};
use super::item::ChangeItem;
use super::objects::{
    PendingCapture, capture_after, capture_before, revert_objects_pending, unchanged_since,
};
use super::record::{
    CapturedObject, ObjectsRevertStep, OutlineLeftUnder, OutlineMoveStep, OutlineRemoveStep,
    OutlineReopenStep, OutlineRestoreStep, ReverseStep, RevertFallback, RevertSubject,
};
use super::transaction::in_layer_transaction;
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::outline::tree::OutlineTree;
use crate::features::outline::write::{OutlineFields, OutlineMutator, Updated};
use crate::features::outline::{OutlineItem, OutlinePosition, TOP, parent_ref};
use crate::identity::{OutlineItemRef, PageObjectNumber};
use crate::mutation::list_position::ListPosition;

/// The fields an update can change, each with a value a revert can put
/// back, in the order a fallback lists them (`FIELDS`).
const FIELDS: [&str; 6] = ["title", "target", "color", "bold", "italic", "open"];

/// What a capture of an item follows: its action, and its destination
/// (`TARGET_KEYS`).
const TARGET_KEYS: &[&str] = &["A", "Dest"];

fn mutator<'a>(ctx: &'a ChangeContext<'_>) -> Result<OutlineMutator<'a>, EngineError> {
    Ok(OutlineMutator::new(ctx.session.document()?.document(), ctx.session.write_stamp()?))
}

fn authorize(ctx: &ChangeContext<'_>) -> Result<(), EngineError> {
    ctx.authority.require(DocCapability::DocOutlineModify)
}

/// `outline.create`; its reverse deletes the item while nobody changed it
/// since (`createOutlineItem`).
pub fn create_outline_item(
    ctx: &mut ChangeContext<'_>,
    parent: Option<OutlineItemRef>,
    position: Option<&OutlinePosition>,
    data: &Map<String, Value>,
    object_number: Option<&serde_json::Number>,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let fields = OutlineFields::of(data)?;
    let title = fields.title.clone().ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "a title is a string")
            .with_details(serde_json::json!({ "field": "title" }))
    })?;
    let object_number = object_number.map(super::annotations::object_number_of).transpose()?;
    let place = mutator(ctx)?.plan_create(parent, position.unwrap_or(&ListPosition::END))?;
    if let Some(number) = object_number {
        ctx.session.claim_object_number(number)?;
    }
    let created = mutator(ctx)?.create(place, &title, &fields, object_number.unwrap_or(0))?;
    let reference = created.item.reference;
    Ok(Done {
        item: ChangeItem::OutlineCreate {
            item: item_value(&created.item)?,
            order: created.order,
            meta: created.meta,
        },
        reverse: vec![ReverseStep::OutlineRemove(OutlineRemoveStep {
            reference,
            left: guard_of(ctx, &[reference])?,
        })],
    })
}

/// `outline.update`; its reverse puts the item's dictionary back as it
/// was, or, when someone changed it since, the fields still holding what it
/// set (`updateOutlineItem`).
pub fn update_outline_item(
    ctx: &mut ChangeContext<'_>,
    reference: OutlineItemRef,
    patch: &Map<String, Value>,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let before = require_item(ctx, reference)?;
    if let Some(expect) = expect {
        assert_expected(at, &object(item_value(&before)?)?, expect)?;
    }
    write_update(ctx, reference, &OutlineFields::of(patch)?, &before)
}

/// `outline.move`; its reverse puts each item back beside the sibling it
/// followed, and opens again a parent it emptied that was open
/// (`moveOutlineItems`).
pub fn move_outline_items(
    ctx: &mut ChangeContext<'_>,
    refs: &[OutlineItemRef],
    parent: Option<OutlineItemRef>,
    position: &OutlinePosition,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let mutator = mutator(ctx)?;
    let tree = mutator.tree();
    let items: Vec<u32> = refs.iter().map(|reference| reference.object_number()).collect();
    let back = mutator.placements_of(&items, &tree);
    let reopens = reopens_of(&mutator, &tree, &items)?;
    let moved = mutator.move_items(refs, parent, position)?;
    let parent = parent.map_or(TOP, OutlineItemRef::object_number);
    let mut reverse = vec![ReverseStep::OutlineMove(OutlineMoveStep {
        placements: back,
        left: items.iter().map(|item| OutlineLeftUnder { item: *item, parent }).collect(),
    })];
    reverse.extend(reopens);
    Ok(Done { item: ChangeItem::OutlineMove { orders: moved.orders, meta: moved.meta }, reverse })
}

/// `outline.delete`; its reverse brings the item and the items under it
/// back (`deleteOutlineItem`).
pub fn delete_outline_item(
    ctx: &mut ChangeContext<'_>,
    reference: OutlineItemRef,
    expect: Option<&Map<String, Value>>,
    at: usize,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let before = require_item(ctx, reference)?;
    if let Some(expect) = expect {
        assert_expected(at, &object(item_value(&before)?)?, expect)?;
    }
    write_delete(ctx, reference)
}

/// Puts an item's dictionary back, or the fields that still show the
/// update (`revertOutlineItem`).
pub fn revert_outline_item(
    ctx: &mut ChangeContext<'_>,
    step: &ObjectsRevertStep,
) -> Result<Done, EngineError> {
    let RevertSubject::OutlineItem { outline_item: reference } = step.subject else {
        return left_alone(ctx, "outline.update");
    };
    let current = {
        let mutator = mutator(ctx)?;
        mutator.find(reference, &mutator.tree())?
    };
    let Some(current) = current else {
        return left_alone(ctx, "outline.update");
    };
    authorize(ctx)?;
    let document = ctx.session.document()?.document();
    if unchanged_since(document, &step.objects) {
        let mutator = mutator(ctx)?;
        let reverted = in_layer_transaction(document, || {
            let reverted = revert_objects_pending(document, &step.objects)?;
            // The item came back with its own /Count; the items above it
            // count again.
            mutator.count_again(reference)?;
            Ok(reverted)
        })?;
        let redo = capture_after(document, reverted)?;
        let Updated { item, meta } = mutator.updated(reference)?;
        let fallback = step.fallback.as_ref().map(|fallback| match fallback {
            RevertFallback::Outline { restore, left } => {
                RevertFallback::Outline { restore: left.clone(), left: restore.clone() }
            }
            other => other.clone(),
        });
        return Ok(Done {
            item: ChangeItem::OutlineUpdate { item: item_value(&item)?, meta, skipped: Vec::new() },
            reverse: vec![ReverseStep::ObjectsRevert(ObjectsRevertStep {
                objects: redo,
                fallback,
                ..step.clone()
            })],
        });
    }
    // Someone changed it since: put back by value what still shows the
    // update.
    let Some(RevertFallback::Outline { restore, left }) = &step.fallback else {
        return left_alone(ctx, "outline.update");
    };
    let now = object(item_value(&current)?)?;
    let keys: Vec<&String> = left.keys().collect();
    let kept: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|key| {
            values_equal(now.get(key.as_str()), left.get(key.as_str()))
                && writable(ctx, key, restore.get(key.as_str()))
        })
        .collect();
    if kept.is_empty() {
        return left_alone(ctx, "outline.update");
    }
    let patch: Map<String, Value> = kept
        .iter()
        .map(|key| ((*key).clone(), restore.get(key.as_str()).cloned().unwrap_or(Value::Null)))
        .collect();
    let mut done = write_update(ctx, reference, &OutlineFields::of(&patch)?, &current)?;
    let left_out: Vec<String> =
        keys.iter().filter(|key| !kept.contains(key)).map(|key| (*key).clone()).collect();
    if let ChangeItem::OutlineUpdate { skipped, .. } = &mut done.item {
        *skipped = left_out;
    }
    Ok(done)
}

/// Deletes what a create or a restore brought back, while the item and the
/// items under it are the same ones and read exactly as the change left
/// them (`removeOutlineItem`).
pub fn remove_outline_item(
    ctx: &mut ChangeContext<'_>,
    step: &OutlineRemoveStep,
) -> Result<Done, EngineError> {
    let tree = mutator(ctx)?.tree();
    let number = step.reference.object_number();
    if !tree.has(number) {
        return left_alone(ctx, "outline.delete");
    }
    authorize(ctx)?;
    let items = tree.subtree_of(number);
    let left: Vec<u32> = step.left.iter().map(|object| object.object_number).collect();
    if items != left || !unchanged_since(ctx.session.document()?.document(), &step.left) {
        return left_alone(ctx, "outline.delete");
    }
    write_delete(ctx, step.reference)
}

/// Brings back what a delete removed, while its parent is there and it
/// isn't back already (`restoreOutlineItem`).
pub fn restore_outline_item(
    ctx: &mut ChangeContext<'_>,
    step: &OutlineRestoreStep,
) -> Result<Done, EngineError> {
    let tree = mutator(ctx)?.tree();
    let number = step.reference.object_number();
    if tree.has(number) {
        return left_alone(ctx, "outline.restore");
    }
    if let Some(parent) = step.parent
        && !tree.has(parent.object_number())
    {
        return left_alone(ctx, "outline.restore");
    }
    authorize(ctx)?;
    let document = ctx.session.document()?.document();
    in_layer_transaction(document, || {
        if document.import_outline_item_raw(&step.capture) {
            Ok(())
        } else {
            Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFOutline_ImportItemRaw refused a capture",
            ))
        }
    })?;
    let restored = mutator(ctx)?.restored(number)?;
    let refs: Vec<OutlineItemRef> = restored.items.iter().map(|item| item.reference).collect();
    Ok(Done {
        item: ChangeItem::OutlineRestore {
            items: restored.items.iter().map(item_value).collect::<Result<_, _>>()?,
            order: restored.order,
            meta: restored.meta,
        },
        reverse: vec![ReverseStep::OutlineRemove(OutlineRemoveStep {
            reference: step.reference,
            left: guard_of(ctx, &refs)?,
        })],
    })
}

/// Puts items back where a move found them: each still under the parent
/// the move left it under, its old parent still there, and not under
/// itself (`moveOutlineItemsBack`).
pub fn move_outline_items_back(
    ctx: &mut ChangeContext<'_>,
    step: &OutlineMoveStep,
) -> Result<Done, EngineError> {
    let mutator = mutator(ctx)?;
    let tree = mutator.tree();
    let left_under = |item: u32| step.left.iter().find(|l| l.item == item).map(|l| l.parent);
    let placements: Vec<_> = step
        .placements
        .iter()
        .copied()
        .filter(|placement| {
            tree.parent_of(placement.item) == left_under(placement.item)
                && (placement.parent == TOP || tree.has(placement.parent))
                && !tree.is_at_or_under(placement.parent, placement.item)
        })
        .collect();
    if placements.is_empty() {
        return left_alone(ctx, "outline.move");
    }
    authorize(ctx)?;
    let items: Vec<u32> = placements.iter().map(|placement| placement.item).collect();
    let back = mutator.placements_of(&items, &tree);
    let reopens = reopens_of(&mutator, &tree, &items)?;
    let moved = mutator.place(&placements, &tree)?;
    let mut reverse = vec![ReverseStep::OutlineMove(OutlineMoveStep {
        placements: back,
        left: placements
            .iter()
            .map(|placement| OutlineLeftUnder { item: placement.item, parent: placement.parent })
            .collect(),
    })];
    reverse.extend(reopens);
    Ok(Done { item: ChangeItem::OutlineMove { orders: moved.orders, meta: moved.meta }, reverse })
}

/// Opens a parent again, as its step says (`reopenOutlineItem`).
pub fn reopen_outline_item(
    ctx: &mut ChangeContext<'_>,
    step: &OutlineReopenStep,
) -> Result<Done, EngineError> {
    let mutator = mutator(ctx)?;
    let tree = mutator.tree();
    let item = mutator.find(step.reference, &tree)?;
    let children = tree.children_of(step.reference.object_number());
    let same = children.len() == step.children.len()
        && children.iter().all(|child| step.children.contains(child));
    if !same || item.is_none_or(|item| item.open) {
        return left_alone(ctx, "outline.update");
    }
    authorize(ctx)?;
    let open = OutlineFields { open: Some(true), ..OutlineFields::default() };
    let Updated { item, meta } = mutator.update(step.reference, &open)?;
    // Its redo empties the parent again, which leaves it without an open
    // state.
    Ok(Done {
        item: ChangeItem::OutlineUpdate { item: item_value(&item)?, meta, skipped: Vec::new() },
        reverse: Vec::new(),
    })
}

/// An update, captured so it can be put back exactly (`writeUpdate`).
fn write_update(
    ctx: &mut ChangeContext<'_>,
    reference: OutlineItemRef,
    patch: &OutlineFields,
    before: &OutlineItem,
) -> Result<Done, EngineError> {
    let document = ctx.session.document()?.document();
    let pending = capture_before(document, &[(reference.object_number(), TARGET_KEYS)]);
    let Updated { item, meta } = mutator(ctx)?.update(reference, patch)?;
    let objects = capture_after(document, pending)?;
    let after = item_value(&item)?;
    let fallback = fallback_of(&object(item_value(before)?)?, &object(after.clone())?, patch);
    Ok(Done {
        item: ChangeItem::OutlineUpdate { item: after, meta, skipped: Vec::new() },
        reverse: vec![ReverseStep::ObjectsRevert(ObjectsRevertStep {
            reports: "outline.update".into(),
            objects,
            subject: RevertSubject::OutlineItem { outline_item: reference },
            appearance_changed: None,
            fallback: Some(fallback),
            rights: None,
        })],
    })
}

/// A delete, captured so an undo brings back the same objects, and opens
/// again the parent it emptied when it was open (`writeDelete`).
fn write_delete(
    ctx: &mut ChangeContext<'_>,
    reference: OutlineItemRef,
) -> Result<Done, EngineError> {
    let mutator = mutator(ctx)?;
    let tree = mutator.tree();
    let number = reference.object_number();
    let parent = tree.parent_of(number);
    let reopens = reopens_of(&mutator, &tree, &[number])?;
    let capture =
        ctx.session.document()?.document().export_outline_item_raw(number).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the capture of outline item {number} failed"),
            )
        })?;
    let meta = mutator.delete(reference)?;
    let mut reverse = vec![ReverseStep::OutlineRestore(OutlineRestoreStep {
        reference,
        parent: parent.and_then(parent_ref),
        capture,
    })];
    reverse.extend(reopens);
    Ok(Done { item: ChangeItem::OutlineDelete { meta }, reverse })
}

/// The parents that lose every child when `items` leave them, and are
/// open: each comes back closed with its children, so the undo opens it
/// again (`reopensOf`).
fn reopens_of(
    mutator: &OutlineMutator<'_>,
    tree: &OutlineTree,
    items: &[u32],
) -> Result<Vec<ReverseStep>, EngineError> {
    let mut parents: Vec<u32> = Vec::new();
    for item in items {
        let parent = tree.parent_of(*item).unwrap_or(TOP);
        if parent != TOP && !parents.contains(&parent) {
            parents.push(parent);
        }
    }
    let mut reopens = Vec::new();
    for parent in parents {
        let children = tree.children_of(parent);
        if !children.iter().all(|child| items.contains(child)) {
            continue;
        }
        let reference = OutlineItemRef::new(parent);
        if !mutator.find(reference, tree)?.is_some_and(|item| item.open) {
            continue;
        }
        reopens.push(ReverseStep::OutlineReopen(OutlineReopenStep {
            reference,
            children: children.to_vec(),
        }));
    }
    Ok(reopens)
}

/// The guard of items a change leaves: each dictionary as it reads now
/// (`guardOf`).
fn guard_of(
    ctx: &ChangeContext<'_>,
    refs: &[OutlineItemRef],
) -> Result<Vec<CapturedObject>, EngineError> {
    capture_after(
        ctx.session.document()?.document(),
        refs.iter()
            .map(|reference| PendingCapture {
                object_number: reference.object_number(),
                deep: Vec::new(),
                before: None,
            })
            .collect(),
    )
}

/// The item as it reads now; `NotFound` when the outline has no such item
/// (`requireItem`).
fn require_item(
    ctx: &ChangeContext<'_>,
    reference: OutlineItemRef,
) -> Result<OutlineItem, EngineError> {
    let mutator = mutator(ctx)?;
    mutator.find(reference, &mutator.tree())?.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no outline item {}", reference.object_number()),
        )
        .with_details(serde_json::json!({ "field": "ref" }))
    })
}

/// The fields an update changed: their values before, and the values it
/// left, each named by the patch (`fallbackOf`).
fn fallback_of(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    patch: &OutlineFields,
) -> RevertFallback {
    let named = |field: &str| match field {
        "title" => patch.title.is_some(),
        "target" => patch.target.is_some(),
        "color" => patch.color.is_some(),
        "bold" => patch.bold.is_some(),
        "italic" => patch.italic.is_some(),
        _ => patch.open.is_some(),
    };
    let changed: Vec<&str> = FIELDS
        .into_iter()
        .filter(|field| named(field) && !values_equal(before.get(*field), after.get(*field)))
        .collect();
    let pick = |from: &Map<String, Value>| -> Map<String, Value> {
        changed
            .iter()
            .map(|field| ((*field).to_owned(), from.get(*field).cloned().unwrap_or(Value::Null)))
            .collect()
    };
    RevertFallback::Outline { restore: pick(before), left: pick(after) }
}

/// Whether a value read back can be written again: a target another app
/// made may not be, nor one that goes to a page the document no longer has
/// (`writable`).
fn writable(ctx: &ChangeContext<'_>, field: &str, value: Option<&Value>) -> bool {
    let Some(target) = value.filter(|value| field == "target" && !value.is_null()) else {
        return true;
    };
    match target.get("kind").and_then(Value::as_str) {
        Some("goto") => target
            .pointer("/destination/page/objectNumber")
            .and_then(Value::as_u64)
            .and_then(|page| u32::try_from(page).ok())
            .and_then(|page| PageObjectNumber::new(page).ok())
            .is_some_and(|page| {
                ctx.session.document().is_ok_and(|document| document.page_index(page).is_ok())
            }),
        Some("uri") => true,
        Some("named") => target
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| ["NextPage", "PrevPage", "FirstPage", "LastPage"].contains(&name)),
        _ => false,
    }
}

fn item_value(item: &OutlineItem) -> Result<Value, EngineError> {
    serde_json::to_value(item).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("an outline item could not be serialized: {e}"),
        )
    })
}

fn object(value: Value) -> Result<Map<String, Value>, EngineError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => {
            Err(EngineError::with_message(EngineErrorCode::Unknown, "an outline item is an object"))
        }
    }
}
