//! The page list in page space (`core/src/pageSpace/pages.ts`).

use std::collections::HashMap;

use super::destinations::page_destination_of;
use super::{PageCoordinates, PdfCoordinates, VisibleBoxOf, page_rect_of};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::pages::{PageLayout, PageListSnapshot};
use crate::geom::{PageBoxes, PageRect, PdfRect};
use crate::identity::PageObjectNumber;

/// A page's five boxes in page space, measured from the top-left of the crop
/// box, the visible page, which is therefore `{x: 0, y: 0, width, height}`
/// (`pageSpaceBoxesOf`).
pub fn page_space_boxes_of(boxes: PageBoxes) -> PageBoxes<PageRect> {
    let visible = boxes.crop;
    PageBoxes {
        media: page_rect_of(boxes.media, visible),
        crop: page_rect_of(boxes.crop, visible),
        bleed: page_rect_of(boxes.bleed, visible),
        trim: page_rect_of(boxes.trim, visible),
        art: page_rect_of(boxes.art, visible),
    }
}

/// A page's layout in page space (`pageLayoutOf`). Its actions may go to
/// other pages, so each destination is measured with `box_of`, the box of
/// the page it goes to.
pub fn page_layout_of(
    layout: PageLayout<PdfCoordinates>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<PageLayout<PageCoordinates>, EngineError> {
    let actions = layout
        .actions
        .map(|actions| {
            actions.try_map_destinations(&mut |destination| {
                page_destination_of(destination, &mut *box_of)
            })
        })
        .transpose()?;
    Ok(PageLayout::<PageCoordinates> {
        index: layout.index,
        page_object_number: layout.page_object_number,
        label: layout.label,
        size: layout.size,
        rotation: layout.rotation,
        user_unit: layout.user_unit,
        tab_order: layout.tab_order,
        boxes: page_space_boxes_of(layout.boxes),
        pdf_crop_box: layout.pdf_crop_box,
        actions,
    })
}

/// Every page's visible box, by page, from the pages' layouts
/// (`visibleBoxesOf`). A page that isn't in the list is `InvalidArg`, as
/// upstream's is.
pub fn visible_boxes_of(
    pages: &[PageLayout<PdfCoordinates>],
) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + use<> {
    let boxes: HashMap<u32, PdfRect> =
        pages.iter().map(|page| (page.page_object_number.get(), page.boxes.crop)).collect();
    move |page| {
        boxes.get(&page.get()).copied().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("No page {} in this document", page.get()),
            )
        })
    }
}

/// The document's pages in page space (`pageListOf`). A page list carries
/// every page's box itself, so it needs no document.
pub fn page_list_of(
    snapshot: PageListSnapshot<PdfCoordinates>,
) -> Result<PageListSnapshot<PageCoordinates>, EngineError> {
    let mut box_of = visible_boxes_of(&snapshot.pages);
    let pages = snapshot
        .pages
        .into_iter()
        .map(|page| page_layout_of(page, &mut box_of))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PageListSnapshot {
        page_count: snapshot.page_count,
        pages,
        named_pages: snapshot.named_pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::actions::destination::PdfDestination;
    use crate::features::actions::reader::{ActionNode, ActionNodeKind, ActionTree};
    use crate::features::actions::vocabulary::ActionWarningFlags;
    use crate::features::pages::PageActions;
    use crate::geom::{PdfRotation, PdfSize};
    use crate::page_space::destinations::PageDestination;

    fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    fn layout(number: u32, boxes: PageBoxes, goes_to: Option<u32>) -> PageLayout {
        let page = |n| PageObjectNumber::new(n).unwrap();
        PageLayout {
            index: 0,
            page_object_number: page(number),
            label: None,
            size: PdfSize {
                width: boxes.crop.right - boxes.crop.left,
                height: boxes.crop.top - boxes.crop.bottom,
            },
            rotation: PdfRotation::Clockwise90,
            user_unit: 1.0,
            tab_order: None,
            boxes,
            pdf_crop_box: boxes.crop,
            actions: goes_to.map(|to| PageActions {
                open: None,
                close: Some(ActionTree {
                    root: Some(ActionNode {
                        kind: ActionNodeKind::Goto {
                            destination: PdfDestination::Xyz {
                                page_object_number: page(to),
                                left: Some(100.0),
                                top: Some(700.0),
                                zoom: None,
                            },
                        },
                        subtype: "GoTo".into(),
                        next: Vec::new(),
                    }),
                    incomplete: false,
                    warning_flags: ActionWarningFlags(0),
                    warnings: Vec::new(),
                }),
            }),
        }
    }

    #[test]
    fn the_crop_box_is_the_origin_and_the_others_are_measured_from_it() {
        let media = rect(0.0, 0.0, 612.0, 792.0);
        let crop = rect(50.0, 60.0, 562.0, 732.0);
        let trim = rect(100.0, 100.0, 500.0, 700.0);
        let boxes = page_space_boxes_of(PageBoxes { media, crop, bleed: crop, trim, art: crop });
        assert_eq!(boxes.crop, PageRect { x: 0.0, y: 0.0, width: 512.0, height: 672.0 });
        assert_eq!(boxes.media, PageRect { x: -50.0, y: -60.0, width: 612.0, height: 792.0 });
        assert_eq!(boxes.trim, PageRect { x: 50.0, y: 32.0, width: 400.0, height: 600.0 });
    }

    #[test]
    fn a_destination_is_measured_on_the_page_it_goes_to_and_the_turn_changes_nothing() {
        let letter = rect(0.0, 0.0, 612.0, 792.0);
        let shifted = rect(-306.0, -396.0, 306.0, 396.0);
        let first = layout(
            3,
            PageBoxes { media: letter, crop: letter, bleed: letter, trim: letter, art: letter },
            Some(4),
        );
        let second = layout(
            4,
            PageBoxes {
                media: shifted,
                crop: shifted,
                bleed: shifted,
                trim: shifted,
                art: shifted,
            },
            None,
        );
        let list = page_list_of(PageListSnapshot {
            page_count: 2,
            pages: vec![first, second],
            named_pages: Vec::new(),
        })
        .unwrap();
        let first = &list.pages[0];
        // A 90-degree page is still 612 by 792 and its boxes are not turned.
        assert_eq!(first.size, PdfSize { width: 612.0, height: 792.0 });
        assert_eq!(first.boxes.crop, PageRect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 });
        assert_eq!(first.pdf_crop_box, letter);
        let close = first.actions.as_ref().unwrap().close.as_ref().unwrap();
        assert_eq!(
            close.root.as_ref().unwrap().kind,
            ActionNodeKind::Goto {
                destination: PageDestination::Xyz {
                    page_object_number: PageObjectNumber::new(4).unwrap(),
                    x: Some(406.0),
                    y: Some(-304.0),
                    zoom: None,
                },
            },
        );
    }

    #[test]
    fn a_destination_to_a_page_not_in_the_list_is_refused() {
        let letter = rect(0.0, 0.0, 612.0, 792.0);
        let only = layout(
            3,
            PageBoxes { media: letter, crop: letter, bleed: letter, trim: letter, art: letter },
            Some(9),
        );
        let refused = page_list_of(PageListSnapshot {
            page_count: 1,
            pages: vec![only],
            named_pages: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    }
}
