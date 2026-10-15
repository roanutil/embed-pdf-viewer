//! G1. Mutation metadata: what a mutation tells its caller besides its result.
//!
//! Replaces `core/src/mutation/{MutationMeta,RefetchReason,PageStructureCache}.ts`.
//! The per-family result and impact types (`AnnotationMutationResults` in G11,
//! `FormMutationResults` in G12, the page result types in G13,
//! `AttachmentMutationResults` in G14) land here as their families migrate.

use serde::{Deserialize, Serialize};

use crate::dto::CachePins;
use crate::identity::PageObjectNumber;
use crate::revision::PageState;

/// Why a mutation invalidated the snapshot the caller was holding, so the
/// client can pick the right treatment: a silent refresh or a visible "the
/// list changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefetchReason {
    /// Indices on the affected page moved, so weak refs are stale.
    WeakRefsInvalidated,
    /// A change unrelated to this mutation, typically another writer.
    ExternalChange,
    /// The page was rebuilt: insert, delete or reorder.
    PageRebuilt,
}

/// One page's cache pin patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCacheDelta {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub cache: CachePins,
}

/// Cache pin patch returned by cloud mutations after the durable transaction
/// commits.
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
    /// pin (annotation CRUD, page insert or delete, redaction, flatten, form
    /// structure). Absorbing it keeps a cached manifest addressing the fresh
    /// bulk leaf without a 404 round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations_version: Option<u64>,
    pub pages: Vec<PageCacheDelta>,
}

/// The envelope every layer-mutating operation returns alongside its result.
///
/// `affected_pages` is the state delta. `cache_delta` is the cloud and CDN pin
/// delta, and is `None` for local engines, which have no manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationMeta {
    pub affected_pages: Vec<PageState>,
    pub cache_delta: Option<CacheDelta>,
}

/// Cloud-only coherence pins returned by every page-STRUCTURE mutation: move,
/// rotate, delete.
///
/// All three share one shape. They bump the manifest `doc_version` and the
/// geometry pointer `layout_version`, and touch no per-page content or
/// annotation pins, so those caches stay warm: rotation is presentation
/// metadata applied at display time over normalized renders, and move and
/// delete change the arrangement, never a surviving page's bytes.
///
/// `None` on the result for local engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageStructureCache {
    pub previous_doc_version: u64,
    pub doc_version: u64,
    pub layout_version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revision::{RevisionToken, WeakAnnotationState};

    #[test]
    fn a_local_engine_publishes_page_state_with_no_cache_delta() {
        let page = PageObjectNumber::new(3).unwrap();
        let meta = MutationMeta {
            affected_pages: vec![PageState {
                page_object_number: page,
                revision: RevisionToken::new("session-1", page, 2),
                weak_annotation_state: WeakAnnotationState::Known {
                    has_any_weak_annotations: false,
                },
            }],
            cache_delta: None,
        };
        assert_eq!(
            serde_json::to_value(&meta).unwrap(),
            serde_json::json!({
                "affectedPages": [{
                    "page": { "kind": "objectNumber", "pageObjectNumber": 3 },
                    "revision": {
                        "docSessionId": "session-1",
                        "page": { "kind": "objectNumber", "pageObjectNumber": 3 },
                        "generation": 2,
                    },
                    "weakAnnotationState": { "kind": "known", "hasAnyWeakAnnotations": false },
                }],
                "cacheDelta": null,
            })
        );
    }

    #[test]
    fn an_unbumped_annotations_pin_is_absent_rather_than_zero() {
        let delta = CacheDelta {
            previous_doc_version: 7,
            doc_version: 8,
            annotations_version: None,
            pages: Vec::new(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json.get("annotationsVersion"), None);
        assert_eq!(json["previousDocVersion"], 7);
    }

    #[test]
    fn refetch_reasons_keep_their_wire_spelling() {
        assert_eq!(
            serde_json::to_value(RefetchReason::WeakRefsInvalidated).unwrap(),
            serde_json::json!("weakRefsInvalidated")
        );
        assert_eq!(
            serde_json::to_value(RefetchReason::PageRebuilt).unwrap(),
            serde_json::json!("pageRebuilt")
        );
    }
}
