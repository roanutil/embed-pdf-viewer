//! `model.ts`: the measurement slice. Per-page viewports and resolved scale
//! (keyed by page object number), the busy counter, the pending
//! calibration and the last change's reports.

use std::collections::BTreeMap;

use epdf_annotation_core::{PageObjectNumber, PageRef};
use serde::{Deserialize, Serialize};

use crate::contract::{CalibrationRequest, MeasurementViewport, PageScale, ScaleChangeReport};

/// One page's entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageEntry {
    pub viewports: Vec<MeasurementViewport>,
    pub scale: PageScale,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementState {
    #[serde(deserialize_with = "pages_by_key")]
    pub pages: BTreeMap<PageObjectNumber, PageEntry>,
    pub pending: f64,
    pub calibration: Option<CalibrationRequest>,
    pub reports: Vec<ScaleChangeReport>,
}

/// Object keys are strings on the wire; the slice only ever holds page
/// object numbers.
fn pages_by_key<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PageObjectNumber, PageEntry>, D::Error> {
    BTreeMap::<String, PageEntry>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, entry)| {
            key.parse().map(|pon| (pon, entry)).map_err(|_| {
                serde::de::Error::custom(format!("page key {key:?} is not a page object number"))
            })
        })
        .collect()
}

// The TypeScript action union; the page-scale arm carries the whole page
// entry, which is what it dispatches.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MeasurementAction {
    #[serde(rename = "PAGE_SCALE")]
    PageScale { page: PageRef, viewports: Vec<MeasurementViewport>, scale: PageScale },
    #[serde(rename = "PENDING")]
    Pending { delta: f64 },
    #[serde(rename = "CALIBRATION")]
    Calibration { request: Option<CalibrationRequest> },
    #[serde(rename = "REPORTS")]
    Reports { reports: Vec<ScaleChangeReport> },
    /// An action the reducer does not know; it returns the state as it is.
    #[serde(other)]
    Unknown,
}

pub fn initial_measurement_state() -> MeasurementState {
    MeasurementState::default()
}

/// Pure. `None` is the TypeScript's `return s`.
pub fn measurement_reducer(
    s: &MeasurementState,
    a: &MeasurementAction,
) -> Option<MeasurementState> {
    let mut next = s.clone();
    match a {
        MeasurementAction::PageScale { page, viewports, scale } => {
            next.pages.insert(
                page.page_object_number,
                PageEntry { viewports: viewports.clone(), scale: scale.clone() },
            );
        }
        MeasurementAction::Pending { delta } => {
            next.pending = epdf_geometry::js::max(0.0, s.pending + delta);
        }
        MeasurementAction::Calibration { request } => next.calibration = request.clone(),
        MeasurementAction::Reports { reports } => next.reports = reports.clone(),
        MeasurementAction::Unknown => return None,
    }
    Some(next)
}
