//! Explicit PDF destinations.
//!
//! G6. A transcription of `services/src/features/destinations/*` and
//! `core/src/dto/PdfDestination.ts`.
//!
//! A page, a location and a magnification, per ISO 32000-1 12.3.2.2. Named
//! destinations resolve through the catalog before reaching here, so only the
//! explicit form exists in this type and a viewer never sees a name.
//!
//! Coordinates are PDF user space: y-up, points, absolute rather than
//! crop-relative. `None` means "retain the current value", which is the spec's
//! meaning for a null array entry. The wire hands destinations out in page
//! space ([`crate::page_space::destinations::PageDestination`]); this is the
//! file's form, which readers make and writers take.

use serde::{Deserialize, Serialize};

use pdfium::RawDestination;

use crate::identity::PageObjectNumber;

/// Where a destination puts the reader.
///
/// The optional coordinates serialize as `null` rather than being omitted.
/// `null` is the spec's "retain the current value", which is a thing the
/// document said; an absent key would be indistinguishable from a producer
/// that never wrote the entry, and the TypeScript emits the key either way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PdfDestination {
    /// A point and a zoom. Any of the three may retain the current value.
    Xyz {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        left: Option<f64>,
        top: Option<f64>,
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
        top: Option<f64>,
    },
    #[serde(rename = "fitV")]
    FitV {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        left: Option<f64>,
    },
    /// A rectangle, the one kind whose coordinates are all required.
    #[serde(rename = "fitR")]
    FitR {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        left: f64,
        bottom: f64,
        right: f64,
        top: f64,
    },
    /// The page's content bounding box rather than its crop box.
    #[serde(rename = "fitB")]
    FitB {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
    },
    #[serde(rename = "fitBH")]
    FitBh {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        top: Option<f64>,
    },
    #[serde(rename = "fitBV")]
    FitBv {
        #[serde(rename = "page", with = "crate::identity::page_ref")]
        page_object_number: PageObjectNumber,
        left: Option<f64>,
    },
}

impl PdfDestination {
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

/// `PDFDEST_VIEW_*` from `public/fpdf_doc.h`.
mod view {
    pub const XYZ: i32 = 1;
    pub const FIT: i32 = 2;
    pub const FIT_H: i32 = 3;
    pub const FIT_V: i32 = 4;
    pub const FIT_R: i32 = 5;
    pub const FIT_B: i32 = 6;
    pub const FIT_BH: i32 = 7;
    pub const FIT_BV: i32 = 8;
}

/// Materialize a destination.
///
/// `None` when it is unreadable: no visible indirect page, or a fit type this
/// build does not know. Coordinates stay raw; no conversion happens here.
pub fn read_destination(raw: &RawDestination<'_>) -> Option<PdfDestination> {
    let page_object_number = PageObjectNumber::new(raw.page_object_number()).ok()?;
    let view = raw.view();
    // A null parameter (the viewer keeps its value) reads as `None`, not the
    // 0 `FPDFDest_GetView` reports for it.
    let nulls = raw.view_null_params();
    let at = |i: usize| -> Option<f64> {
        (i < view.param_count && nulls & (1 << i) == 0).then(|| f64::from(view.params[i]))
    };

    Some(match view.code {
        view::XYZ => {
            let location = raw.location();
            PdfDestination::Xyz {
                page_object_number,
                left: location.left.map(f64::from),
                top: location.top.map(f64::from),
                zoom: location.zoom.map(f64::from),
            }
        }
        view::FIT => PdfDestination::Fit { page_object_number },
        view::FIT_H => PdfDestination::FitH { page_object_number, top: at(0) },
        view::FIT_V => PdfDestination::FitV { page_object_number, left: at(0) },
        view::FIT_R => {
            // `/FitR` is only meaningful with the whole rect. A malformed one
            // degrades to the whole page rather than inventing coordinates a
            // viewer would then scroll to.
            if view.param_count < 4 {
                PdfDestination::Fit { page_object_number }
            } else {
                PdfDestination::FitR {
                    page_object_number,
                    left: f64::from(view.params[0]),
                    bottom: f64::from(view.params[1]),
                    right: f64::from(view.params[2]),
                    top: f64::from(view.params[3]),
                }
            }
        }
        view::FIT_B => PdfDestination::FitB { page_object_number },
        view::FIT_BH => PdfDestination::FitBh { page_object_number, top: at(0) },
        view::FIT_BV => PdfDestination::FitBv { page_object_number, left: at(0) },
        _ => return None,
    })
}
