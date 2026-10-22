//! Destinations in page space, and the values that carry them.
//!
//! `core/src/pageSpace/destinations.ts` and `core/src/dto/PdfDestination.ts`
//! (`PageDestination`). A destination is measured on the page it goes to, so
//! converting one takes that page's visible box ([`super::VisibleBoxOf`]).

use serde::{Deserialize, Serialize};

use super::VisibleBoxOf;
use crate::error::EngineError;
use crate::features::actions::destination::PdfDestination;
use crate::features::actions::document::DocumentActionsSnapshot;
use crate::features::actions::reader::ActionTree;
use crate::identity::PageObjectNumber;

/// A destination (ISO 32000-1 §12.3.2.2) in page space: the page it goes to
/// and where on that page, measured from the top-left of that page's visible
/// box, y down (`PageDestination`).
///
/// `None` keeps the viewer's current value, as `null` in the file does, and
/// is written as `null`; an `xyz` zoom of 0 means the same. The reader always
/// says which axes the file left null, so this has no "left out" state apart
/// from that.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageDestination {
    Xyz {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        zoom: Option<f64>,
    },
    Fit {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
    },
    #[serde(rename = "fitH")]
    FitH {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        #[serde(default)]
        y: Option<f64>,
    },
    #[serde(rename = "fitV")]
    FitV {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        #[serde(default)]
        x: Option<f64>,
    },
    #[serde(rename = "fitR")]
    FitR {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    #[serde(rename = "fitB")]
    FitB {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
    },
    #[serde(rename = "fitBH")]
    FitBh {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        #[serde(default)]
        y: Option<f64>,
    },
    #[serde(rename = "fitBV")]
    FitBv {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        #[serde(default)]
        x: Option<f64>,
    },
}

impl PageDestination {
    pub fn page_object_number(&self) -> PageObjectNumber {
        match self {
            Self::Xyz { page_object_number, .. }
            | Self::Fit { page_object_number }
            | Self::FitH { page_object_number, .. }
            | Self::FitV { page_object_number, .. }
            | Self::FitR { page_object_number, .. }
            | Self::FitB { page_object_number }
            | Self::FitBh { page_object_number, .. }
            | Self::FitBv { page_object_number, .. } => *page_object_number,
        }
    }
}

/// A destination in the file's coordinates, in page space
/// (`pageDestinationOf`). `box_of` gives the visible box of the page it goes
/// to.
pub fn page_destination_of(
    destination: PdfDestination,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<PageDestination, EngineError> {
    let visible = box_of(destination.page_object_number())?;
    let x = |left: Option<f64>| left.map(|left| left - visible.left);
    let y = |top: Option<f64>| top.map(|top| visible.top - top);
    Ok(match destination {
        PdfDestination::Xyz { page_object_number, left, top, zoom } => {
            PageDestination::Xyz { page_object_number, x: x(left), y: y(top), zoom }
        }
        PdfDestination::FitH { page_object_number, top } => {
            PageDestination::FitH { page_object_number, y: y(top) }
        }
        PdfDestination::FitBh { page_object_number, top } => {
            PageDestination::FitBh { page_object_number, y: y(top) }
        }
        PdfDestination::FitV { page_object_number, left } => {
            PageDestination::FitV { page_object_number, x: x(left) }
        }
        PdfDestination::FitBv { page_object_number, left } => {
            PageDestination::FitBv { page_object_number, x: x(left) }
        }
        PdfDestination::FitR { page_object_number, left, bottom, right, top } => {
            let lower = bottom.min(top);
            let upper = bottom.max(top);
            PageDestination::FitR {
                page_object_number,
                x: left.min(right) - visible.left,
                y: visible.top - upper,
                width: (right - left).abs(),
                height: upper - lower,
            }
        }
        PdfDestination::Fit { page_object_number } => PageDestination::Fit { page_object_number },
        PdfDestination::FitB { page_object_number } => PageDestination::FitB { page_object_number },
    })
}

/// A page-space destination in the file's coordinates (`pdfDestinationOf`).
pub fn pdf_destination_of(
    destination: PageDestination,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<PdfDestination, EngineError> {
    let visible = box_of(destination.page_object_number())?;
    let left = |x: Option<f64>| x.map(|x| x + visible.left);
    let top = |y: Option<f64>| y.map(|y| visible.top - y);
    Ok(match destination {
        PageDestination::Xyz { page_object_number, x, y, zoom } => {
            PdfDestination::Xyz { page_object_number, left: left(x), top: top(y), zoom }
        }
        PageDestination::FitH { page_object_number, y } => {
            PdfDestination::FitH { page_object_number, top: top(y) }
        }
        PageDestination::FitBh { page_object_number, y } => {
            PdfDestination::FitBh { page_object_number, top: top(y) }
        }
        PageDestination::FitV { page_object_number, x } => {
            PdfDestination::FitV { page_object_number, left: left(x) }
        }
        PageDestination::FitBv { page_object_number, x } => {
            PdfDestination::FitBv { page_object_number, left: left(x) }
        }
        PageDestination::FitR { page_object_number, x, y, width, height } => {
            let upper = visible.top - y;
            PdfDestination::FitR {
                page_object_number,
                left: x + visible.left,
                bottom: upper - height,
                right: x + visible.left + width,
                top: upper,
            }
        }
        PageDestination::Fit { page_object_number } => PdfDestination::Fit { page_object_number },
        PageDestination::FitB { page_object_number } => PdfDestination::FitB { page_object_number },
    })
}

/// A page-space destination as the wire spells it, in the file's
/// coordinates, an axis left out staying out (`pdfDestinationOf`, which
/// writes `left` or `top` only `'x' in destination` or `'y' in
/// destination`). [`PageDestination`] reads a left-out axis as `null`; a
/// write treats the two the same, but an `expect` compares them apart.
pub fn pdf_destination_value_of(
    destination: serde_json::Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<serde_json::Value, EngineError> {
    let sent: PageDestination = serde_json::from_value(destination.clone()).map_err(|e| {
        EngineError::with_message(
            crate::error::EngineErrorCode::WireFormat,
            format!("a destination is not the shape it takes: {e}"),
        )
    })?;
    let mut converted = serde_json::to_value(pdf_destination_of(sent, box_of)?).map_err(|e| {
        EngineError::with_message(crate::error::EngineErrorCode::WireFormat, e.to_string())
    })?;
    let axes: &[(&str, &str)] = match sent {
        PageDestination::Xyz { .. } => &[("x", "left"), ("y", "top"), ("zoom", "zoom")],
        PageDestination::FitH { .. } | PageDestination::FitBh { .. } => &[("y", "top")],
        PageDestination::FitV { .. } | PageDestination::FitBv { .. } => &[("x", "left")],
        PageDestination::Fit { .. }
        | PageDestination::FitR { .. }
        | PageDestination::FitB { .. } => &[],
    };
    if let Some(out) = converted.as_object_mut() {
        for (axis, key) in axes {
            if destination.get(axis).is_none() {
                out.remove(*key);
            }
        }
    }
    Ok(converted)
}

/// An action tree with every destination in page space (`mapActionTree` with
/// `pageDestinationOf`).
pub fn page_action_tree_of(
    tree: ActionTree<PdfDestination>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<ActionTree<PageDestination>, EngineError> {
    tree.try_map_destinations(&mut |destination| page_destination_of(destination, box_of))
}

/// The catalog's actions with every destination in page space
/// (`mapDocumentActions` with `pageDestinationOf`).
pub fn page_document_actions_of(
    snapshot: DocumentActionsSnapshot<PdfDestination>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<DocumentActionsSnapshot<PageDestination>, EngineError> {
    snapshot.try_map_destinations(&mut |destination| page_destination_of(destination, box_of))
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/pageSpace/destinations.test.ts`.

    use super::*;
    use crate::features::actions::reader::{ActionNode, ActionNodeKind};
    use crate::features::actions::vocabulary::ActionWarningFlags;
    use crate::geom::PdfRect;

    fn page(n: u32) -> PageObjectNumber {
        PageObjectNumber::new(n).expect("a page")
    }

    fn a() -> PageObjectNumber {
        page(4)
    }

    fn b() -> PageObjectNumber {
        page(6)
    }

    fn box_of(page: PageObjectNumber) -> Result<PdfRect, EngineError> {
        Ok(match page.get() {
            4 => PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 },
            6 => PdfRect { left: -300.0, bottom: -390.0, right: 300.0, top: 390.0 },
            other => panic!("no page {other}"),
        })
    }

    fn to_page(destination: PdfDestination) -> PageDestination {
        page_destination_of(destination, &mut box_of).expect("converts")
    }

    fn to_pdf(destination: PageDestination) -> PdfDestination {
        pdf_destination_of(destination, &mut box_of).expect("converts")
    }

    #[test]
    fn each_kind_is_measured_on_its_own_page_and_comes_back() {
        use PageDestination as G;
        use PdfDestination as P;
        let cases = [
            (
                P::Xyz {
                    page_object_number: b(),
                    left: Some(-250.0),
                    top: Some(350.0),
                    zoom: Some(0.0),
                },
                G::Xyz { page_object_number: b(), x: Some(50.0), y: Some(40.0), zoom: Some(0.0) },
            ),
            (
                P::Xyz { page_object_number: b(), left: None, top: None, zoom: None },
                G::Xyz { page_object_number: b(), x: None, y: None, zoom: None },
            ),
            (
                P::FitH { page_object_number: a(), top: Some(700.0) },
                G::FitH { page_object_number: a(), y: Some(92.0) },
            ),
            (
                P::FitV { page_object_number: a(), left: Some(30.0) },
                G::FitV { page_object_number: a(), x: Some(30.0) },
            ),
            (
                P::FitBh { page_object_number: b(), top: None },
                G::FitBh { page_object_number: b(), y: None },
            ),
            (
                P::FitBv { page_object_number: b(), left: Some(0.0) },
                G::FitBv { page_object_number: b(), x: Some(300.0) },
            ),
            (
                P::FitR {
                    page_object_number: b(),
                    left: -200.0,
                    bottom: -100.0,
                    right: 100.0,
                    top: 300.0,
                },
                G::FitR { page_object_number: b(), x: 100.0, y: 90.0, width: 300.0, height: 400.0 },
            ),
            (P::Fit { page_object_number: a() }, G::Fit { page_object_number: a() }),
            (P::FitB { page_object_number: b() }, G::FitB { page_object_number: b() }),
        ];
        for (pdf, page) in cases {
            assert_eq!(to_page(pdf), page, "{pdf:?}");
            assert_eq!(to_pdf(page), pdf, "{page:?}");
        }
    }

    #[test]
    fn a_fit_r_given_by_its_other_corners_comes_out_upright() {
        let turned = PdfDestination::FitR {
            page_object_number: a(),
            left: 200.0,
            bottom: 700.0,
            right: 100.0,
            top: 600.0,
        };
        assert_eq!(
            to_page(turned),
            PageDestination::FitR {
                page_object_number: a(),
                x: 100.0,
                y: 92.0,
                width: 100.0,
                height: 100.0
            }
        );
    }

    #[test]
    fn the_box_can_be_the_target_pages_own_instead_of_a_lookup() {
        let destination = PdfDestination::Xyz {
            page_object_number: b(),
            left: Some(-250.0),
            top: Some(350.0),
            zoom: Some(0.0),
        };
        let own = box_of(b()).unwrap();
        let mut one = |_| Ok(own);
        let page = page_destination_of(destination, &mut one).unwrap();
        assert_eq!(page, to_page(destination));
        assert_eq!(pdf_destination_of(page, &mut one).unwrap(), destination);
    }

    #[test]
    fn a_page_the_lookup_does_not_know_is_its_error() {
        let mut none = |page: PageObjectNumber| -> Result<PdfRect, EngineError> {
            Err(EngineError::with_message(
                crate::error::EngineErrorCode::NotFound,
                format!("no page with object number {}", page.get()),
            ))
        };
        let refused =
            page_destination_of(PdfDestination::Fit { page_object_number: a() }, &mut none)
                .expect_err("no box");
        assert_eq!(refused.code, crate::error::EngineErrorCode::NotFound);
    }

    #[test]
    fn a_null_axis_is_written_as_null() {
        let page = to_page(PdfDestination::Xyz {
            page_object_number: a(),
            left: None,
            top: Some(500.0),
            zoom: None,
        });
        assert_eq!(
            serde_json::to_value(page).unwrap(),
            serde_json::json!({
                "kind": "xyz", "page": { "kind": "objectNumber", "objectNumber": 4 },
                "x": null, "y": 292.0, "zoom": null,
            })
        );
    }

    fn node(kind: ActionNodeKind, subtype: &str, next: Vec<ActionNode>) -> ActionNode {
        ActionNode { kind, subtype: subtype.into(), next }
    }

    #[test]
    fn every_goto_in_an_action_tree_converts_down_its_next_chain() {
        let tree = ActionTree {
            incomplete: false,
            warning_flags: ActionWarningFlags(0),
            warnings: Vec::new(),
            root: Some(node(
                ActionNodeKind::Goto {
                    destination: PdfDestination::FitH { page_object_number: a(), top: Some(700.0) },
                },
                "GoTo",
                vec![
                    node(
                        ActionNodeKind::Uri { uri: "https://x".into(), is_map: false },
                        "URI",
                        Vec::new(),
                    ),
                    node(
                        ActionNodeKind::Goto {
                            destination: PdfDestination::Xyz {
                                page_object_number: b(),
                                left: Some(-250.0),
                                top: Some(350.0),
                                zoom: None,
                            },
                        },
                        "GoTo",
                        Vec::new(),
                    ),
                ],
            )),
        };
        let page = page_action_tree_of(tree, &mut box_of).unwrap();
        let root = page.root.expect("root");
        assert_eq!(
            root.kind,
            ActionNodeKind::Goto {
                destination: PageDestination::FitH { page_object_number: a(), y: Some(92.0) }
            }
        );
        assert!(matches!(root.next[0].kind, ActionNodeKind::Uri { .. }));
        assert_eq!(
            root.next[1].kind,
            ActionNodeKind::Goto {
                destination: PageDestination::Xyz {
                    page_object_number: b(),
                    x: Some(50.0),
                    y: Some(40.0),
                    zoom: None
                }
            }
        );
    }

    #[test]
    fn a_documents_open_destination_converts() {
        let snapshot = DocumentActionsSnapshot {
            will_close: None,
            will_save: None,
            did_save: None,
            will_print: None,
            did_print: None,
            name_tree_scripts: Vec::new(),
            open_action: None,
            open_destination: Some(PdfDestination::Xyz {
                page_object_number: b(),
                left: Some(-250.0),
                top: Some(350.0),
                zoom: None,
            }),
        };
        let page = page_document_actions_of(snapshot, &mut box_of).unwrap();
        assert_eq!(
            page.open_destination,
            Some(PageDestination::Xyz {
                page_object_number: b(),
                x: Some(50.0),
                y: Some(40.0),
                zoom: None
            })
        );
        assert!(page.name_tree_scripts.is_empty() && page.open_action.is_none());
    }
}
