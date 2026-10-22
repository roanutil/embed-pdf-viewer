//! The attachment ops and their reverse
//! (`services/src/features/changes/internal/attachmentChanges.ts`).
//!
//! A create's reverse deletes the file while it is still the file the create
//! made; a delete's reverse brings the file back while no file has its key.
//! The guard is the file's canonical bytes
//! (`EPDFAttachment_ExportCanonicalToOwnedBuffer`), and the restore imports
//! the fork's raw capture of what the delete removed
//! (`EPDFAttachment_ExportRawToOwnedBuffer`, `EPDFAttachment_ImportRaw`), in
//! the change's layer transaction: the same file specification at the same
//! object number, its streams, and its `/Params` as they were.

use serde::{Deserialize, Serialize};

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::record::{AttachmentRemoveStep, AttachmentRestoreStep, ReverseStep};
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::files::attachments::{self as files, AttachmentFileBase, AttachmentRef};
use crate::mutation::MutationMeta;

/// The `meta` of an attachment write: the attachments it created or deleted
/// (`AttachmentMutationMeta`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMutationMeta {
    #[serde(flatten)]
    pub meta: MutationMeta,
    pub changed: Vec<AttachmentRef>,
}

/// `attachments.create`; its reverse deletes the file while it is still the
/// file this made.
pub fn create_attachment(
    ctx: &mut ChangeContext<'_>,
    file: &AttachmentFileBase,
    bytes: Option<&[u8]>,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocAttachmentsModify)?;
    let document = ctx.session.document()?.document();
    let attachment = files::create_attachment(document, file, bytes, ctx.clock, ctx.cancel)?;
    let reverse = remove_step_of(ctx, &attachment.reference)?;
    Ok(Done {
        item: ChangeItem::AttachmentsCreate {
            meta: meta_of(ctx, &attachment.reference)?,
            attachment,
        },
        reverse: vec![reverse],
    })
}

/// `attachments.delete`, captured in its transaction so an undo brings back
/// the same file.
pub fn delete_attachment(
    ctx: &mut ChangeContext<'_>,
    reference: &AttachmentRef,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocAttachmentsModify)?;
    write_delete(ctx, reference)
}

/// Deletes a file a create or a restore brought, while it reads as the
/// change left it (`removeAttachment`).
pub fn remove_attachment(
    ctx: &mut ChangeContext<'_>,
    step: &AttachmentRemoveStep,
) -> Result<Done, EngineError> {
    let now =
        ctx.session.document()?.document().export_attachment_canonical(step.reference.as_key());
    if now.as_ref() != Some(&step.left) {
        return left_alone(ctx, "attachments.delete");
    }
    ctx.authority.require(DocCapability::DocAttachmentsModify)?;
    write_delete(ctx, &step.reference)
}

/// Brings back a file a delete removed, while no file has its key
/// (`restoreAttachment`).
pub fn restore_attachment(
    ctx: &mut ChangeContext<'_>,
    step: &AttachmentRestoreStep,
) -> Result<Done, EngineError> {
    let key = step.reference.as_key();
    let document = ctx.session.document()?.document();
    if files::find_attachment(document, key)?.is_some() {
        return left_alone(ctx, "attachments.restore");
    }
    ctx.authority.require(DocCapability::DocAttachmentsModify)?;
    if !document.import_attachment_raw(&step.capture) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAttachment_ImportRaw refused a capture",
        ));
    }
    let attachment = files::find_attachment(document, key)?.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("attachment '{key}' did not come back"),
        )
    })?;
    let reverse = remove_step_of(ctx, &step.reference)?;
    Ok(Done {
        item: ChangeItem::AttachmentsRestore { meta: meta_of(ctx, &step.reference)?, attachment },
        reverse: vec![reverse],
    })
}

/// Deletes the file under `reference`, captured first so its restore brings
/// it back (`writeDelete`).
fn write_delete(
    ctx: &mut ChangeContext<'_>,
    reference: &AttachmentRef,
) -> Result<Done, EngineError> {
    let key = reference.as_key();
    let document = ctx.session.document()?.document();
    if files::find_attachment(document, key)?.is_none() {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no embedded file with key '{key}' in the document"),
        )
        .with_details(serde_json::json!({ "field": "ref" })));
    }
    let capture = document.export_attachment_raw(key).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the capture of attachment '{key}' failed"),
        )
    })?;
    files::delete_attachment(document, key, ctx.cancel)?;
    Ok(Done {
        item: ChangeItem::AttachmentsDelete { meta: meta_of(ctx, reference)? },
        reverse: vec![ReverseStep::AttachmentsRestore(AttachmentRestoreStep {
            reference: reference.clone(),
            capture,
        })],
    })
}

/// The step that deletes the file under `reference` again while it reads as
/// it does now, in canonical bytes (`removeStepOf`).
fn remove_step_of(
    ctx: &ChangeContext<'_>,
    reference: &AttachmentRef,
) -> Result<ReverseStep, EngineError> {
    let key = reference.as_key();
    let left =
        ctx.session.document()?.document().export_attachment_canonical(key).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("attachment '{key}' is not there to guard"),
            )
        })?;
    Ok(ReverseStep::AttachmentsRemove(AttachmentRemoveStep { reference: reference.clone(), left }))
}

/// A write's meta, naming the file it changed (`AttachmentMutator.meta`).
fn meta_of(
    ctx: &ChangeContext<'_>,
    reference: &AttachmentRef,
) -> Result<AttachmentMutationMeta, EngineError> {
    Ok(AttachmentMutationMeta {
        meta: MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?),
        changed: vec![reference.clone()],
    })
}
