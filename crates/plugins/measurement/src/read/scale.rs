//! `read/scale.ts`: the page scale, the vocabulary, readouts, and pure
//! conversions in the page's scale.

use epdf_annotation_core::measure::{DimensionInput, PdfMeasurement, measurement_readout};
use epdf_annotation_core::{AnnotationRef, PageRef, PdfPoint, Point};
use epdf_kernel::Error;
use epdf_plugin_annotation::value::{get, pdf_point_of, str_of, truthy};
use serde_json::Value;

use crate::contract::*;
use crate::controller::MeasurementCapability;

/// `Object.keys(METRES)`.
pub const UNITS: &[&str] = &["pt", "mm", "cm", "m", "km", "in", "ft", "yd", "mi"];

/// `[...UNITS.map(squareOf), 'ha', 'acre']`.
pub const AREA_UNITS: &[&str] =
    &["pt2", "mm2", "cm2", "m2", "km2", "in2", "ft2", "yd2", "mi2", "ha", "acre"];

/// `measurementReadout(dto)` for an engine DTO read as JSON.
pub fn readout_of_dto(dto: &Value) -> Readout {
    let measure: Option<PdfMeasurement> = get(dto, "measure")
        .filter(|m| truthy(Some(m)))
        .and_then(|m| serde_json::from_value(m.clone()).ok());
    let line_points = get(dto, "linePoints")
        .filter(|lp| truthy(Some(lp)))
        .map(|lp| (pdf_point_of(get(lp, "start")), pdf_point_of(get(lp, "end"))));
    let vertices: Vec<PdfPoint> = get(dto, "vertices")
        .and_then(Value::as_array)
        .map(|vs| vs.iter().map(|v| pdf_point_of(Some(v))).collect())
        .unwrap_or_default();
    measurement_readout(&DimensionInput {
        subtype: str_of(dto, "subtype").unwrap_or_default(),
        intent: str_of(dto, "intent").unwrap_or_default(),
        measure: measure.as_ref(),
        line_points,
        vertices,
    })
}

impl MeasurementCapability {
    /// Create authority and a ready scale on the page.
    pub fn can_measure(&self, page: &PageRef) -> bool {
        self.s().annotation.can_create() && self.s().scale_of(page).ready
    }

    /// The resolved scale, its source and readiness.
    pub fn get_page_scale(&self, page: &PageRef) -> PageScale {
        self.s().scale_of(page)
    }

    /// A scale change is in flight.
    pub fn is_busy(&self) -> bool {
        self.s().state().pending > 0.0
    }

    /// The reports of the last scale change.
    pub fn list_last_reports(&self) -> Vec<ScaleChangeReport> {
        self.s().state().reports.clone()
    }

    pub fn list_presets(&self) -> Vec<ScalePreset> {
        self.0.presets.clone()
    }

    pub fn list_units(&self) -> Vec<&'static str> {
        UNITS.to_vec()
    }

    pub fn list_area_units(&self) -> Vec<&'static str> {
        AREA_UNITS.to_vec()
    }

    /// The formatted measurement of a measurement annotation.
    pub fn get_readout(&self, r: &AnnotationRef) -> Readout {
        match self.s().annotation.get_raw(r) {
            Some(raw) if !raw.is_null() => readout_of_dto(&raw),
            _ => Readout::Unavailable { unavailable: Unavailable::NotDimension },
        }
    }

    /// A distance in the page's scale, with no annotation. `Err` is a page
    /// that is not in the document.
    pub fn measure_distance(
        &self,
        page: &PageRef,
        from: Point,
        to: Point,
    ) -> Result<Readout, Error> {
        let measure = self.s().scale_of(page).measure;
        let start = self.s().to_pdf(page, from)?;
        let end = self.s().to_pdf(page, to)?;
        Ok(measurement_readout(&DimensionInput {
            subtype: "line",
            intent: "LineDimension",
            measure: measure.as_ref(),
            line_points: Some((start, end)),
            vertices: Vec::new(),
        }))
    }

    /// An area in the page's scale, with no annotation.
    pub fn measure_area(&self, page: &PageRef, vertices: &[Point]) -> Result<Readout, Error> {
        let measure = self.s().scale_of(page).measure;
        let vertices =
            vertices.iter().map(|v| self.s().to_pdf(page, *v)).collect::<Result<Vec<_>, _>>()?;
        Ok(measurement_readout(&DimensionInput {
            subtype: "polygon",
            intent: "PolygonDimension",
            measure: measure.as_ref(),
            line_points: None,
            vertices,
        }))
    }

    /// Two points captured, awaiting a length.
    pub fn get_calibration_request(&self) -> Option<CalibrationRequest> {
        self.s().state().calibration.clone()
    }
}
