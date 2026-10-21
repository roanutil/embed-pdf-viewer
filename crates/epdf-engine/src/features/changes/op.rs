//! The op table: what a change is made of (`core/src/mutation/Change.ts`).
//!
//! [`ChangeOp`] holds the op types this engine runs. Every other op type of
//! upstream's `ChangeOp` union is in [`OP_OWNERS`] with the sub-task that
//! registers it; a change naming one is refused with `NotImplemented` before
//! any op runs. An owner adds its variant to [`ChangeOp`], its arm to
//! [`super::applier`]'s `run_op` and to the page-space conversions
//! (`page_space::boundary::change_op_in_file_space` and
//! `change_item_in_page_space`), and removes its rows here. The matches
//! have no catch-all, so a new variant does not compile until it is placed.

use serde::{Deserialize, Serialize};

use crate::dto::{CustomMetadataPatch, MetadataPatch};
use crate::error::{EngineError, EngineErrorCode};

/// One change (`Change`): its ops in order, all of them or none; or the
/// reverse of an earlier change, named by its `opId`.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Ops(Vec<ChangeOp>),
    UndoOf(String),
}

/// One op of a change: exactly the parameters of its single verb
/// (`ChangeOp`). `expect` says what the op assumes the document holds now;
/// when it doesn't, the whole change is refused with `ChangeConflict`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChangeOp {
    #[serde(rename = "metadata.update", rename_all = "camelCase")]
    MetadataUpdate {
        patch: Box<MetadataPatch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<Box<MetadataPatch>>,
    },
    #[serde(rename = "metadata.updateCustom", rename_all = "camelCase")]
    MetadataUpdateCustom { patch: CustomMetadataPatch },
}

impl ChangeOp {
    /// The op's `type`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::MetadataUpdate { .. } => "metadata.update",
            Self::MetadataUpdateCustom { .. } => "metadata.updateCustom",
        }
    }
}

/// The op types of upstream's `ChangeOp` union this engine does not run yet,
/// each with the sub-task that registers it. The one list the owners look
/// in: each removes its rows when it adds its variant to [`ChangeOp`].
pub const OP_OWNERS: &[(&str, &str)] = &[
    // 8.8: attachment writes (c2d7a5194).
    ("attachments.create", "8.8"),
    ("attachments.delete", "8.8"),
    // 8.14 to 8.16: the annotation ops.
    ("annotations.create", "8.14"),
    ("annotations.update", "8.15"),
    ("annotations.delete", "8.16"),
    ("annotations.reorder", "8.16"),
    // 8.18: page structure (119685ea4, b69c40377).
    ("pages.rotate", "8.18"),
    ("pages.reorder", "8.18"),
    ("pages.delete", "8.18"),
    ("pages.insertBlank", "8.18"),
    ("pages.insert", "8.18"),
    // 8.20: form values and effects (98e5c270c).
    ("forms.setValue", "8.20"),
    ("forms.setDisplay", "8.20"),
    ("forms.setAppearanceText", "8.20"),
    ("forms.reset", "8.20"),
    // 8.21: the other form ops.
    ("forms.create", "8.21"),
    ("forms.update", "8.21"),
    ("forms.delete", "8.21"),
    ("forms.addWidget", "8.21"),
    ("forms.removeWidget", "8.21"),
    ("forms.deleteWidget", "8.21"),
    ("forms.reorderCalculations", "8.21"),
    ("forms.reorderWidgets", "8.21"),
    ("forms.updateWidget", "8.21"),
    // 8.22: the imports, which only their own verbs run as a one-op change
    // (`RecordedOp`); `document.apply` refuses them (`assertNoImport`).
    ("annotations.import", "8.22"),
    ("forms.import", "8.22"),
    ("forms.importValues", "8.22"),
    // 8.24: the outline.
    ("outline.create", "8.24"),
    ("outline.update", "8.24"),
    ("outline.move", "8.24"),
    ("outline.delete", "8.24"),
    // 8.26: a signature field's appearance.
    ("forms.setSignatureAppearance", "8.26"),
];

/// The imports `document.apply` refuses, with the verb that runs each
/// (`assertNoImport`).
const IMPORT_VERBS: &[(&str, &str)] = &[
    ("annotations.import", "doc.annotations.import"),
    ("forms.import", "doc.forms.import"),
    ("forms.importValues", "doc.forms.importValues"),
];

/// `change` as the wire spells it: `{ ops }` or `{ undoOf }`. Every op is
/// read before any runs, so a change naming an op this engine does not run,
/// or an op of the wrong shape, is refused whole.
pub fn parse_change(change: &serde_json::Value) -> Result<Change, EngineError> {
    let object = change.as_object().ok_or_else(|| malformed("a change is an object"))?;
    match (object.get("ops"), object.get("undoOf")) {
        (Some(ops), None) => parse_ops(ops).map(Change::Ops),
        (None, Some(serde_json::Value::String(op_id))) => Ok(Change::UndoOf(op_id.clone())),
        (None, Some(_)) => Err(malformed("undoOf is an opId")),
        _ => Err(malformed("a change has exactly one of ops or undoOf")),
    }
}

/// A change's ops, in order.
pub fn parse_ops(ops: &serde_json::Value) -> Result<Vec<ChangeOp>, EngineError> {
    let ops = ops.as_array().ok_or_else(|| malformed("ops is a list"))?;
    if let Some((_, verb)) = ops.iter().find_map(|op| {
        let kind = op.get("type").and_then(serde_json::Value::as_str)?;
        IMPORT_VERBS.iter().find(|(import, _)| *import == kind)
    }) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("doc.apply takes no import: import a bundle with {verb}"),
        ));
    }
    ops.iter().enumerate().map(|(at, op)| parse_op(at, op)).collect()
}

fn parse_op(at: usize, op: &serde_json::Value) -> Result<ChangeOp, EngineError> {
    let kind = op
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| malformed(format!("op {at} has no type")))?;
    if let Some((_, owner)) = OP_OWNERS.iter().find(|(name, _)| *name == kind) {
        return Err(EngineError::with_message(
            EngineErrorCode::NotImplemented,
            format!("the {kind} op is not ported yet (Task {owner})"),
        )
        .with_details(serde_json::json!({ "opIndex": at, "type": kind })));
    }
    serde_json::from_value(op.clone())
        .map_err(|e| malformed(format!("op {at} ({kind}) is not the shape it takes: {e}")))
}

fn malformed(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::WireFormat, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_its_ops_or_an_undo() {
        let ops = parse_change(&serde_json::json!({ "ops": [
            { "type": "metadata.update", "patch": { "title": "T" } },
            { "type": "metadata.updateCustom", "patch": { "Department": null } },
        ] }))
        .unwrap();
        let Change::Ops(ops) = ops else { panic!("ops") };
        assert_eq!(
            ops.iter().map(ChangeOp::type_name).collect::<Vec<_>>(),
            ["metadata.update", "metadata.updateCustom"]
        );
        assert_eq!(
            parse_change(&serde_json::json!({ "undoOf": "op-1" })).unwrap(),
            Change::UndoOf("op-1".into())
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({ "ops": [], "undoOf": "op-1" }),
            serde_json::json!({ "undoOf": 3 }),
            serde_json::json!("x"),
        ] {
            assert_eq!(parse_change(&bad).unwrap_err().code, EngineErrorCode::WireFormat, "{bad}");
        }
    }

    #[test]
    fn an_op_another_task_registers_refuses_the_whole_change_naming_its_owner() {
        let refused = parse_ops(&serde_json::json!([
            { "type": "metadata.update", "patch": {} },
            { "type": "pages.rotate", "pages": [], "rotation": 90 },
        ]))
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::NotImplemented);
        assert!(refused.message.contains("8.18"), "{refused}");
        assert_eq!(refused.details.unwrap()["opIndex"], 1);
    }

    #[test]
    fn an_import_is_refused_as_its_own_verbs() {
        let refused =
            parse_ops(&serde_json::json!([{ "type": "forms.importValues", "bundle": {} }]))
                .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert!(refused.message.contains("doc.forms.importValues"), "{refused}");
    }

    #[test]
    fn an_unknown_or_malformed_op_is_a_wire_format_error() {
        for op in [
            serde_json::json!({ "type": "metadata.rename" }),
            serde_json::json!({ "type": "metadata.update" }),
            serde_json::json!({ "type": "metadata.update", "patch": { "custom": {} } }),
            serde_json::json!({ "patch": {} }),
        ] {
            let refused = parse_ops(&serde_json::json!([op])).unwrap_err();
            assert_eq!(refused.code, EngineErrorCode::WireFormat, "{op}");
        }
    }

    #[test]
    fn every_owned_op_is_named_once() {
        let mut names: Vec<&str> = OP_OWNERS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OP_OWNERS.len());
        for ported in ["metadata.update", "metadata.updateCustom"] {
            assert!(!names.contains(&ported), "{ported} is ported and still listed");
        }
    }
}
