//! The attachment ops and their reverse
//! (`services/src/features/changes/internal/attachmentChanges.ts`).
//!
//! A create's reverse deletes the file while it is still the file the create
//! made; a delete's reverse brings the file back while no file has its key.
//! Upstream guards the first with the file's canonical bytes and restores
//! the second from a raw capture, both of which the fork offers only on a
//! layer inside a layer transaction. This engine has neither yet (8.30), so
//! the guard is the file as it reads ([`AttachmentFileInfo`]) and the
//! capture the file as the API reads and writes it
//! ([`crate::features::files::attachments::AttachmentCapture`]).

use serde::{Deserialize, Serialize};

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::record::{AttachmentRemoveStep, AttachmentRestoreStep, ReverseStep};
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::files::attachments::{
    self as files, AttachmentFileBase, AttachmentRef, EmbeddedFileItem,
};
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
    let reverse = remove_step_of(attachment.clone());
    Ok(Done {
        item: ChangeItem::AttachmentsCreate {
            meta: meta_of(ctx, &attachment.reference)?,
            attachment,
        },
        reverse: vec![reverse],
    })
}

/// `attachments.delete`, captured first so an undo brings back the same
/// file.
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
    let now = files::find_attachment(ctx.session.document()?.document(), step.reference.as_key())?;
    if now.map(|entry| entry.file).as_ref() != Some(&step.left) {
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
    files::restore_attachment(document, key, &step.capture)?;
    let attachment = files::find_attachment(document, key)?.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("attachment '{key}' did not come back"),
        )
    })?;
    let reverse = remove_step_of(attachment.clone());
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
    let capture = files::capture_attachment(document, key)?;
    files::delete_attachment(document, key, ctx.cancel)?;
    Ok(Done {
        item: ChangeItem::AttachmentsDelete { meta: meta_of(ctx, reference)? },
        reverse: vec![ReverseStep::AttachmentsRestore(AttachmentRestoreStep {
            reference: reference.clone(),
            capture,
        })],
    })
}

/// The step that deletes `attachment` again while it reads as it does now
/// (`removeStepOf`).
fn remove_step_of(attachment: EmbeddedFileItem) -> ReverseStep {
    ReverseStep::AttachmentsRemove(AttachmentRemoveStep {
        reference: attachment.reference,
        left: attachment.file,
    })
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
