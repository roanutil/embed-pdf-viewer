//! Reordering annotations within a page.
//!
//! G11. Ports the `move` half of
//! `services/src/features/annotations/AnnotationMutator.ts`. Operation:
//! `annotations.move`.
//!
//! # A contiguous block, not a permutation
//!
//! The operation detaches the named annotations and re-inserts them together
//! at one position, in the caller's order. The single-annotation case is a
//! block of one. It is not a general permutation, and the symmetry with
//! `pages.move` is deliberate: a viewer reordering a selection is doing this,
//! and a general permutation would have no natural undo.
//!
//! # Two index spaces
//!
//! `from` indices address `/Annots` as it is now. `to_index` addresses it
//! after the block has been removed. A caller that range-checked the
//! destination against the current count would be off by the size of its own
//! block, which is why the check here subtracts it.
//!
//! # Weak references are strengthened on the way out
//!
//! Every annotation in the batch gets a stable id before the move, stamping
//! one if it had none. The ones being moved therefore come back durable. The
//! OTHER weak annotations on the page do not, which is why the envelope can
//! still ask for a refetch after a move that strengthened everything it
//! touched.

use pdfium::PageView;
use serde::{Deserialize, Serialize};

use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use super::resolve::{capture_or_stamp_stable_id, resolve_annotation};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::annotation::read::dto::AnnotationDto;
use crate::features::annotation::read::page::read_page_annotations;
use crate::identity::{AnnotationRef, AnnotationStableId, PageObjectNumber};
use crate::ids::IdSource;
use crate::revision::PageState;

/// What a move produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationMoveResult {
    /// The moved annotations in their NEW order, one per reference and in
    /// the order they were given. `moved[i]` sits at `to_index + i`.
    ///
    /// Each ref may come back stronger than it went in, for the reason the
    /// module docs give.
    pub moved: Vec<AnnotationDto>,
    /// ONE envelope per batch: one revision bump and one impact
    /// computation, however many annotations were moved.
    pub meta: AnnotationListMutationMeta,
}

/// Move a contiguous block.
#[allow(clippy::too_many_arguments)]
pub fn move_annotations(
    document: &pdfium::Document,
    page: &PageView<'_>,
    page_index: u32,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
    to_index: u32,
    state_before: &PageState,
    state_after: &PageState,
    ids: &dyn IdSource,
    cancel: &CancelToken,
) -> Result<AnnotationMoveResult, EngineError> {
    cancel.check()?;
    if refs.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a move needs at least one annotation",
        ));
    }
    for annotation_ref in refs {
        if annotation_ref.page() != page_object_number {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "a move's annotations must all be on page {}; one is on page {}",
                    page_object_number.get(),
                    annotation_ref.page().get()
                ),
            ));
        }
    }

    // Resolve in caller order, capturing each one's current index and its
    // stable id. The handles are closed as we go: the native helper takes
    // the PAGE and a list of indices, and the read-back reopens by the new
    // index afterwards.
    let mut from_indices: Vec<i32> = Vec::with_capacity(refs.len());
    let mut changed: Vec<AnnotationStableId> = Vec::with_capacity(refs.len());
    for annotation_ref in refs {
        cancel.check()?;
        let annotation = resolve_annotation(page, annotation_ref)?;
        let index = page.annotation_index(&annotation);
        if index < 0 {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "an annotation being moved is not in the page's /Annots",
            ));
        }
        from_indices.push(index);
        // A write, and the last one before the apply boundary: two
        // references that resolve to the same annotation are refused below,
        // after every id is captured, so the refusal is not affected by how
        // far the stamping got.
        changed.push(capture_or_stamp_stable_id(&annotation, ids)?);
    }

    // Two references naming one annotation would make the block's size
    // disagree with the number of annotations in it, and the helper's
    // contract assumes they match.
    let mut seen = std::collections::BTreeSet::new();
    for index in &from_indices {
        if !seen.insert(*index) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("two of the move's references resolve to /Annots index {index}"),
            ));
        }
    }

    // The destination is a position in the post-removal list.
    //
    // PDFium range-checks it too, and refuses with the same engine error
    // code, so removing this check passes every test: what it buys is a
    // message that names the post-removal count instead of a bare refusal.
    // The same is true of the empty-batch guard above.
    let count = page.annotation_count();
    let post_removal = count.saturating_sub(from_indices.len() as u32);
    if to_index > post_removal {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "a move to index {to_index} is out of range: after removing the block the \
                 page has {post_removal} annotations"
            ),
        ));
    }

    cancel.check()?;
    if !page.move_annotations(&from_indices, to_index) {
        // The helper validates atomically, so a refusal changed nothing and
        // the caller's bookkeeping does not need unwinding.
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("PDFium refused the move of {:?} to index {to_index}", from_indices),
        ));
    }

    // Read back against the AFTER state's revision, so the returned DTOs
    // carry the token a caller would have to hand back rather than the one
    // that was current before their indices moved.
    //
    // Unobservable today, and correct anyway: every annotation in the batch
    // was strengthened above, so none of them comes back with an index
    // reference and the revision never reaches the wire. It would the moment
    // a weak annotation could survive a move, which is a property of the
    // stamping rather than of this read.
    let listing = read_page_annotations(
        document,
        page_index,
        page_object_number,
        &state_after.revision,
        &mut ActionReadBudgetTracker::default(),
        cancel,
    )?;
    let mut moved = Vec::with_capacity(refs.len());
    for offset in 0..refs.len() {
        let index = to_index as usize + offset;
        moved.push(listing.annotations.get(index).cloned().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the moved block should end at index {index} and the page is shorter"),
            )
        })?);
    }

    let meta =
        compute_mutation_impact(AnnotationMutationKind::Move, state_before, state_after, changed)
            .map_err(|e| EngineError::with_message(EngineErrorCode::InvalidArg, e.to_string()))?;

    Ok(AnnotationMoveResult { moved, meta })
}
