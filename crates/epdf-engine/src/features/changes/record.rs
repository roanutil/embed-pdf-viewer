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
//! Only the metadata step is here; each family adds its own step kinds with
//! its ops ([`super::op::OP_OWNERS`]).

use serde::{Deserialize, Serialize};

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
/// (`recordBytes`). A metadata step captures no bytes.
pub fn record_bytes(record: &ChangeRecord) -> usize {
    record
        .steps
        .iter()
        .map(|step| match step {
            ReverseStep::MetadataRevert(_) => 0,
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
