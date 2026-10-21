//! What a change answers (`ChangeItem`, `ChangeResult`, `ChangeAnswer` in
//! `core/src/mutation/Change.ts`).

use serde::{Deserialize, Serialize};

use crate::error::EngineError;
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
            Self::Skipped { meta, .. } => meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut MutationMeta {
        match self {
            Self::MetadataUpdate { result, .. } => &mut result.meta,
            Self::MetadataUpdateCustom { result, .. } => &mut result.meta,
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
