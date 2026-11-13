//! `pages.setScale` and its reverse
//! (`services/src/features/changes/internal/scaleChanges.ts`, c7f8d75ff).
//!
//! Each page's own viewport gets the scale. The op's measure is in page
//! space (its origin), so each page takes it on its own visible box. Its
//! reverse puts back the scale each page had, while the page still has the
//! one this change left. A page's scale belongs to its measurements: writing
//! one takes what writing them does (`doc.annotate.modify`).

use serde_json::Value;

use super::applier::{ChangeContext, Done, left_alone};
use super::item::ChangeItem;
use super::record::{PageScaleStep, ReverseStep};
use crate::auth::scope::DocCapability;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::measure::{own_scale_of, set_scale};
use crate::features::pages::{list_pages, read_boxes};
use crate::geom::PdfRect;
use crate::identity::{PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::page_space::measure::{page_measure_of, pdf_measure_of};

fn authorize(ctx: &ChangeContext<'_>) -> Result<(), EngineError> {
    ctx.authority.require(DocCapability::DocAnnotateModify)
}

/// `pages.setScale` (`setPageScales`): the reverse has one step for each
/// scale and box the pages had, in the order each first appears.
pub fn set_page_scales(
    ctx: &mut ChangeContext<'_>,
    pages: &[PageRef],
    measure: Option<&Value>,
) -> Result<Done, EngineError> {
    authorize(ctx)?;
    let numbers: Vec<u32> = pages.iter().map(|page| page.object_number().get()).collect();
    for (i, page) in numbers.iter().enumerate() {
        if numbers[..i].contains(page) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "pages.setScale names a page twice",
            )
            .with_details(serde_json::json!({ "field": "pages" })));
        }
    }
    let mut steps: Vec<(Value, PageScaleStep)> = Vec::new();
    for page in &numbers {
        let visible = visible_box_of(ctx, *page)?;
        let before = scale_of(ctx, *page)?;
        let written = measure.map(|measure| pdf_measure_of(measure.clone(), visible));
        write(ctx, *page, written.as_ref())?;
        let left = scale_of(ctx, *page)?;
        let key = serde_json::json!([before, left, visible_key(visible)]);
        match steps.iter_mut().find(|(k, _)| *k == key) {
            Some((_, step)) => step.pages.push(*page),
            None => steps.push((key, PageScaleStep { pages: vec![*page], measure: before, left })),
        }
    }
    let first = pages.first().copied().ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "pages.setScale names no page")
    })?;
    Ok(Done {
        item: item_of(ctx, pages.to_vec(), measure.cloned(), first)?,
        reverse: steps.into_iter().map(|(_, step)| ReverseStep::PagesSetScale(step)).collect(),
    })
}

/// Gives a step's pages its scale, those that still have the scale it left
/// (`setPageScalesBack`).
pub fn set_page_scales_back(
    ctx: &mut ChangeContext<'_>,
    step: &PageScaleStep,
) -> Result<Done, EngineError> {
    let mut pages = Vec::new();
    for page in &step.pages {
        if scale_of(ctx, *page)? == step.left {
            pages.push(*page);
        }
    }
    let Some(&first) = pages.first() else {
        return left_alone(ctx, "pages.setScale");
    };
    authorize(ctx)?;
    for page in &pages {
        write(ctx, *page, step.measure.as_ref())?;
    }
    let refs = pages.iter().map(|page| page_ref(*page)).collect::<Result<Vec<_>, _>>()?;
    // A step's pages share their size, so the first one's box says where the
    // scale is on each.
    let visible = visible_box_of(ctx, first)?;
    let measure = step.measure.clone().map(|measure| page_measure_of(measure, visible));
    let first = refs[0];
    Ok(Done {
        item: item_of(ctx, refs, measure, first)?,
        reverse: vec![ReverseStep::PagesSetScale(PageScaleStep {
            pages,
            measure: step.left.clone(),
            left: step.measure.clone(),
        })],
    })
}

fn write(
    ctx: &mut ChangeContext<'_>,
    page: u32,
    measure: Option<&Value>,
) -> Result<(), EngineError> {
    let owned = ctx.session.document()?;
    let index = owned.page_index(page_number(page)?)?;
    set_scale(owned.document(), index, measure, ctx.cancel)
}

/// What a scale change answers: its pages and measure, its first page, and
/// the layout that carries every page's viewports (`resultOf`).
fn item_of(
    ctx: &mut ChangeContext<'_>,
    pages: Vec<PageRef>,
    measure: Option<Value>,
    page: PageRef,
) -> Result<ChangeItem, EngineError> {
    let layout = list_pages(ctx.session.document()?.document(), ctx.cancel)?;
    Ok(ChangeItem::PagesSetScale {
        pages,
        measure,
        page,
        layout: serde_json::to_value(&layout).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("a layout did not serialize: {e}"),
            )
        })?,
        meta: MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?),
    })
}

/// The scale of the last viewport the engine owns on the page, in the file's
/// coordinates, or `None` (`ownScaleOf`).
fn scale_of(ctx: &mut ChangeContext<'_>, page: u32) -> Result<Option<Value>, EngineError> {
    let owned = ctx.session.document()?;
    let index = owned.page_index(page_number(page)?)?;
    own_scale_of(owned.document(), index, ctx.cancel)
}

/// The page's visible box in the file's coordinates (`visibleBoxOf`).
fn visible_box_of(ctx: &mut ChangeContext<'_>, page: u32) -> Result<PdfRect, EngineError> {
    let owned = ctx.session.document()?;
    let index = owned.page_index(page_number(page)?)?;
    Ok(read_boxes(owned.document(), index).crop)
}

/// A box as a grouping key: two pages whose boxes are equal share a step.
fn visible_key(visible: PdfRect) -> Value {
    serde_json::json!([visible.left, visible.bottom, visible.right, visible.top])
}

fn page_number(page: u32) -> Result<PageObjectNumber, EngineError> {
    PageObjectNumber::new(page).map_err(|_| {
        EngineError::with_message(EngineErrorCode::NotFound, format!("no page {page}"))
    })
}

fn page_ref(page: u32) -> Result<PageRef, EngineError> {
    Ok(PageRef::new(page_number(page)?))
}
