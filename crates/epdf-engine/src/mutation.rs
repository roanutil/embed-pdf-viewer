//! G1. Mutation metadata: what a mutation tells its caller besides its result.
//!
//! Replaces `core/src/mutation/MutationMeta.ts`: the `meta` every write
//! result carries. The per-family result types land beside their families
//! (`MetadataUpdateResult` in [`crate::features::metadata_write`], the
//! change results in [`crate::features::changes`]).
//!
//! The no-op flatten and redaction answers carry a `mutationSequence`
//! beside their result where upstream sends only `{tag, result}` (a kept
//! divergence, 87b61b533). It rides on the host's answer envelope
//! (`{result, mutationSequence, event}`), never inside this type, so the two
//! stay independent.

use serde::{Deserialize, Serialize};

use crate::dto::CachePins;
use crate::identity::{PageObjectNumber, PageRef};
use crate::revision::PageState;

/// One page's cache pin patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCacheDelta {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub cache: CachePins,
}

/// Cache pin patch returned by cloud mutations after the durable transaction
/// commits (`CacheDelta`).
///
/// `previous_doc_version` is what makes a partial page delta safe: a client
/// may apply the patch only when its cached manifest is exactly at that
/// version, and must otherwise refresh rather than manufacture a mixed-version
/// manifest.
///
/// Deliberately carries no plane scopes. Scopes only ever move base to layer
/// and each mutation kind knows which planes it owns, so the client flips them
/// locally when absorbing this delta. The manifest stays authoritative and the
/// 404-to-refresh rail is the backstop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheDelta {
    pub previous_doc_version: u64,
    pub doc_version: u64,
    /// Present when this mutation bumped the document-level bulk annotations
    /// pin (annotation CRUD, page insert or delete, redaction, flatten).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations_version: Option<u64>,
    /// Present when this mutation bumped the form pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forms_version: Option<u64>,
    /// Present for a page-structure write (move, rotate, delete, insert,
    /// names).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_version: Option<u64>,
    /// Present for a metadata write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_version: Option<u64>,
    /// Present for an attachment write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments_version: Option<u64>,
    /// Present for an outline write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_version: Option<u64>,
    /// The layer's write serial after this mutation (cloud only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_version: Option<u64>,
    /// Whether an artifact now exists (cloud only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working: Option<bool>,
    pub pages: Vec<PageCacheDelta>,
}

/// The `meta` of every write result (`MutationMeta`).
///
/// `affected_pages` names the pages whose annotations or content the write
/// changed. `cache_delta` is the cloud and CDN pin delta, and is `None` for
/// local engines, which have no manifest, and for a write that changed
/// nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationMeta {
    pub affected_pages: Vec<PageRef>,
    pub cache_delta: Option<CacheDelta>,
    /// The write's id: the `opId` it was given, or the one minted for it.
    /// Its events carry it as `origin.tx.id`, and `document.apply` with
    /// `{ undoOf: opId }` undoes the write.
    pub op_id: String,
    /// Whether `{ undoOf: opId }` can undo the write. False for a write that
    /// changed nothing, and for the final writes (redaction apply, flatten,
    /// signing and form repair), which also end undo for every write before
    /// them.
    pub undoable: bool,
}

impl MutationMeta {
    /// A local write's meta: no cache delta, stamped with the write it ran
    /// in ([`crate::session::document::WriteStamp`]).
    pub fn local(
        affected_pages: Vec<PageRef>,
        stamp: &crate::session::document::WriteStamp,
    ) -> Self {
        Self {
            affected_pages,
            cache_delta: None,
            op_id: stamp.op_id.clone(),
            undoable: stamp.undoable,
        }
    }
}

/// The page-state envelope the families answered before upstream's
/// [`MutationMeta`]: each affected page's revision and weak-annotation
/// state. Kept only for the writes that have not moved to [`MutationMeta`]
/// yet, each named by the sub-task that moves it: the annotation flatten
/// (8.15), `pages.flatten` (8.17), `forms.applyEffects` (8.20, retired for
/// `document.apply`) and `redaction.apply` (8.23). The last owner deletes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageStateMeta {
    pub affected_pages: Vec<PageState>,
    pub cache_delta: Option<CacheDelta>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::document::WriteStamp;

    #[test]
    fn a_local_write_answers_its_pages_its_op_and_no_cache_delta() {
        let page = PageObjectNumber::new(3).unwrap();
        let stamp = WriteStamp { op_id: "op-1".into(), undoable: true };
        let meta = MutationMeta::local(vec![PageRef::new(page)], &stamp);
        assert_eq!(
            serde_json::to_value(&meta).unwrap(),
            serde_json::json!({
                "affectedPages": [{ "kind": "objectNumber", "objectNumber": 3 }],
                "cacheDelta": null,
                "opId": "op-1",
                "undoable": true,
            })
        );
    }

    #[test]
    fn an_unbumped_pin_is_absent_rather_than_zero() {
        let delta = CacheDelta {
            previous_doc_version: 7,
            doc_version: 8,
            annotations_version: None,
            forms_version: None,
            layout_version: None,
            metadata_version: Some(4),
            attachments_version: None,
            outline_version: None,
            layer_version: None,
            working: None,
            pages: Vec::new(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json.get("annotationsVersion"), None);
        assert_eq!(json.get("working"), None);
        assert_eq!(json["metadataVersion"], 4);
        assert_eq!(json["previousDocVersion"], 7);
    }
}
