//! The page ops and their reverse
//! (`services/src/features/changes/internal/pageChanges.ts`).
//!
//! `pages.rotate`: its reverse turns each page back while it still has the
//! rotation the op left. `pages.reorder`: its reverse puts each page back
//! after the page it followed, while it is still where the reorder left it.
//! `pages.delete`: each page is captured first (the fork's page capture),
//! and its reverse brings the same pages back at the same numbers
//! (`pages.restore`), whose reverse deletes them again. `pages.insertBlank`
//! and `pages.insert`: their reverse deletes the new pages while nobody
//! changed them (`pages.remove`), guarded by a digest of each page. So an
//! undone insert can be redone, and an undone delete deleted again.
//!
//! Each op checks `doc.pages.assemble` against the change's authority, as
//! its single verb did. A page op changes the page tree, so the session's
//! page registry is rebuilt inside the op: a later op of the same change
//! resolves pages against the document as this one left it. Upstream also
//! closes the idle parsed pages around a page op (`changingPageTree`); this
//! engine keeps no page past its last holder (`session::pages`), so there
//! is none to close.

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::record::{
    CapturedPage, LeftPage, PagePlacement, PageRemoveStep, PageReorderStep, PageRestoreStep,
    PageRotateStep, ReverseStep,
};
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::pages::{list_pages, rotation_at};
use crate::features::pages_write::{
    self as mutator, PageInsertResult, PageLayoutResult, PagePosition,
};
use crate::geom::PdfRotation;
use crate::identity::{PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::mutation::list_position::{End, ListPosition};

/// What a page's guard digests: the page, and what the layer holds of these
/// (`PAGE_DEEP_KEYS`).
const PAGE_DEEP_KEYS: &str = "Annots Contents Resources";

/// What it leaves out of the page's bytes: its place (`/Parent`), so moving
/// pages around it changes nothing, and its `/Rotate`, which counts by
/// value instead: turning a page and turning it back reads the same
/// (`PAGE_SKIPPED_KEYS`).
const PAGE_SKIPPED_KEYS: &str = "Parent Rotate";

fn authorize(ctx: &ChangeContext<'_>) -> Result<(), EngineError> {
    ctx.authority.require(DocCapability::DocPagesAssemble)
}

/// `pages.rotate`; its reverse turns each page back while it still has this
/// rotation (`rotatePages`).
pub fn rotate_pages(
    ctx: &mut ChangeContext<'_>,
    pages: &[PageRef],
    rotation: &serde_json::Number,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let before = rotations_of(ctx)?;
    let result = mutator::rotate_pages(ctx.session, pages, rotation, ctx.cancel)?;
    // Checked by the rotate: one of the four.
    let left = rotation_of(rotation)?;
    let numbers: Vec<u32> = pages.iter().map(|page| page.object_number().get()).collect();
    Ok(Done {
        item: rotate_item(pages.to_vec(), left, result)?,
        reverse: rotate_steps(&numbers, &before, left),
    })
}

/// Turns pages to a step's rotation, those that still have the rotation it
/// left (`rotatePagesBack`).
pub fn rotate_pages_back(
    ctx: &mut ChangeContext<'_>,
    step: &PageRotateStep,
) -> Result<Done, EngineError> {
    let now = rotations_of(ctx)?;
    let pages: Vec<u32> = step
        .pages
        .iter()
        .copied()
        .filter(|page| now.iter().any(|(p, r)| p == page && *r == step.left))
        .collect();
    if pages.is_empty() {
        return left_alone(ctx, "pages.rotate");
    }
    authorize(ctx)?;
    let refs = page_refs(&pages)?;
    let degrees = serde_json::Number::from(step.rotation.degrees());
    let result = mutator::rotate_pages(ctx.session, &refs, &degrees, ctx.cancel)?;
    Ok(Done {
        item: rotate_item(refs, step.rotation, result)?,
        reverse: vec![ReverseStep::PagesRotate(PageRotateStep {
            pages,
            rotation: step.left,
            left: step.rotation,
        })],
    })
}

/// `pages.reorder`; its reverse puts each page back after the page it
/// followed (`reorderPages`).
pub fn reorder_pages(
    ctx: &mut ChangeContext<'_>,
    pages: &[PageRef],
    position: &PagePosition,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let numbers: Vec<u32> = pages.iter().map(|page| page.object_number().get()).collect();
    let back = placements_of(&order_of(ctx)?, &numbers);
    let result = mutator::reorder_pages(ctx.session, pages, position, ctx.cancel)?;
    let left = placements_of(&order_of(ctx)?, &numbers);
    Ok(Done {
        item: ChangeItem::PagesReorder {
            pages: result.pages,
            layout: to_value(&result.layout)?,
            meta: result.meta,
        },
        reverse: vec![ReverseStep::PagesReorder(PageReorderStep { placements: back, left })],
    })
}

/// Puts pages back where a move found them: each still after the page the
/// move left it after, the page it goes after still there
/// (`reorderPagesBack`).
pub fn reorder_pages_back(
    ctx: &mut ChangeContext<'_>,
    step: &PageReorderStep,
) -> Result<Done, EngineError> {
    let order = order_of(ctx)?;
    let placements: Vec<PagePlacement> = step
        .placements
        .iter()
        .copied()
        .filter(|placement| {
            let Some(at) = order.iter().position(|page| *page == placement.page) else {
                return false;
            };
            let follows = if at == 0 { 0 } else { order[at - 1] };
            let left_after =
                step.left.iter().find(|left| left.page == placement.page).map(|left| left.after);
            Some(follows) == left_after
                && (placement.after == 0 || order.contains(&placement.after))
        })
        .collect();
    if placements.is_empty() {
        return left_alone(ctx, "pages.reorder");
    }
    authorize(ctx)?;
    let pages: Vec<u32> = placements.iter().map(|placement| placement.page).collect();
    let back = placements_of(&order, &pages);
    let mut layout = None;
    for placement in &placements {
        let position = if placement.after == 0 {
            ListPosition::End(End::Start)
        } else {
            ListPosition::After { after: page_ref(placement.after)? }
        };
        let moved = mutator::reorder_pages(
            ctx.session,
            &[page_ref(placement.page)?],
            &position,
            ctx.cancel,
        )?;
        layout = Some(moved.layout);
    }
    let left = placements_of(&order_of(ctx)?, &pages);
    Ok(Done {
        item: ChangeItem::PagesReorder {
            pages: page_refs(&pages)?,
            layout: to_value(&layout)?,
            meta: meta_of(ctx)?,
        },
        reverse: vec![ReverseStep::PagesReorder(PageReorderStep { placements: back, left })],
    })
}

/// `pages.delete`, captured in its transaction so an undo brings back the
/// same pages (`deletePages`).
pub fn delete_pages(ctx: &mut ChangeContext<'_>, pages: &[PageRef]) -> Result<Done, EngineError> {
    authorize(ctx)?;
    // NotFound for a page the document doesn't have.
    let resolved = mutator::resolve_page_refs(ctx.session, pages)?;
    let numbers: Vec<u32> = resolved.iter().map(|page| page.get()).collect();
    let pages = in_display_order(&order_of(ctx)?, &numbers);
    write_delete(ctx, &pages)
}

/// Brings back pages a delete removed, those that aren't back already
/// (`restorePages`).
pub fn restore_pages(
    ctx: &mut ChangeContext<'_>,
    step: &PageRestoreStep,
) -> Result<Done, EngineError> {
    let order = order_of(ctx)?;
    let gone: Vec<&CapturedPage> =
        step.pages.iter().filter(|captured| !order.contains(&captured.page)).collect();
    if gone.is_empty() {
        return left_alone(ctx, "pages.restore");
    }
    authorize(ctx)?;
    {
        // An import runs inside the change's layer transaction. In the
        // display order they had: each page it followed is back before it.
        let document = ctx.session.document()?.document();
        for captured in &gone {
            if !document.import_page_raw(&captured.capture) {
                return Err(unknown("EPDFPage_ImportRaw refused a capture"));
            }
        }
    }
    ctx.session.refresh_page_registry()?;
    let numbers: Vec<u32> = gone.iter().map(|captured| captured.page).collect();
    let pages = in_display_order(&order_of(ctx)?, &numbers);
    let layout = list_pages(ctx.session.document()?.document(), ctx.cancel)?;
    let reverse = remove_step_of(ctx, &pages)?;
    Ok(Done {
        item: ChangeItem::PagesRestore {
            pages: page_refs(&pages)?,
            layout: to_value(&layout)?,
            meta: meta_of(ctx)?,
        },
        reverse: vec![reverse],
    })
}

/// Deletes pages an insert or a restore brought, those that read as the
/// change left them; never every page of the document (`removePages`).
pub fn remove_pages(
    ctx: &mut ChangeContext<'_>,
    step: &PageRemoveStep,
) -> Result<Done, EngineError> {
    let order = order_of(ctx)?;
    let mut pages = Vec::new();
    for left in &step.pages {
        if order.contains(&left.page) && digest_of(ctx, left.page)? == left.left {
            pages.push(left.page);
        }
    }
    if pages.is_empty() || pages.len() >= order.len() {
        return left_alone(ctx, "pages.delete");
    }
    authorize(ctx)?;
    let pages = in_display_order(&order, &pages);
    write_delete(ctx, &pages)
}

/// `pages.insertBlank`; its reverse deletes the new pages while nobody
/// changed them (`insertBlankPages`).
pub fn insert_blank_pages(
    ctx: &mut ChangeContext<'_>,
    spec: &serde_json::Map<String, serde_json::Value>,
    position: Option<&PagePosition>,
    object_numbers: Option<&[serde_json::Number]>,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let result = mutator::insert_blank_pages(
        ctx.session,
        spec,
        position.unwrap_or(&ListPosition::End(End::End)),
        object_numbers,
        ctx.cancel,
    )?;
    let (item, reverse) = inserted(ctx, result, false)?;
    Ok(Done { item, reverse })
}

/// `pages.insert`; its reverse deletes the pages it copied in while nobody
/// changed them (`insertPages`).
pub fn insert_pages(
    ctx: &mut ChangeContext<'_>,
    source: &[u8],
    position: Option<&PagePosition>,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let result = mutator::insert_pages(
        ctx.session,
        source,
        position.unwrap_or(&ListPosition::End(End::End)),
        ctx.cancel,
    )?;
    let (item, reverse) = inserted(ctx, result, true)?;
    Ok(Done { item, reverse })
}

/// An insert's item, and the step that deletes its pages again.
fn inserted(
    ctx: &ChangeContext<'_>,
    result: PageInsertResult,
    copied: bool,
) -> Result<(ChangeItem, Vec<ReverseStep>), EngineError> {
    let pages: Vec<u32> =
        result.inserted_pages.iter().map(|page| page.object_number().get()).collect();
    let reverse = vec![remove_step_of(ctx, &pages)?];
    let layout = to_value(&result.layout)?;
    let item = if copied {
        ChangeItem::PagesInsert { inserted_pages: result.inserted_pages, layout, meta: result.meta }
    } else {
        ChangeItem::PagesInsertBlank {
            inserted_pages: result.inserted_pages,
            layout,
            meta: result.meta,
        }
    };
    Ok((item, reverse))
}

/// Deletes `pages`, in display order, each captured first so its restore
/// brings it back (`writeDelete`).
fn write_delete(ctx: &mut ChangeContext<'_>, pages: &[u32]) -> Result<Done, EngineError> {
    // Every capture before any page goes, so each holds the page it
    // followed.
    let captures = {
        let document = ctx.session.document()?.document();
        pages
            .iter()
            .map(|page| {
                let capture = document
                    .export_page_raw(*page)
                    .ok_or_else(|| unknown(&format!("the capture of page {page} failed")))?;
                Ok(CapturedPage { page: *page, capture })
            })
            .collect::<Result<Vec<_>, EngineError>>()?
    };
    let refs = page_refs(pages)?;
    let result: PageLayoutResult = mutator::delete_pages(ctx.session, &refs, ctx.cancel)?;
    Ok(Done {
        item: ChangeItem::PagesDelete {
            pages: refs,
            layout: to_value(&result.layout)?,
            meta: result.meta,
        },
        reverse: vec![ReverseStep::PagesRestore(PageRestoreStep { pages: captures })],
    })
}

/// The step that deletes `pages` again while each reads as it does now
/// (`removeStepOf`).
fn remove_step_of(ctx: &ChangeContext<'_>, pages: &[u32]) -> Result<ReverseStep, EngineError> {
    let pages = pages
        .iter()
        .map(|page| Ok(LeftPage { page: *page, left: digest_of(ctx, *page)? }))
        .collect::<Result<_, EngineError>>()?;
    Ok(ReverseStep::PagesRemove(PageRemoveStep { pages }))
}

/// How a page reads now (`digestOf`): the SHA-256 of its dictionary and of
/// what the layer holds of its annotations, contents and resources, as they
/// read (canonical bytes: a save that only encodes a stream again changes
/// nothing), without its place, and with its turn as a value.
fn digest_of(ctx: &ChangeContext<'_>, page: u32) -> Result<String, EngineError> {
    let owned = ctx.session.document()?;
    let document = owned.document();
    let mut bytes = document
        .export_dict_canonical(page, PAGE_DEEP_KEYS, PAGE_SKIPPED_KEYS)
        .ok_or_else(|| unknown(&format!("the capture of object {page} failed")))?;
    let turn = (0..document.page_count())
        .find(|index| document.page_object_number_by_index(*index) == page)
        .map_or(0, |index| rotation_at(document, index).degrees());
    bytes.extend_from_slice(format!("\nturn {turn}").as_bytes());
    let digest = pdfium::sha256(&bytes).ok_or_else(|| unknown("EPDF_DigestBuffer refused"))?;
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The steps that turn each page back, one for each rotation the pages had
/// (`rotateSteps`), in the order the rotations first appear.
fn rotate_steps(
    pages: &[u32],
    before: &[(u32, PdfRotation)],
    left: PdfRotation,
) -> Vec<ReverseStep> {
    let mut groups: Vec<(PdfRotation, Vec<u32>)> = Vec::new();
    for page in pages {
        let Some((_, rotation)) = before.iter().find(|(p, _)| p == page) else {
            continue;
        };
        match groups.iter_mut().find(|(r, _)| r == rotation) {
            Some((_, group)) => group.push(*page),
            None => groups.push((*rotation, vec![*page])),
        }
    }
    groups
        .into_iter()
        .map(|(rotation, pages)| ReverseStep::PagesRotate(PageRotateStep { pages, rotation, left }))
        .collect()
}

/// Where each of `pages` is in `order`: the page it follows (0: first), in
/// display order (`placementsOf`).
fn placements_of(order: &[u32], pages: &[u32]) -> Vec<PagePlacement> {
    in_display_order(order, pages)
        .into_iter()
        .map(|page| {
            let at = order.iter().position(|p| *p == page);
            let after = match at {
                Some(at) if at > 0 => order[at - 1],
                _ => 0,
            };
            PagePlacement { page, after }
        })
        .collect()
}

/// `pages` sorted as `order` has them (`inDisplayOrder`).
fn in_display_order(order: &[u32], pages: &[u32]) -> Vec<u32> {
    let at = |page: &u32| order.iter().position(|p| p == page).unwrap_or(usize::MAX);
    let mut sorted = pages.to_vec();
    sorted.sort_by_key(at);
    sorted
}

/// The document's pages by object number, in display order (`orderOf`).
fn order_of(ctx: &mut ChangeContext<'_>) -> Result<Vec<u32>, EngineError> {
    Ok(ctx.session.all_records()?.iter().map(|record| record.page_object_number.get()).collect())
}

/// Each page's rotation now (`rotationsOf`).
fn rotations_of(ctx: &ChangeContext<'_>) -> Result<Vec<(u32, PdfRotation)>, EngineError> {
    let document = ctx.session.document()?.document();
    Ok((0..document.page_count())
        .map(|index| (document.page_object_number_by_index(index), rotation_at(document, index)))
        .collect())
}

fn rotate_item(
    pages: Vec<PageRef>,
    rotation: PdfRotation,
    result: PageLayoutResult,
) -> Result<ChangeItem, EngineError> {
    Ok(ChangeItem::PagesRotate {
        pages,
        rotation,
        layout: to_value(&result.layout)?,
        meta: result.meta,
    })
}

fn rotation_of(rotation: &serde_json::Number) -> Result<PdfRotation, EngineError> {
    rotation
        .as_f64()
        .filter(|r| r.fract() == 0.0 && (0.0..=270.0).contains(r))
        .and_then(|r| PdfRotation::try_from(r as u16).ok())
        .ok_or_else(|| unknown(&format!("{rotation} is not a rotation")))
}

fn meta_of(ctx: &ChangeContext<'_>) -> Result<MutationMeta, EngineError> {
    Ok(MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?))
}

fn page_ref(page: u32) -> Result<PageRef, EngineError> {
    Ok(PageRef::new(PageObjectNumber::new(page)?))
}

fn page_refs(pages: &[u32]) -> Result<Vec<PageRef>, EngineError> {
    pages.iter().map(|page| page_ref(*page)).collect()
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, EngineError> {
    serde_json::to_value(value).map_err(|e| unknown(&format!("a layout did not serialize: {e}")))
}

fn unknown(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_names_the_page_before_or_none() {
        let order = [3, 4, 5, 6];
        assert_eq!(
            placements_of(&order, &[5, 3]),
            [PagePlacement { page: 3, after: 0 }, PagePlacement { page: 5, after: 4 }],
        );
        assert_eq!(in_display_order(&order, &[6, 3, 5]), [3, 5, 6]);
    }

    #[test]
    fn a_rotation_reverses_as_one_step_per_rotation_the_pages_had() {
        let before =
            [(3, PdfRotation::None), (4, PdfRotation::Clockwise90), (5, PdfRotation::None)];
        let steps = rotate_steps(&[4, 3, 5], &before, PdfRotation::Clockwise180);
        assert_eq!(
            steps,
            [
                ReverseStep::PagesRotate(PageRotateStep {
                    pages: vec![4],
                    rotation: PdfRotation::Clockwise90,
                    left: PdfRotation::Clockwise180,
                }),
                ReverseStep::PagesRotate(PageRotateStep {
                    pages: vec![3, 5],
                    rotation: PdfRotation::None,
                    left: PdfRotation::Clockwise180,
                }),
            ]
        );
    }
}
