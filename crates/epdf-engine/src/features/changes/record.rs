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
    #[serde(rename = "annotation.restore")]
    AnnotationRestore(AnnotationRestoreStep),
    #[serde(rename = "annotation.reorder")]
    AnnotationReorder(AnnotationReorderStep),
    #[serde(rename = "objects.revert")]
    ObjectsRevert(ObjectsRevertStep),
    #[serde(rename = "pages.rotate")]
    PagesRotate(PageRotateStep),
    #[serde(rename = "pages.reorder")]
    PagesReorder(PageReorderStep),
    #[serde(rename = "pages.restore")]
    PagesRestore(PageRestoreStep),
    #[serde(rename = "pages.remove")]
    PagesRemove(PageRemoveStep),
    #[serde(rename = "field.remove")]
    FieldRemove(FieldRemoveStep),
    #[serde(rename = "field.restore")]
    FieldRestore(FieldRestoreStep),
    #[serde(rename = "widget.restore")]
    WidgetRestore(WidgetRestoreStep),
    #[serde(rename = "widget.delete")]
    WidgetDelete(WidgetDeleteStep),
    #[serde(rename = "calculations.restore")]
    CalculationsRestore(CalculationOrderStep),
}

/// Deletes a field a create made, when nobody changed it since
/// (`FieldRemoveStep`). `left` is the field as the change left it, in the
/// file's coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldRemoveStep {
    pub object_number: u32,
    pub left: serde_json::Value,
}

/// Brings back a field a delete removed, its widgets and the parents it
/// pruned, from the fork's capture (`EPDFForm_ExportFieldRawToOwnedBuffer`)
/// (`FieldRestoreStep`). Left alone when another field took its name, or a
/// page its widgets were on is gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldRestoreStep {
    pub object_number: u32,
    /// Its fully qualified name.
    pub name: String,
    /// The pages its widgets were on.
    pub pages: Vec<crate::identity::PageRef>,
    #[serde(with = "hex_vec")]
    pub capture: Vec<u8>,
}

/// Brings back a widget a delete removed: onto its page at its place, and
/// into its field again when it had one (`WidgetRestoreStep`). Left alone
/// when the page is gone, the widget is back already, or its field changed
/// since.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetRestoreStep {
    pub page: crate::identity::PageRef,
    pub widget: crate::identity::AnnotationRef,
    /// The widget as it was on its page, out of its field.
    #[serde(with = "hex_vec")]
    pub capture: Vec<u8>,
    /// The field it was in; `None` for a widget in no field.
    pub field_object_number: Option<u32>,
    /// The field's dictionary and the widget's, before and after it left
    /// the field.
    pub detached: Vec<CapturedObject>,
}

/// Deletes a widget a restore brought back, when nobody changed it since:
/// the redo of a delete (`WidgetDeleteStep`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDeleteStep {
    pub widget: crate::identity::AnnotationRef,
    pub left: serde_json::Value,
}

/// Takes back what a change did to the form's calculation order, by field
/// object number, where nobody changed it since (`CalculationOrderStep`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationOrderStep {
    pub before: Vec<u32>,
    pub after: Vec<u32>,
}

/// Turns pages back to `rotation` while each still has the rotation the
/// change left (`PageRotateStep`); the pages by object number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRotateStep {
    pub pages: Vec<u32>,
    pub rotation: crate::geom::PdfRotation,
    pub left: crate::geom::PdfRotation,
}

/// Where a page goes back to, or was left: right after `after`, by object
/// number, or first for 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePlacement {
    pub page: u32,
    pub after: u32,
}

/// Puts pages back where they were before a move: its reverse, and, the
/// other way round, its redo (`PageReorderStep`). Each page goes only while
/// it still follows the page the move left it after, and the page it goes
/// after is still there. `placements` is in display order, so an anchor is
/// placed before what follows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReorderStep {
    pub placements: Vec<PagePlacement>,
    pub left: Vec<PagePlacement>,
}

/// One page a delete removed, and the fork's capture of it
/// (`EPDFPage_ExportRawToOwnedBuffer`): the page, what it reaches, its
/// names and the page it followed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedPage {
    pub page: u32,
    #[serde(with = "hex_vec")]
    pub capture: Vec<u8>,
}

/// Brings back what a page delete removed: the same pages, at the same
/// numbers, each after the page it followed while that is still there,
/// else first, with their annotations and names (`PageRestoreStep`). In the
/// display order they had.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRestoreStep {
    pub pages: Vec<CapturedPage>,
}

/// One page an insert or a restore brought, and the digest of how the
/// change left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftPage {
    pub page: u32,
    pub left: String,
}

/// Deletes pages an insert or a restore brought, each while it reads as the
/// change left it: the page and what the layer holds of its annotations,
/// contents and resources (`PageRemoveStep`). Undoing an insert never
/// deletes a page someone annotated, or turned, since. A step that would
/// delete every page is left alone: a document keeps one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRemoveStep {
    pub pages: Vec<LeftPage>,
}

/// Bytes as hex, as a record crosses the wire here.
mod hex_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::features::files::attachments::hex_bytes;

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex_bytes::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        hex_bytes::decode(&String::deserialize(d)?)
            .ok_or_else(|| serde::de::Error::custom("not hex bytes"))
    }
}

/// Puts back the dictionaries an op wrote, exactly as they were, when each
/// still reads as the op left it (`ObjectsRevertStep`). When one doesn't,
/// someone changed it since: an annotation update or a form value write
/// puts back, by value, only what still shows what the op set
/// (`fallback`); any other op is skipped.
///
/// A form write captures its dictionaries (`objects`, the fork's raw and
/// canonical dictionary exports). An annotation update does not yet: its
/// captures wait for Task 8.30, so its `objects` is empty and it always
/// takes the value path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectsRevertStep {
    /// What its item reports doing: `annotations.update`, `forms.setValue`,
    /// `forms.setDisplay`, `forms.setAppearanceText` or `forms.reset`.
    pub reports: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<CapturedObject>,
    /// What the item reads back.
    pub subject: RevertSubject,
    /// Whether the update changed what the annotation shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance_changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<RevertFallback>,
}

/// A dictionary an op wrote (`CapturedObject`): what the layer held of it
/// before (`None`: it didn't exist), its keys under `deep` with the objects
/// they reach, and the canonical bytes of how the op left it, the guard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedObject {
    pub object_number: u32,
    pub deep: Vec<String>,
    #[serde(with = "crate::features::files::attachments::hex_bytes")]
    pub before: Option<Vec<u8>>,
    #[serde(with = "hex_vec")]
    pub after: Vec<u8>,
}

/// What a revert's item reads back: the annotation, or the form fields by
/// object number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RevertSubject {
    Annotation { annotation: crate::identity::AnnotationRef },
    Fields { fields: Vec<u32> },
}

/// How a revert puts values back when the dictionaries it wrote changed
/// since (`RevertFallback`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RevertFallback {
    /// `restore` the old values of the fields an annotation update changed,
    /// `left` the values it left in them. A field still holding its value
    /// is put back.
    Annotation {
        restore: serde_json::Map<String, serde_json::Value>,
        left: serde_json::Map<String, serde_json::Value>,
    },
    /// Each form field a value write changed: its value before, and the
    /// value it left. A field still holding that value gets its old one
    /// back.
    Value { fields: Vec<FieldValueChange> },
    /// The properties a field update changed, as a patch: their old values
    /// (`restore`) and the values it left (`left`). A property still
    /// holding its value is put back.
    #[serde(rename_all = "camelCase")]
    Properties {
        object_number: u32,
        restore: serde_json::Map<String, serde_json::Value>,
        left: serde_json::Map<String, serde_json::Value>,
    },
}

/// One field of a [`RevertFallback::Value`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldValueChange {
    pub object_number: u32,
    pub before: crate::features::forms::value::FormFieldValue,
    pub left: crate::features::forms::value::FormFieldValue,
}

/// Deletes what a create or a restore brought, with its thread and popups,
/// while it all reads as the change left it (`AnnotationRemoveStep`):
/// undoing a create never deletes an annotation someone changed since.
/// `left` is each, as it read in the file's coordinates.
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

/// Brings back what a delete removed, at the same numbers and positions
/// (`AnnotationRestoreStep`). Skipped when its page is gone or it is back
/// already.
///
/// `capture` is the fork's capture of the members as the delete removed
/// them (`EPDFPage_ExportAnnotsRawToOwnedBuffer`), which only a layer can
/// take, and every session is one: `None` only for a delete that removed
/// nothing.
/// `unlinked` is where upstream keeps a capture of each annotation that
/// stayed and stopped naming a popup that went (`captureAfter`), which the
/// fork exports only in a layer transaction (Task 8.30): here each is named,
/// with the popup it showed, and linked again while it names none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationRestoreStep {
    #[serde(with = "crate::identity::page_ref")]
    pub page: crate::identity::PageObjectNumber,
    /// What the delete removed, the annotation first.
    pub members: Vec<crate::identity::AnnotationRef>,
    #[serde(with = "crate::features::files::attachments::hex_bytes")]
    pub capture: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlinked: Vec<UnlinkedPopup>,
}

/// An annotation that stayed when the popup it showed went with a delete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkedPopup {
    #[serde(rename = "ref")]
    pub reference: crate::identity::AnnotationRef,
    pub popup: crate::identity::AnnotationRef,
}

/// Puts the rows of a reorder back beside their old neighbours
/// (`AnnotationReorderStep`). A row goes back only while it is where the
/// reorder left it, above the same row. `before` and `after` are the
/// family's rows, bottom to top, on each side of the reorder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationReorderStep {
    #[serde(with = "crate::identity::page_ref")]
    pub page: crate::identity::PageObjectNumber,
    pub family: crate::features::annotation::appearance::AnnotationFamily,
    pub refs: Vec<crate::identity::AnnotationRef>,
    pub before: Vec<crate::identity::AnnotationRef>,
    pub after: Vec<crate::identity::AnnotationRef>,
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
/// attachment remove here: its guard is a read, not canonical bytes. An
/// annotation restore, a page restore and a form write's revert hold their
/// captures.
pub fn record_bytes(record: &ChangeRecord) -> usize {
    record
        .steps
        .iter()
        .map(|step| match step {
            ReverseStep::MetadataRevert(_)
            | ReverseStep::AttachmentsRemove(_)
            | ReverseStep::AnnotationRemove(_) => 0,
            ReverseStep::ObjectsRevert(step) => step
                .objects
                .iter()
                .map(|object| object.before.as_ref().map_or(0, Vec::len) + object.after.len())
                .sum(),
            ReverseStep::AttachmentsRestore(step) => {
                step.capture.bytes.as_ref().map_or(0, Vec::len)
            }
            ReverseStep::AnnotationRestore(step) => step.capture.as_ref().map_or(0, Vec::len),
            ReverseStep::AnnotationReorder(_)
            | ReverseStep::PagesRotate(_)
            | ReverseStep::PagesReorder(_)
            | ReverseStep::PagesRemove(_) => 0,
            ReverseStep::PagesRestore(step) => {
                step.pages.iter().map(|page| page.capture.len()).sum()
            }
            // As upstream counts them: a widget's restore and the guards
            // of a field's remove hold no counted capture.
            ReverseStep::FieldRemove(_)
            | ReverseStep::WidgetDelete(_)
            | ReverseStep::WidgetRestore(_)
            | ReverseStep::CalculationsRestore(_) => 0,
            ReverseStep::FieldRestore(step) => step.capture.len(),
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
