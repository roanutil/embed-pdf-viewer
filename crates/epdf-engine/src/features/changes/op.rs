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
use crate::features::files::attachments::{AttachmentFileBase, AttachmentRef};

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
        /// The patch's keys in the order the wire sent them: what the op's
        /// reverse lists, and an undo's `skipped` follows
        /// (`Object.keys(op.patch)`). Not part of the op's identity.
        #[serde(skip)]
        keys: PatchKeys,
    },
    #[serde(rename = "metadata.updateCustom", rename_all = "camelCase")]
    MetadataUpdateCustom {
        patch: CustomMetadataPatch,
        /// As above: the custom map is a `BTreeMap`, which sorts its keys.
        #[serde(skip)]
        keys: PatchKeys,
    },
    /// A file embedded in the document (`/EmbeddedFiles`), under its name:
    /// the name is its key, and a name another file has is refused.
    #[serde(rename = "attachments.create", rename_all = "camelCase")]
    AttachmentsCreate {
        file: AttachmentFileBase,
        /// Its bytes, as the `file` resource.
        #[serde(default)]
        resources: OpResources,
    },
    #[serde(rename = "attachments.delete")]
    AttachmentsDelete {
        #[serde(rename = "ref")]
        reference: AttachmentRef,
    },
    /// An annotation on `page`, from its data (`AnnotationDraft`) and the
    /// bytes beside it, at `objectNumber` when given. The data stays the
    /// JSON object the caller sent: a create checks it field by field
    /// (`features::annotation::write::prepare`).
    #[serde(rename = "annotations.create", rename_all = "camelCase")]
    AnnotationsCreate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        data: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_number: Option<u32>,
        #[serde(default, skip_serializing_if = "OpResources::is_empty")]
        resources: OpResources,
    },
}

/// The bytes beside an op, by role (`WireAnnotationResources`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OpResources {
    /// What a stamp shows: PNG, JPEG or a one-page PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<ResourceBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ResourceBytes>,
}

impl OpResources {
    pub fn is_empty(&self) -> bool {
        self.appearance.is_none() && self.file.is_none()
    }

    /// Each role's bytes from the request's payloads, by the payload each
    /// names.
    pub fn resolve(&mut self, payloads: &[Vec<u8>]) -> Result<(), EngineError> {
        for slot in [&mut self.appearance, &mut self.file] {
            if let Some(ResourceBytes::Payload(index)) = slot {
                let bytes = payloads.get(*index).ok_or_else(|| {
                    malformed(format!(
                        "resource payload {index} is missing: the request carries {}",
                        payloads.len()
                    ))
                })?;
                *slot = Some(ResourceBytes::Bytes(bytes.clone()));
            }
        }
        Ok(())
    }
}

/// One resource's bytes. On this wire a resource names the request payload
/// that holds it, `{ "payload": n }`, as an annotation write's resources do;
/// the host puts the bytes in before the change runs or is fingerprinted
/// ([`resolve_payloads`]), and they then fingerprint as upstream's typed
/// array does, `{ "$bytes": ... }`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceBytes {
    Payload(usize),
    Bytes(Vec<u8>),
}

impl Serialize for ResourceBytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        match self {
            Self::Payload(index) => map.serialize_entry("payload", index)?,
            Self::Bytes(bytes) => {
                map.serialize_entry("$bytes", &super::fingerprint::bytes_token(bytes))?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ResourceBytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PayloadRef {
            payload: usize,
        }
        Ok(Self::Payload(PayloadRef::deserialize(d)?.payload))
    }
}

impl ResourceBytes {
    /// The bytes, once the host put them in.
    pub fn bytes(&self) -> Result<&[u8], EngineError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Payload(index) => {
                Err(malformed(format!("resource payload {index} was never put in")))
            }
        }
    }
}

/// Put each op's resource bytes in from the request's payloads, by the
/// payload each names.
pub fn resolve_payloads(change: &mut Change, payloads: &[Vec<u8>]) -> Result<(), EngineError> {
    let Change::Ops(ops) = change else {
        return Ok(());
    };
    for op in ops {
        match op {
            ChangeOp::AttachmentsCreate { resources, .. }
            | ChangeOp::AnnotationsCreate { resources, .. } => resources.resolve(payloads)?,
            ChangeOp::MetadataUpdate { .. }
            | ChangeOp::MetadataUpdateCustom { .. }
            | ChangeOp::AttachmentsDelete { .. } => {}
        }
    }
    Ok(())
}

/// A patch's keys in wire order. Compares equal whatever it holds, so two
/// ops with the same patch are the same op.
#[derive(Debug, Clone, Default)]
pub struct PatchKeys(pub Vec<String>);

impl PartialEq for PatchKeys {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl PatchKeys {
    /// The keys of `op`'s `patch` object, in the order they arrived.
    fn of(op: &serde_json::Value) -> Self {
        Self(
            op.get("patch")
                .and_then(serde_json::Value::as_object)
                .map(|patch| patch.keys().cloned().collect())
                .unwrap_or_default(),
        )
    }
}

impl ChangeOp {
    /// The op's `type`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::MetadataUpdate { .. } => "metadata.update",
            Self::MetadataUpdateCustom { .. } => "metadata.updateCustom",
            Self::AttachmentsCreate { .. } => "attachments.create",
            Self::AttachmentsDelete { .. } => "attachments.delete",
            Self::AnnotationsCreate { .. } => "annotations.create",
        }
    }
}

/// A change as its typed ops spell it, what both `document.apply` and a
/// single verb fingerprint, so the two agree on what "the same change" is.
pub fn change_value(change: &Change) -> Result<serde_json::Value, EngineError> {
    match change {
        Change::UndoOf(op_id) => Ok(serde_json::json!({ "undoOf": op_id })),
        Change::Ops(ops) => {
            let ops = serde_json::to_value(ops).map_err(|e| malformed(e.to_string()))?;
            Ok(serde_json::json!({ "ops": ops }))
        }
    }
}

/// The op types of upstream's `ChangeOp` union this engine does not run yet,
/// each with the sub-task that registers it. The one list the owners look
/// in: each removes its rows when it adds its variant to [`ChangeOp`].
pub const OP_OWNERS: &[(&str, &str)] = &[
    // 8.15 and 8.16: the other annotation ops.
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
    ops.iter().enumerate().map(|(at, op)| parse_op(at, op)).collect()
}

/// `document.apply` takes no import (`assertNoImport`, which only
/// `handleDocumentApply` runs): an import is its own verb's one-op change.
/// A server request's change may hold one; until 8.22 ports the imports,
/// it answers `NotImplemented` there like any op owned elsewhere.
pub fn assert_no_import(change: &serde_json::Value) -> Result<(), EngineError> {
    let Some(ops) = change.get("ops").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };
    if let Some((_, verb)) = ops.iter().find_map(|op| {
        let kind = op.get("type").and_then(serde_json::Value::as_str)?;
        IMPORT_VERBS.iter().find(|(import, _)| *import == kind)
    }) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("doc.apply takes no import: import a bundle with {verb}"),
        ));
    }
    Ok(())
}

/// One op as the wire spells it, its patch's keys kept in wire order.
pub fn parse_op(at: usize, op: &serde_json::Value) -> Result<ChangeOp, EngineError> {
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
    let mut parsed: ChangeOp = serde_json::from_value(op.clone())
        .map_err(|e| malformed(format!("op {at} ({kind}) is not the shape it takes: {e}")))?;
    match &mut parsed {
        ChangeOp::MetadataUpdate { keys, .. } | ChangeOp::MetadataUpdateCustom { keys, .. } => {
            *keys = PatchKeys::of(op);
        }
        ChangeOp::AttachmentsCreate { .. }
        | ChangeOp::AttachmentsDelete { .. }
        | ChangeOp::AnnotationsCreate { .. } => {}
    }
    Ok(parsed)
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
    fn document_apply_refuses_an_import_as_its_own_verbs_and_a_server_change_as_unported() {
        let change = serde_json::json!({ "ops": [{ "type": "forms.importValues", "bundle": {} }] });
        let refused = assert_no_import(&change).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert!(refused.message.contains("doc.forms.importValues"), "{refused}");
        let unported = parse_change(&change).unwrap_err();
        assert_eq!(unported.code, EngineErrorCode::NotImplemented);
        assert!(unported.message.contains("8.22"), "{unported}");
    }

    #[test]
    fn a_patch_keeps_its_wire_key_order() {
        let ops = parse_ops(&serde_json::json!([
            { "type": "metadata.update", "patch": { "subject": "S", "author": "A", "title": "T" } },
            { "type": "metadata.updateCustom", "patch": { "Zed": "z", "Alpha": null } },
        ]))
        .unwrap();
        let ChangeOp::MetadataUpdate { keys, .. } = &ops[0] else { panic!("update") };
        assert_eq!(keys.0, ["subject", "author", "title"]);
        let ChangeOp::MetadataUpdateCustom { keys, .. } = &ops[1] else { panic!("custom") };
        assert_eq!(keys.0, ["Zed", "Alpha"]);
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
    fn an_attachment_op_takes_its_bytes_from_the_payload_it_names() {
        let mut change = parse_change(&serde_json::json!({ "ops": [
            { "type": "attachments.create", "file": { "name": "a.txt" },
              "resources": { "file": { "payload": 1 } } },
            { "type": "attachments.delete", "ref": { "kind": "key", "key": "b.txt" } },
        ] }))
        .unwrap();
        let unresolved = change_value(&change).unwrap();
        assert_eq!(unresolved["ops"][0]["resources"]["file"], serde_json::json!({ "payload": 1 }));
        resolve_payloads(&mut change, &[Vec::new(), b"hello".to_vec()]).unwrap();
        let Change::Ops(ops) = &change else { panic!("ops") };
        let ChangeOp::AttachmentsCreate { resources, .. } = &ops[0] else { panic!("create") };
        assert_eq!(resources.file.as_ref().unwrap().bytes().unwrap(), b"hello");
        // Fingerprinted as upstream's typed array: `$bytes`, length and hash.
        let resolved = change_value(&change).unwrap();
        assert_eq!(
            resolved["ops"][0]["resources"]["file"],
            serde_json::json!({ "$bytes": "5-19jqrdkps12" })
        );
        assert_eq!(resolved["ops"][1]["ref"], serde_json::json!({ "kind": "key", "key": "b.txt" }));

        let mut missing = parse_change(&serde_json::json!({ "ops": [
            { "type": "attachments.create", "file": { "name": "a.txt" },
              "resources": { "file": { "payload": 2 } } },
        ] }))
        .unwrap();
        let refused = resolve_payloads(&mut missing, &[]).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::WireFormat);
    }

    #[test]
    fn every_owned_op_is_named_once() {
        let mut names: Vec<&str> = OP_OWNERS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OP_OWNERS.len());
        for ported in [
            "metadata.update",
            "metadata.updateCustom",
            "attachments.create",
            "attachments.delete",
            "annotations.create",
        ] {
            assert!(!names.contains(&ported), "{ported} is ported and still listed");
        }
    }
}
