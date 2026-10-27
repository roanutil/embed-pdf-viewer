//! A page's stacking order, per family.
//!
//! G11. Ports `services/src/features/annotations/internal/stackingOrder.ts`.
//! Operation: `annotations.reorder` (1233d1349), as a change op
//! ([`crate::features::changes`]); it replaced `annotations.move`.
//!
//! A page's `/Annots` holds both of its families, but PDFium paints every
//! annotation and then every widget (`CPDF_AnnotList::DisplayAnnots`), so
//! each family has an order of its own, later over earlier, and where the
//! two interleave shows nowhere. A reorder moves rows of one family next to
//! a neighbour of the same family, as one block move in `/Annots`: the rows
//! land beside their own family's rows and the other family keeps its
//! order. Nothing is renamed or stamped (bc0057f3a): every row keeps the
//! name it had.

use pdfium::Document;
use serde::Serialize;

use super::create::AnnotationMutationMeta;
use super::delete::promote_inline_annotations;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::appearance::AnnotationFamily;
use crate::features::annotation::read::record::annotation_ref_of;
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::mutation::list_position::{AnnotationPosition, End, ListPosition, reordered_list};

/// What a reorder answers (`AnnotationReorderResult`): every annotation of
/// the page, widgets excepted, bottom to top; `meta.changed` names the
/// annotations that moved, in the order given.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationReorderResult {
    pub order: Vec<AnnotationRef>,
    pub meta: AnnotationMutationMeta,
}

/// One entry of a page's `/Annots`: the annotation it holds and its family.
#[derive(Debug, Clone, PartialEq)]
pub struct StackEntry {
    pub reference: AnnotationRef,
    pub family: AnnotationFamily,
}

/// A page's `/Annots` as it is now, read from its dictionaries without
/// loading the page (`PageStack`).
#[derive(Debug, Clone, PartialEq)]
pub struct PageStack {
    pub page: PageObjectNumber,
    pub page_index: u32,
    /// Every entry in `/Annots` order; `None` for one that can't be opened.
    pub entries: Vec<Option<StackEntry>>,
}

/// `readPageStack`.
pub fn read_page_stack(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
) -> Result<PageStack, EngineError> {
    let count = document.raw_annotation_count(page_index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFPage_GetAnnotCountRaw refused page {}", page.get()),
        )
    })?;
    let entries = (0..count)
        .map(|index| {
            document.raw_annotation(page_index, index).map(|annotation| StackEntry {
                reference: annotation_ref_of(document, &annotation, page, index),
                family: AnnotationFamily::of_code(annotation.subtype_code()),
            })
        })
        .collect();
    Ok(PageStack { page, page_index, entries })
}

/// `familyOrder`: `family`'s rows on the page, bottom to top.
pub fn family_order(stack: &PageStack, family: AnnotationFamily) -> Vec<AnnotationRef> {
    stack
        .entries
        .iter()
        .flatten()
        .filter(|entry| entry.family == family)
        .map(|entry| entry.reference.clone())
        .collect()
}

/// A reorder worked out against a page's stack, before anything is written
/// (`PlannedReorder`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedReorder {
    pub stack: PageStack,
    pub family: AnnotationFamily,
    /// The family's rows before the reorder, bottom to top.
    pub before: Vec<AnnotationRef>,
    /// The family's rows after it.
    pub after: Vec<AnnotationRef>,
    /// The `/Annots` positions of the rows that move, in the order given.
    pub from: Vec<u32>,
    /// Where they go together, as a position in the `/Annots` left once
    /// they are out.
    pub to: u32,
}

/// `planReorder`: where `refs` go when they move together, in the order
/// given, to `position` among `family`'s rows. Refuses before anything is
/// written: a row or a neighbour of the other family, or on another page,
/// is `InvalidArg` naming the verb that orders it; so is a row named twice
/// and a neighbour that is one of the rows. A row or a neighbour that isn't
/// on the page is `NotFound`.
pub fn plan_reorder(
    stack: PageStack,
    family: AnnotationFamily,
    refs: &[AnnotationRef],
    position: &AnnotationPosition,
) -> Result<PlannedReorder, EngineError> {
    if refs.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a reorder names at least one row",
        )
        .with_details(serde_json::json!({ "field": "refs" })));
    }
    let at = |reference: &AnnotationRef| {
        let key = reference.key();
        stack
            .entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.reference.key() == key))
    };
    let anchor = position.anchor();
    let named = refs.iter().map(|r| (r, "refs")).chain(anchor.map(|a| (a, "position")));
    for (reference, field) in named {
        if reference.page() != stack.page {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "{} is on page {}; a reorder stays on page {}",
                    reference.key(),
                    reference.page().get(),
                    stack.page.get()
                ),
            )
            .with_details(serde_json::json!({ "field": field })));
        }
        if let Some(entry) = at(reference).and_then(|index| stack.entries[index].as_ref())
            && entry.family != family
        {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                other_family(entry.family),
            )
            .with_details(serde_json::json!({ "field": field })));
        }
    }

    let before = family_order(&stack, family);
    let after = reordered_list(&before, refs, position, AnnotationRef::key)?;

    // Every row is in the family's order now, so each has a position.
    let index_of = |reference: &AnnotationRef| at(reference).map(|i| i as u32);
    let from: Vec<u32> = refs.iter().filter_map(index_of).collect();
    // A position once the moving rows are out.
    let kept = |index: u32| index - from.iter().filter(|f| **f < index).count() as u32;
    let staying: Vec<u32> =
        before.iter().filter_map(index_of).filter(|index| !from.contains(index)).collect();
    let first = from.iter().copied().min().unwrap_or(0);
    let to = match position {
        ListPosition::End(End::Start) => staying.first().map_or(first, |index| kept(*index)),
        ListPosition::End(End::End) => staying.last().map_or(first, |index| kept(*index) + 1),
        ListPosition::Before { before: anchor } | ListPosition::After { after: anchor } => {
            let neighbour = kept(index_of(anchor).unwrap_or(0));
            if matches!(position, ListPosition::Before { .. }) { neighbour } else { neighbour + 1 }
        }
    };
    Ok(PlannedReorder { stack, family, before, after, from, to })
}

/// `applyReorder`: writes a planned reorder, one block move in the page's
/// `/Annots`. Promotion keeps every position, so the plan's still hold.
pub fn apply_reorder(document: &Document, plan: &PlannedReorder) -> Result<(), EngineError> {
    promote_inline_annotations(document, plan.stack.page_index, plan.stack.page)?;
    if !document.move_annotations_raw(plan.stack.page_index, &plan.from, plan.to) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the annotations of page {} could not be reordered", plan.stack.page.get()),
        ));
    }
    Ok(())
}

/// Why a row of `family` isn't ordered by the other family's verb.
fn other_family(family: AnnotationFamily) -> &'static str {
    match family {
        AnnotationFamily::Widgets => {
            "widgets paint above every annotation and have an order of their own: reorder them \
             with doc.forms.reorderWidgets"
        }
        AnnotationFamily::Annotations => {
            "annotations paint below every widget and have an order of their own: reorder them \
             with page.annotations.reorder"
        }
    }
}
