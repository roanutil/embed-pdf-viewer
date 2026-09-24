//! What a mutation did to the page's index space.
//!
//! G11. Ports `core/src/mutation/AnnotationMutationImpactPolicy.ts` and
//! `services/src/features/annotations/internal/mutations/computeMutationImpact.ts`.
//!
//! # Only two of the four mutations move an index
//!
//! An `update` does not touch `/Annots` at all. A `create` appends, so the
//! new annotation lands at the old count and nothing that was already there
//! moves. A `delete` removes an entry and shifts every later one down. A
//! `move` detaches a contiguous block and re-inserts it, rewriting indices on
//! both ends of the gap.
//!
//! So only `delete` and `move` can make a client's index-addressed reference
//! point at a different annotation, and only they can require a refetch.
//!
//! # Revisions are for weak-reference authentication and nothing else
//!
//! A `create` is not given a revision bump even though the page demonstrably
//! changed. Bumping for a change no weak reference depends on would turn the
//! revision into a general "something happened" signal, and then it could no
//! longer be used to prove that an index is still the index it was.

use serde::{Deserialize, Serialize};

use crate::identity::AnnotationStableId;
use crate::revision::{PageState, WeakAnnotationState};

/// Which mutation happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationMutationKind {
    Create,
    Update,
    Delete,
    Move,
}

impl AnnotationMutationKind {
    /// Whether this mutation moves indices that already existed.
    ///
    /// An explicit-position `create_at` would join `Delete` and `Move` here,
    /// because it would shift every index at or after its insertion point.
    /// There is no such operation yet.
    pub fn shifts_existing_indices(self) -> bool {
        matches!(self, Self::Delete | Self::Move)
    }
}

/// Why a client should re-read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefetchReason {
    /// Indices on the page moved and weak references are stale.
    WeakRefsInvalidated,
    /// A change from somewhere else, such as another writer. Not emitted by
    /// this engine yet.
    ExternalChange,
    /// The page's `/Annots` was rebuilt. Not emitted by this engine yet.
    ///
    /// Notably NOT emitted by a page REORDER: reordering pages does not bump
    /// a per-page revision, because an annotation's index within its page is
    /// unaffected by where the page sits.
    PageRebuilt,
}

/// The side-effect envelope every annotation mutation returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationListMutationMeta {
    pub affected_pages: Vec<PageState>,
    /// The cloud's CDN pin delta. Always `None` locally, and present on the
    /// wire as `null` rather than omitted, because a consumer distinguishing
    /// the two engines by a missing key would be reading the transport.
    pub cache_delta: Option<serde_json::Value>,
    /// The stable ids the mutation actually touched. Empty for a weak
    /// delete, which has no durable id to report.
    pub changed: Vec<AnnotationStableId>,
    pub weak_refs_invalidated: bool,
    /// `None` means the caller's snapshot is still valid.
    pub should_refetch: Option<ShouldRefetch>,
}

/// The refetch instruction, as an object so the reason can be read without
/// the caller having to know whether `null` meant no or meant unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShouldRefetch {
    pub reason: RefetchReason,
}

/// Whether a mutation invalidated index-addressed references.
///
/// # Panics
///
/// Never. The TypeScript THROWS when asked about an index-shifting mutation
/// with an unknown weak state, on the grounds that the caller was supposed to
/// scan the page first. Here it is a `Result`, because the condition is a
/// caller's mistake rather than an impossible state, and an engine that
/// unwound a mutation's bookkeeping through a panic would leave the session
/// in a shape nobody has reasoned about.
pub fn invalidates_weak_index_refs(
    kind: AnnotationMutationKind,
    weak_state_before: WeakAnnotationState,
) -> Result<bool, UnknownWeakState> {
    if !kind.shifts_existing_indices() {
        return Ok(false);
    }
    match weak_state_before {
        WeakAnnotationState::Known { has_any_weak_annotations } => Ok(has_any_weak_annotations),
        WeakAnnotationState::Unknown => Err(UnknownWeakState { kind }),
    }
}

/// An index-shifting mutation was asked about before the page was scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownWeakState {
    pub kind: AnnotationMutationKind,
}

impl std::fmt::Display for UnknownWeakState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "computing the impact of a {:?} needs a known weak-annotation state; \
             the page has to be scanned first",
            self.kind
        )
    }
}

/// What the mutation's caller needs to know.
///
/// `before` is the page state captured BEFORE the mutation, and its weak
/// state is what decides a refetch: a page that was already fully durable has
/// no client holding a stale index, whatever just happened to it. `after`
/// carries the bumped revision and the recomputed weak state.
pub fn compute_mutation_impact(
    kind: AnnotationMutationKind,
    before: &PageState,
    after: &PageState,
    changed: Vec<AnnotationStableId>,
) -> Result<AnnotationListMutationMeta, UnknownWeakState> {
    if !kind.shifts_existing_indices() {
        return Ok(AnnotationListMutationMeta {
            affected_pages: vec![after.clone()],
            cache_delta: None,
            changed,
            weak_refs_invalidated: false,
            should_refetch: None,
        });
    }

    let weak_refs_invalidated = invalidates_weak_index_refs(kind, before.weak_annotation_state)?;
    Ok(AnnotationListMutationMeta {
        affected_pages: vec![after.clone()],
        cache_delta: None,
        changed,
        weak_refs_invalidated,
        // Always this reason when it fires. The other two are reserved for
        // signals this engine does not emit.
        should_refetch: weak_refs_invalidated
            .then_some(ShouldRefetch { reason: RefetchReason::WeakRefsInvalidated }),
    })
}
