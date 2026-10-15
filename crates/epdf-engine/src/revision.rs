//! G1. Revision tokens and per-page liveness.
//!
//! Replaces `core/src/revision/*`.

use serde::{Deserialize, Serialize};

use crate::identity::PageObjectNumber;

/// The token a client hands back when addressing an annotation by
/// `(page, index)`.
///
/// The engine validates strict equality against its own revision store; a
/// mismatch is [`crate::error::EngineErrorCode::InvalidReference`], never a
/// silent retarget. `doc_session_id` ties a token to one open session, so a
/// token minted before a close-and-reopen is rejected by the new session
/// rather than bleeding across.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionToken {
    pub doc_session_id: String,
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub generation: u64,
}

impl RevisionToken {
    pub fn new(
        doc_session_id: impl Into<String>,
        page_object_number: PageObjectNumber,
        generation: u64,
    ) -> Self {
        Self { doc_session_id: doc_session_id.into(), page_object_number, generation }
    }
}

/// Tri-state knowledge about weak annotation identities on a page.
///
/// `Known(false)` is a stronger claim than "we have not looked yet": it means
/// the page's annotations were scanned and every one had a durable identity.
/// A cloud or CDN manifest may publish a boolean weak flag only from a known
/// state, which is why `Unknown` is a variant rather than an `Option`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WeakAnnotationState {
    Unknown,
    Known {
        #[serde(rename = "hasAnyWeakAnnotations")]
        has_any_weak_annotations: bool,
    },
}

/// Per-page liveness envelope returned with annotation reads and mutation
/// metadata.
///
/// Deliberately scoped to annotation liveness. Geometry and display order live
/// in `PageLayout` from `pages.list`, joined to this by page object number:
/// the two change on different cadences (annotation edits versus structural
/// operations) and are never merged into one struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub revision: RevisionToken,
    pub weak_annotation_state: WeakAnnotationState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_from_different_sessions_never_match() {
        let page = PageObjectNumber::new(3).unwrap();
        let minted = RevisionToken::new("session-1", page, 4);
        assert_eq!(minted, RevisionToken::new("session-1", page, 4));
        // Same page, same generation, document closed and reopened.
        assert_ne!(minted, RevisionToken::new("session-2", page, 4));
        assert_ne!(minted, RevisionToken::new("session-1", page, 5));
        assert_ne!(minted, RevisionToken::new("session-1", PageObjectNumber::new(4).unwrap(), 4));
    }

    #[test]
    fn weak_state_keeps_unknown_distinct_from_known_false() {
        assert_ne!(
            WeakAnnotationState::Unknown,
            WeakAnnotationState::Known { has_any_weak_annotations: false }
        );
        assert_eq!(
            serde_json::to_value(WeakAnnotationState::Unknown).unwrap(),
            serde_json::json!({ "kind": "unknown" })
        );
        assert_eq!(
            serde_json::to_value(WeakAnnotationState::Known { has_any_weak_annotations: true })
                .unwrap(),
            serde_json::json!({ "kind": "known", "hasAnyWeakAnnotations": true })
        );
    }
}
