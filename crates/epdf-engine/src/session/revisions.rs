//! Per-page generation counters.
//!
//! G3. Replaces `services/src/document-session/revisions/RevisionAuthority.ts`.
//!
//! Backs [`RevisionToken`] so that a weak annotation reference, the kind
//! addressed by `(pageObjectNumber, index)` rather than by a durable id, can
//! be validated against the exact page state it was minted from. An index is
//! only meaningful while the page has not changed underneath it, and this is
//! what makes "has not changed" a thing the engine can answer instead of
//! assume.
//!
//! [`RevisionAuthority::bump`] is the only mutator. Mutation services call it
//! after a structural change to a page; read paths never touch the store.

use std::collections::BTreeMap;

use crate::error::{EngineError, EngineErrorCode};
use crate::identity::PageObjectNumber;
use crate::revision::{RevisionToken, WeakAnnotationState};

/// The authority for one open document session.
#[derive(Debug)]
pub struct RevisionAuthority {
    doc_session_id: String,
    generations: BTreeMap<PageObjectNumber, u64>,
    weak_annotation_states: BTreeMap<PageObjectNumber, WeakAnnotationState>,
}

impl RevisionAuthority {
    pub fn new(doc_session_id: impl Into<String>) -> Self {
        Self {
            doc_session_id: doc_session_id.into(),
            generations: BTreeMap::new(),
            weak_annotation_states: BTreeMap::new(),
        }
    }

    pub fn doc_session_id(&self) -> &str {
        &self.doc_session_id
    }

    /// The current generation, which is zero for a page never bumped.
    ///
    /// Absent and zero are the same answer on purpose: a page nobody has
    /// mutated is at generation zero, and storing that explicitly would only
    /// make the map bigger.
    pub fn current(&self, page_object_number: PageObjectNumber) -> u64 {
        self.generations.get(&page_object_number).copied().unwrap_or(0)
    }

    /// Mint a token for the page's current generation.
    pub fn token(&self, page_object_number: PageObjectNumber) -> RevisionToken {
        RevisionToken::new(
            self.doc_session_id.clone(),
            page_object_number,
            self.current(page_object_number),
        )
    }

    /// Increment the page's generation and return the new token.
    ///
    /// Saturating rather than wrapping. A wrap would make a very old token
    /// valid again, which is the one failure mode this counter exists to
    /// prevent; at one bump per nanosecond a `u64` takes longer than the age
    /// of the universe to get there, so the branch is unreachable and cheap
    /// insurance either way.
    pub fn bump(&mut self, page_object_number: PageObjectNumber) -> RevisionToken {
        let next = self.current(page_object_number).saturating_add(1);
        self.generations.insert(page_object_number, next);
        self.token(page_object_number)
    }

    /// Strict equality against what the store holds.
    ///
    /// Reports [`EngineErrorCode::InvalidReference`] rather than silently
    /// retargeting. A stale token means the caller is addressing a row that
    /// may no longer be the row they meant, and guessing which one they want
    /// is how an edit lands on the wrong annotation.
    pub fn validate(&self, token: &RevisionToken) -> Result<(), EngineError> {
        if token.doc_session_id != self.doc_session_id {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidReference,
                "revision token belongs to a different document session",
            ));
        }
        let current = self.token(token.page_object_number);
        if &current != token {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidReference,
                format!(
                    "revision token is stale: page {} is at generation {}, token carries {}",
                    token.page_object_number, current.generation, token.generation,
                ),
            ));
        }
        Ok(())
    }

    pub fn weak_annotation_state(
        &self,
        page_object_number: PageObjectNumber,
    ) -> WeakAnnotationState {
        self.weak_annotation_states
            .get(&page_object_number)
            .copied()
            .unwrap_or(WeakAnnotationState::Unknown)
    }

    pub fn record_weak_annotation_state(
        &mut self,
        page_object_number: PageObjectNumber,
        state: WeakAnnotationState,
    ) {
        self.weak_annotation_states.insert(page_object_number, state);
    }

    /// Forget a page entirely.
    ///
    /// Called when the page is deleted. Its object number is retired and never
    /// recycled, so this is hygiene rather than correctness: a token for a
    /// deleted page would validate against generation zero afterwards, which
    /// is harmless because no page will ever carry that number again.
    pub fn drop_page(&mut self, page_object_number: PageObjectNumber) {
        self.generations.remove(&page_object_number);
        self.weak_annotation_states.remove(&page_object_number);
    }

    pub fn clear(&mut self) {
        self.generations.clear();
        self.weak_annotation_states.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: u32) -> PageObjectNumber {
        PageObjectNumber::new(n).expect("object number")
    }

    #[test]
    fn a_page_nobody_has_touched_is_at_generation_zero() {
        let authority = RevisionAuthority::new("session-1");
        assert_eq!(authority.current(page(3)), 0);
        authority.validate(&authority.token(page(3))).expect("a fresh token validates");
    }

    #[test]
    fn bump_invalidates_a_token_minted_before_it() {
        let mut authority = RevisionAuthority::new("session-1");
        let before = authority.token(page(3));
        let after = authority.bump(page(3));

        authority.validate(&after).expect("the new token");
        assert_eq!(
            authority.validate(&before).expect_err("the old token").code,
            EngineErrorCode::InvalidReference,
        );
    }

    #[test]
    fn bumping_one_page_does_not_disturb_another() {
        let mut authority = RevisionAuthority::new("session-1");
        let other = authority.token(page(4));
        authority.bump(page(3));
        authority.validate(&other).expect("page 4 is untouched");
    }

    #[test]
    fn a_token_from_another_session_is_refused() {
        // The close-and-reopen case. Same page, same generation, different
        // session: accepting it would let a reference survive a reopen that
        // may have changed the document underneath it.
        let authority = RevisionAuthority::new("session-2");
        let foreign = RevisionToken::new("session-1", page(3), 0);
        assert_eq!(
            authority.validate(&foreign).expect_err("a foreign token").code,
            EngineErrorCode::InvalidReference,
        );
    }

    #[test]
    fn weak_state_starts_unknown_and_is_remembered() {
        let mut authority = RevisionAuthority::new("session-1");
        assert_eq!(authority.weak_annotation_state(page(3)), WeakAnnotationState::Unknown);

        let known = WeakAnnotationState::Known { has_any_weak_annotations: false };
        authority.record_weak_annotation_state(page(3), known);
        assert_eq!(authority.weak_annotation_state(page(3)), known);
        // Known-false is a stronger claim than unknown, and the two must not
        // collapse: a manifest may publish a boolean only from a known state.
        assert_ne!(known, WeakAnnotationState::Unknown);
    }

    #[test]
    fn dropping_a_page_forgets_both_maps() {
        let mut authority = RevisionAuthority::new("session-1");
        authority.bump(page(3));
        authority.record_weak_annotation_state(
            page(3),
            WeakAnnotationState::Known { has_any_weak_annotations: true },
        );

        authority.drop_page(page(3));
        assert_eq!(authority.current(page(3)), 0);
        assert_eq!(authority.weak_annotation_state(page(3)), WeakAnnotationState::Unknown);
    }

    #[test]
    fn clear_resets_every_page() {
        let mut authority = RevisionAuthority::new("session-1");
        authority.bump(page(3));
        authority.bump(page(4));
        authority.clear();
        assert_eq!(authority.current(page(3)), 0);
        assert_eq!(authority.current(page(4)), 0);
    }

    #[test]
    fn a_stale_token_says_which_generations_disagree() {
        // The message is diagnostic, not a contract, but a mismatch that does
        // not say what it saw costs somebody an afternoon.
        let mut authority = RevisionAuthority::new("session-1");
        let stale = authority.token(page(3));
        authority.bump(page(3));
        authority.bump(page(3));
        let error = authority.validate(&stale).expect_err("stale");
        assert!(error.message.contains("generation 2"), "{}", error.message);
        assert!(error.message.contains("carries 0"), "{}", error.message);
    }
}
