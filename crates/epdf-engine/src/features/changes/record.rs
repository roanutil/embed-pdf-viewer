//! What the engine keeps of a change so it can undo it
//! (`services/src/features/changes/ChangeRecord.ts`).
//!
//! A record is who made the change and the steps that reverse it, in the
//! order they run: the reverses of its ops, in reverse op order. Each step is
//! guarded by what the change left: it applies where the document still
//! shows that, and is skipped, and reported, where it doesn't. Running a step
//! records the step that reverses it in turn, so the undo of an undo is a
//! redo.
//!
//! The metadata and attachment steps are here; each other family adds its
//! own step kinds with its ops ([`super::op::OP_OWNERS`]).

use serde::{Deserialize, Serialize};

use crate::features::annotation::read::file_attachment::AttachmentFileInfo;
use crate::features::files::attachments::{AttachmentCapture, AttachmentRef};

/// A change's record (`ChangeRecord`), and what crosses the wire as a
/// `ChangeRecordPayload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRecord {
    /// Who made the change: only they may undo it. `None`: anyone may.
    pub user_id: Option<String>,
    pub steps: Vec<ReverseStep>,
}

/// One step of a record (`ReverseStep`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ReverseStep {
    #[serde(rename = "metadata.revert")]
    MetadataRevert(MetadataRevertStep),
    #[serde(rename = "attachments.remove")]
    AttachmentsRemove(AttachmentRemoveStep),
    #[serde(rename = "attachments.restore")]
    AttachmentsRestore(AttachmentRestoreStep),
    #[serde(rename = "annotation.remove")]
    AnnotationRemove(AnnotationRemoveStep),
}

/// Deletes what a create made, with its thread and popups, while it all
/// reads as the change left it (`AnnotationRemoveStep`): undoing a create
/// never deletes an annotation someone changed since. `left` is each, as it
/// read in the file's coordinates, the annotation first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationRemoveStep {
    #[serde(rename = "ref")]
    pub reference: crate::identity::AnnotationRef,
    pub left: Vec<serde_json::Value>,
    /// For a popup's create: the annotation it shows, as it was before the
    /// popup was linked to it, put back when the popup goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<crate::features::annotation::write::create::ParentBefore>,
}

/// Deletes the embedded file a create or a restore brought, while it is
/// still that file, as the change left it (`AttachmentRemoveStep`): undoing
/// an upload never deletes a file someone put under its name since.
///
/// `left` is the file as it read ([`AttachmentFileInfo`]) where upstream
/// keeps its canonical bytes, which the fork exports only from a layer
/// (`features::changes::attachments`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRemoveStep {
    #[serde(rename = "ref")]
    pub reference: AttachmentRef,
    pub left: AttachmentFileInfo,
}

/// Brings back the file a delete removed, under its key
/// (`AttachmentRestoreStep`). Left alone when another file has the key now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRestoreStep {
    #[serde(rename = "ref")]
    pub reference: AttachmentRef,
    pub capture: AttachmentCapture,
}

/// Which metadata write a revert puts back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataOp {
    #[serde(rename = "metadata.update")]
    Update,
    #[serde(rename = "metadata.updateCustom")]
    UpdateCustom,
}

impl MetadataOp {
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Update => "metadata.update",
            Self::UpdateCustom => "metadata.updateCustom",
        }
    }
}

/// Puts back each metadata entry still holding what the change set
/// (`MetadataRevertStep`): `restore` is the entries' old values (`null` for
/// a custom key that wasn't there), `left` the values the change left. Both
/// are patches of the op's kind, keyed by the patch's own keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataRevertStep {
    pub op: MetadataOp,
    pub restore: serde_json::Map<String, serde_json::Value>,
    pub left: serde_json::Map<String, serde_json::Value>,
}

/// The bytes of captures a record holds, which the ledger budgets
/// (`recordBytes`). A metadata step captures no bytes, and neither does an
/// attachment remove here: its guard is a read, not canonical bytes.
pub fn record_bytes(record: &ChangeRecord) -> usize {
    record
        .steps
        .iter()
        .map(|step| match step {
            ReverseStep::MetadataRevert(_)
            | ReverseStep::AttachmentsRemove(_)
            | ReverseStep::AnnotationRemove(_) => 0,
            ReverseStep::AttachmentsRestore(step) => {
                step.capture.bytes.as_ref().map_or(0, Vec::len)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_crosses_the_wire_as_its_steps() {
        let record = ChangeRecord {
            user_id: Some("ann".into()),
            steps: vec![ReverseStep::MetadataRevert(MetadataRevertStep {
                op: MetadataOp::Update,
                restore: serde_json::json!({ "title": null }).as_object().unwrap().clone(),
                left: serde_json::json!({ "title": "T" }).as_object().unwrap().clone(),
            })],
        };
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "userId": "ann", "steps": [{ "kind": "metadata.revert",
                "op": "metadata.update", "restore": { "title": null }, "left": { "title": "T" } }] })
        );
        assert_eq!(serde_json::from_value::<ChangeRecord>(wire).unwrap(), record);
        assert_eq!(record_bytes(&record), 0);
    }
}
