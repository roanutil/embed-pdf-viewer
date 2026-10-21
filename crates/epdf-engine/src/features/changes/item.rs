//! What a change answers (`ChangeItem`, `ChangeResult`, `ChangeAnswer` in
//! `core/src/mutation/Change.ts`).

use serde::{Deserialize, Serialize};

use super::attachments::AttachmentMutationMeta;
use crate::error::EngineError;
use crate::features::annotation::write::create::AnnotationMutationMeta;
use crate::features::files::attachments::EmbeddedFileItem;
use crate::features::metadata_write::{CustomMetadataUpdateResult, MetadataUpdateResult};
use crate::identity::PageRef;
use crate::mutation::MutationMeta;

/// What one op did: its single verb's result with the op's `type`; or, in an
/// undo, an op it left alone entirely (`skipped`). An undo's update items
/// list in `skipped` the entries they left alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChangeItem {
    #[serde(rename = "metadata.update")]
    MetadataUpdate {
        #[serde(flatten)]
        result: MetadataUpdateResult,
        /// The entries an undo left alone.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    #[serde(rename = "metadata.updateCustom")]
    MetadataUpdateCustom {
        #[serde(flatten)]
        result: CustomMetadataUpdateResult,
        /// The keys an undo left alone.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `AttachmentCreateResult`: the new attachment, read back.
    #[serde(rename = "attachments.create")]
    AttachmentsCreate { attachment: EmbeddedFileItem, meta: AttachmentMutationMeta },
    /// `AttachmentDeleteResult`: nothing exists after it, so only `meta`.
    #[serde(rename = "attachments.delete")]
    AttachmentsDelete { meta: AttachmentMutationMeta },
    /// `AttachmentRestoreResult`, only in an undo: the file a delete removed,
    /// back under its key.
    #[serde(rename = "attachments.restore")]
    AttachmentsRestore { attachment: EmbeddedFileItem, meta: AttachmentMutationMeta },
    /// `AnnotationCreateResult`, with the page the create named: the new
    /// annotation, read back as a listing reads it.
    #[serde(rename = "annotations.create")]
    AnnotationsCreate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        annotation: serde_json::Value,
        meta: AnnotationMutationMeta,
    },
    /// `AnnotationUpdateResult`, with its page: the annotation read back,
    /// what happened to its appearance, and in an undo the fields it left
    /// alone.
    #[serde(rename = "annotations.update")]
    AnnotationsUpdate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        annotation: serde_json::Value,
        appearance: crate::features::annotation::write::update::AppearanceOutcome,
        meta: AnnotationMutationMeta,
        /// The fields an undo left alone: they no longer showed what the
        /// change had set.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `AnnotationDeleteResult`, with its page: nothing exists after it, so
    /// only `meta`, whose `changed` names what went. Here only as the undo
    /// of a create; Task 8.16 runs it as an op.
    #[serde(rename = "annotations.delete")]
    AnnotationsDelete {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        meta: AnnotationMutationMeta,
    },
    /// An op an undo left alone entirely (`SkippedChangeItem`): the document
    /// no longer showed what the change being undone had set, so nothing was
    /// written. `op` is the item it would have been.
    #[serde(rename = "skipped")]
    Skipped { op: String, meta: MutationMeta },
}

impl ChangeItem {
    pub fn meta(&self) -> &MutationMeta {
        match self {
            Self::MetadataUpdate { result, .. } => &result.meta,
            Self::MetadataUpdateCustom { result, .. } => &result.meta,
            Self::AttachmentsCreate { meta, .. }
            | Self::AttachmentsDelete { meta }
            | Self::AttachmentsRestore { meta, .. } => &meta.meta,
            Self::AnnotationsCreate { meta, .. }
            | Self::AnnotationsUpdate { meta, .. }
            | Self::AnnotationsDelete { meta, .. } => &meta.meta,
            Self::Skipped { meta, .. } => meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut MutationMeta {
        match self {
            Self::MetadataUpdate { result, .. } => &mut result.meta,
            Self::MetadataUpdateCustom { result, .. } => &mut result.meta,
            Self::AttachmentsCreate { meta, .. }
            | Self::AttachmentsDelete { meta }
            | Self::AttachmentsRestore { meta, .. } => &mut meta.meta,
            Self::AnnotationsCreate { meta, .. }
            | Self::AnnotationsUpdate { meta, .. }
            | Self::AnnotationsDelete { meta, .. } => &mut meta.meta,
            Self::Skipped { meta, .. } => meta,
        }
    }

    /// Whether the item wrote to the document (`itemWrote`).
    pub fn wrote(&self) -> bool {
        !matches!(self, Self::Skipped { .. })
    }

    /// The events its single verb publishes (`changeEvents`' `itemEvents`):
    /// its result under the event's `type`, without the item's own `type`
    /// and `skipped`. An item an undo left alone publishes nothing.
    pub fn events(&self) -> Result<Vec<serde_json::Value>, EngineError> {
        let (event, result) = match self {
            Self::MetadataUpdate { result, .. } => ("metadata.updated", to_value(result)?),
            Self::MetadataUpdateCustom { result, .. } => {
                ("metadata.customUpdated", to_value(result)?)
            }
            Self::AttachmentsCreate { attachment, meta } => (
                "attachments.created",
                serde_json::json!({ "attachment": to_value(attachment)?, "meta": to_value(meta)? }),
            ),
            // `deleted` is what the delete removed (`deletedAttachmentOf`).
            Self::AttachmentsDelete { meta } => (
                "attachments.deleted",
                serde_json::json!({ "deleted": to_value(&meta.changed.first())?,
                    "meta": to_value(meta)? }),
            ),
            Self::AttachmentsRestore { attachment, meta } => (
                "attachments.restored",
                serde_json::json!({ "attachment": to_value(attachment)?, "meta": to_value(meta)? }),
            ),
            Self::AnnotationsCreate { page, annotation, meta } => (
                "annotations.created",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "annotation": annotation, "meta": to_value(meta)? }),
            ),
            Self::AnnotationsUpdate { page, annotation, appearance, meta, .. } => (
                "annotations.updated",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "annotation": annotation, "appearance": to_value(appearance)?,
                    "meta": to_value(meta)? }),
            ),
            // `deletedAnnotationsOf`: the refs in `meta.changed`.
            Self::AnnotationsDelete { page, meta } => (
                "annotations.deleted",
                serde_json::json!({ "deleted": to_value(&meta.changed)?,
                    "page": crate::identity::PageRef::new(*page), "meta": to_value(meta)? }),
            ),
            Self::Skipped { .. } => return Ok(Vec::new()),
        };
        let mut event_value = serde_json::json!({ "type": event });
        if let (Some(object), serde_json::Value::Object(fields)) =
            (event_value.as_object_mut(), result)
        {
            object.extend(fields);
        }
        Ok(vec![event_value])
    }
}

/// What `document.apply` answers (`ChangeResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResult {
    /// One per op, in op order; for an undo, one per step of the record.
    pub items: Vec<ChangeItem>,
    /// Every page the change touched, the change's `opId` and whether it
    /// can be undone.
    pub meta: MutationMeta,
}

impl ChangeResult {
    /// Whether any item wrote: a change that wrote nothing needs no save.
    pub fn wrote(&self) -> bool {
        self.items.iter().any(ChangeItem::wrote)
    }
}

/// Every page the items touched, once each, in order (`pagesOf`).
pub fn pages_of(items: &[ChangeItem]) -> Vec<PageRef> {
    let mut pages: Vec<PageRef> = Vec::new();
    for item in items {
        for page in &item.meta().affected_pages {
            if !pages.contains(page) {
                pages.push(*page);
            }
        }
    }
    pages
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, EngineError> {
    serde_json::to_value(value).map_err(|e| {
        EngineError::with_message(
            crate::error::EngineErrorCode::WireFormat,
            format!("a change item could not be serialized: {e}"),
        )
    })
}
