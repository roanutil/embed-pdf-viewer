//! `read/scale.ts`: the page scale, the vocabulary, readouts, and pure
//! conversions in the page's scale.

use std::rc::Rc;

use epdf_annotation_core::engine::measure::{DimensionInput, measurement_readout};
use epdf_kernel::Error;
use epdf_kernel::geometry::Point;
use serde_json::{Map, Value, json};

use crate::contract::{
    CalibrationRequest, PageInput, PageScale, ScaleChangeReport, ScalePreset, no_readout,
};
use crate::controller::MeasurementCapability;
use crate::units::{METRES, square_of};

/// `UNITS`: `Object.keys(METRES)`.
pub fn units() -> Vec<&'static str> {
    METRES.iter().map(|(unit, _)| *unit).collect()
}

/// `AREA_UNITS`: each unit squared, then `ha` and `acre`.
pub fn area_units() -> Vec<String> {
    let mut list: Vec<String> = units().into_iter().map(square_of).collect();
    list.extend(["ha".to_string(), "acre".to_string()]);
    list
}

fn point(p: Point) -> Value {
    json!({ "x": p.x, "y": p.y })
}

/// `measurementReadout(dto)` over a JSON object.
fn readout(object: &Map<String, Value>) -> Value {
    measurement_readout(&DimensionInput::of(object)).to_value()
}

impl MeasurementCapability {
    /// May measure on the page: `annotations:create`, and the page's scale
    /// has loaded.
    pub fn can_measure(&self, page: impl Into<PageInput>) -> Result<bool, Error> {
        Ok(self.core().cx.allows("annotations:create") && self.scale_of(page.into())?.ready)
    }

    /// The resolved scale, its source and readiness. The same `Rc` while
    /// unchanged; a page that isn't there is not ready and has no measure.
    pub fn get_page_scale(&self, page: impl Into<PageInput>) -> Result<Rc<PageScale>, Error> {
        self.scale_of(page.into())
    }

    /// A scale change is in flight.
    pub fn is_busy(&self) -> bool {
        self.core().cx.state().pending > 0.0
    }

    /// The reports of the last scale change, one per page.
    pub fn list_last_reports(&self) -> Rc<Vec<ScaleChangeReport>> {
        self.core().cx.state().reports.clone()
    }

    /// The `presets` setting.
    pub fn list_presets(&self) -> Vec<ScalePreset> {
        self.get_settings().presets
    }

    /// The `presets` setting as the store holds it.
    pub fn list_presets_json(&self) -> Value {
        self.settings_json().get("presets").cloned().unwrap_or(Value::Null)
    }

    pub fn list_units(&self) -> Vec<&'static str> {
        units()
    }

    pub fn list_area_units(&self) -> Vec<String> {
        area_units()
    }

    /// The formatted measurement of a measurement annotation, or why there
    /// is none (`NO_READOUT` for an annotation that isn't there).
    pub fn get_readout(&self, reference: &Value) -> Value {
        match self.core().annotation.get(reference) {
            Some(Value::Object(raw)) => readout(&raw),
            Some(_) => readout(&Map::new()),
            None => no_readout(),
        }
    }

    /// A distance between two page-space points in the page's scale: a pure
    /// conversion, no annotation.
    pub fn measure_distance(
        &self,
        page: impl Into<PageInput>,
        from: Point,
        to: Point,
    ) -> Result<Value, Error> {
        let measure = self.scale_of(page.into())?.measure.clone();
        let mut dto = Map::new();
        dto.insert("subtype".into(), json!("line"));
        dto.insert("intent".into(), json!("line-dimension"));
        dto.insert("measure".into(), measure);
        dto.insert("linePoints".into(), json!({ "start": point(from), "end": point(to) }));
        Ok(readout(&dto))
    }

    /// The area of a page-space polygon in the page's scale: a pure
    /// conversion, no annotation.
    pub fn measure_area(
        &self,
        page: impl Into<PageInput>,
        vertices: &[Point],
    ) -> Result<Value, Error> {
        let measure = self.scale_of(page.into())?.measure.clone();
        let mut dto = Map::new();
        dto.insert("subtype".into(), json!("polygon"));
        dto.insert("intent".into(), json!("polygon-dimension"));
        dto.insert("measure".into(), measure);
        dto.insert("vertices".into(), Value::Array(vertices.iter().copied().map(point).collect()));
        Ok(readout(&dto))
    }

    /// Two points captured, awaiting a length.
    pub fn get_calibration_request(&self) -> Option<CalibrationRequest> {
        self.core().cx.state().calibration.clone()
    }
}
