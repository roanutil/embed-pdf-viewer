//! `write/measurement.ts`: the measurement seam. The per-page viewports
//! (scale regions) the measurement plugin supplies for dimension drafts,
//! and recalibration of a page's dimensions to a new scale.

use std::cell::RefCell;

use epdf_annotation_core::measure::{
    DimensionInput, PdfMeasurement, is_dimension, measurement_readout,
};
use epdf_annotation_core::{
    AnnotationRef, PageObjectNumber, PageRef, PdfPoint, PdfRect, annot_contents_editable,
    annot_transformable,
};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::host_contract::{RecalibrationReport, SkipReason, serialize_error};
use crate::promise;
use crate::value::{get, pdf_point_of, str_of};

/// `PageMeasurementViewport`: a scale region in PDF space.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PageMeasurementViewport {
    pub bbox: PdfRect,
    #[serde(default)]
    pub measure: Option<PdfMeasurement>,
    #[serde(default)]
    pub owned: bool,
}

/// One page's viewports and the fallback scale.
#[derive(Clone, Debug)]
pub(crate) struct PageViewportsEntry {
    pub viewports: Option<Vec<PageMeasurementViewport>>,
    pub fallback: Option<PdfMeasurement>,
}

#[derive(Default)]
pub(crate) struct PageViewports(RefCell<Vec<(PageObjectNumber, PageViewportsEntry)>>);

impl PageViewports {
    pub(crate) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(crate) fn of(&self, pon: PageObjectNumber) -> Option<PageViewportsEntry> {
        self.0.borrow().iter().find(|(p, _)| *p == pon).map(|(_, e)| e.clone())
    }
}

/// `viewportForPoint(viewports, point)`: the last containing viewport wins.
pub(crate) fn viewport_for_point(
    viewports: &[PageMeasurementViewport],
    point: PdfPoint,
) -> Option<&PageMeasurementViewport> {
    viewports.iter().rev().find(|v| {
        let b = v.bbox;
        point.x >= b.left && point.x <= b.right && point.y >= b.bottom && point.y <= b.top
    })
}

/// The measurement readout of a dimension DTO under a scale, the input
/// `measurementReadout({ ...dto, measure })` sees.
fn readout_available(dto: &Value, measure: Option<&PdfMeasurement>) -> bool {
    let subtype = str_of(dto, "subtype").unwrap_or_default();
    let intent = str_of(dto, "intent").unwrap_or_default();
    let line_points = get(dto, "linePoints")
        .map(|lp| (pdf_point_of(get(lp, "start")), pdf_point_of(get(lp, "end"))));
    let vertices: Vec<PdfPoint> = get(dto, "vertices")
        .and_then(Value::as_array)
        .map(|vs| vs.iter().map(|v| pdf_point_of(Some(v))).collect())
        .unwrap_or_default();
    measurement_readout(&DimensionInput { subtype, intent, measure, line_points, vertices })
        .readout()
        .is_some()
}

impl AnnotationCapability {
    /// `setPageViewports(page, viewports, fallback)` (host).
    pub fn set_page_viewports(
        &self,
        page: PageRef,
        viewports: Option<Vec<PageMeasurementViewport>>,
        fallback: Option<PdfMeasurement>,
    ) {
        let pon = page.page_object_number;
        let entry = PageViewportsEntry { viewports, fallback };
        let mut all = self.0.viewports.0.borrow_mut();
        match all.iter_mut().find(|(p, _)| *p == pon) {
            Some(slot) => slot.1 = entry,
            None => all.push((pon, entry)),
        }
    }

    /// `remeasurePage(page, scale)` (host): rehydrate, then rewrite the
    /// scale of every dimension on the page it can.
    pub fn remeasure_page(&self, page: PageRef, scale: Value) -> Promise<RecalibrationReport> {
        let this = self.clone();
        promise::spawn(async move {
            let pon = page.page_object_number;
            this.rehydrate().await?;
            let mut report = RecalibrationReport {
                page,
                scale: scale.clone(),
                error: None,
                updated: Vec::new(),
                skipped: Vec::new(),
                failed: Vec::new(),
            };
            match this.state().hydration.clone() {
                AnnotationHydration::Complete => {}
                AnnotationHydration::Error(error) => {
                    report.error = Some(serialize_error(&error));
                    return Ok(report);
                }
                _ => {
                    report.error =
                        Some(serialize_error(&Error::new("Annotation hydration is incomplete")));
                    return Ok(report);
                }
            }
            let measure: Option<PdfMeasurement> = serde_json::from_value(scale.clone()).ok();
            let m = this.model();
            // `Object.values(byId)`: insertion order, which the paint order
            // follows (the Rust map is sorted, so walk `order`).
            let candidates: Vec<_> = m
                .order
                .iter()
                .filter_map(|id| m.by_id.get(id))
                .filter(|a| {
                    a.page.page_object_number == pon
                        && a.data.as_ref().is_some_and(|d| {
                            is_dimension(
                                str_of(d, "subtype").unwrap_or_default(),
                                str_of(d, "intent").unwrap_or_default(),
                            )
                        })
                })
                .cloned()
                .collect();
            for a in candidates {
                let dto = a.data.clone().expect("a dimension carries its DTO");
                let Some(r) = get(&dto, "ref")
                    .and_then(|r| serde_json::from_value::<AnnotationRef>(r.clone()).ok())
                else {
                    continue;
                };
                let foreign = get(&dto, "measure")
                    .filter(|m| crate::value::truthy(Some(m)))
                    .is_some_and(|m| str_of(m, "subtype") != Some("RL"));
                let reason = if a.authority.is_some_and(|x| !x.update) {
                    Some(SkipReason::NoAuthority)
                } else if !annot_transformable(&a) || !annot_contents_editable(&a) {
                    Some(SkipReason::Locked)
                } else if foreign {
                    Some(SkipReason::ForeignMeasure)
                } else if !readout_available(&dto, measure.as_ref()) {
                    Some(SkipReason::Unavailable)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    report.skipped.push((r, reason));
                    continue;
                }
                let patch = json!({
                    "subtype": get(&dto, "subtype").cloned().unwrap_or(Value::Null),
                    "measure": scale.clone(),
                });
                match this.update_raw(r.clone(), patch).await {
                    Ok(()) => report.updated.push(r),
                    Err(error) => report.failed.push((r, serialize_error(&error))),
                }
            }
            Ok(report)
        })
    }
}
