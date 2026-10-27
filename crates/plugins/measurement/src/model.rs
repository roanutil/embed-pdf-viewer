//! `model.ts`: the measurement session. Scale changes in flight, the pending
//! calibration, the last change's reports, why a page's viewport read
//! failed, and the session-only viewports of an engine that cannot persist
//! a scale. The pages' viewports themselves are engine data held by a page
//! mirror. Every transition is pure and answers the same `Rc` when nothing
//! changes, so the kernel records no state change; the controller applies
//! them with `update_state`. `select_page_scale` and `page_scale_of` are the
//! pure projections from viewports to the public [`PageScale`].

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_kernel::geometry::Point;
use epdf_kernel::mirror::ResourceStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{CalibrationRequest, PageScale, ScaleChangeReport};
use crate::units::viewport_for_point;

/// `MeasurementState`, in the TypeScript's member order. Object keys are
/// page object numbers, which a JavaScript object lists in ascending order,
/// as the `BTreeMap`s do.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementState {
    /// Scale changes in flight.
    pub pending: f64,
    pub calibration: Option<CalibrationRequest>,
    pub reports: Rc<Vec<ScaleChangeReport>>,
    /// Page object number → why the last read of the page's viewports
    /// failed (a serialized engine error). Shared, so a page's scale can
    /// tell a recorded error from an equal one recorded again, as the
    /// TypeScript's identity check does.
    pub load_errors: BTreeMap<u32, Rc<Value>>,
    /// Page object number → the page's viewports, when the engine has no
    /// page measure service.
    pub local_viewports: BTreeMap<u32, Value>,
}

/// `initialMeasurementState()`.
pub fn initial_measurement_state() -> MeasurementState {
    MeasurementState::default()
}

/// `beginScaleChange`.
pub fn begin_scale_change(state: &Rc<MeasurementState>) -> Rc<MeasurementState> {
    Rc::new(MeasurementState { pending: state.pending + 1.0, ..(**state).clone() })
}

/// `endScaleChange`: the busy counter never goes below zero.
pub fn end_scale_change(state: &Rc<MeasurementState>) -> Rc<MeasurementState> {
    if state.pending == 0.0 {
        return state.clone();
    }
    Rc::new(MeasurementState { pending: state.pending - 1.0, ..(**state).clone() })
}

/// `setCalibration`: `null` over `null` changes nothing; any request is a
/// new object to the TypeScript's identity check.
pub fn set_calibration(
    state: &Rc<MeasurementState>,
    calibration: Option<CalibrationRequest>,
) -> Rc<MeasurementState> {
    if state.calibration.is_none() && calibration.is_none() {
        return state.clone();
    }
    Rc::new(MeasurementState { calibration, ..(**state).clone() })
}

/// `setReports`: the same list changes nothing.
pub fn set_reports(
    state: &Rc<MeasurementState>,
    reports: Rc<Vec<ScaleChangeReport>>,
) -> Rc<MeasurementState> {
    if Rc::ptr_eq(&state.reports, &reports) {
        return state.clone();
    }
    Rc::new(MeasurementState { reports, ..(**state).clone() })
}

/// `recordLoadError`.
pub fn record_load_error(
    state: &Rc<MeasurementState>,
    page_object_number: u32,
    error: Value,
) -> Rc<MeasurementState> {
    let mut next = (**state).clone();
    next.load_errors.insert(page_object_number, Rc::new(error));
    Rc::new(next)
}

/// `clearLoadError`: a page with no error changes nothing.
pub fn clear_load_error(
    state: &Rc<MeasurementState>,
    page_object_number: u32,
) -> Rc<MeasurementState> {
    if !state.load_errors.contains_key(&page_object_number) {
        return state.clone();
    }
    let mut next = (**state).clone();
    next.load_errors.remove(&page_object_number);
    Rc::new(next)
}

/// `setLocalViewports`.
pub fn set_local_viewports(
    state: &Rc<MeasurementState>,
    page_object_number: u32,
    viewports: Value,
) -> Rc<MeasurementState> {
    let mut next = (**state).clone();
    next.local_viewports.insert(page_object_number, viewports);
    Rc::new(next)
}

/// `PdfSize`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

impl PageSize {
    /// A layout's `size`; a missing member reads `NaN`.
    pub fn of(size: &Value) -> PageSize {
        let read = |key: &str| {
            size.get(key).and_then(epdf_annotation_core::json::number).unwrap_or(f64::NAN)
        };
        PageSize { width: read("width"), height: read("height") }
    }
}

fn truthy(value: Option<&Value>) -> bool {
    epdf_annotation_core::json::truthy(value)
}

/// `selectPageScale(viewports, size, fallback, persistent)`: the owned
/// viewport, else the one containing the page's centre, else the fallback.
pub fn select_page_scale(
    viewports: &[Value],
    size: PageSize,
    fallback: &Value,
    persistent: bool,
) -> PageScale {
    let selected = viewports.iter().find(|v| truthy(v.get("owned"))).or_else(|| {
        viewport_for_point(viewports, Point { x: size.width / 2.0, y: size.height / 2.0 })
    });
    let (measure, source) = match selected {
        Some(viewport) => {
            let measure = match viewport.get("measure") {
                None | Some(Value::Null) => Value::Null,
                Some(m) if epdf_kernel::is_undefined(m) => Value::Null,
                Some(m) => m.clone(),
            };
            (measure, if truthy(viewport.get("owned")) { "owned" } else { "foreign" })
        }
        None => (fallback.clone(), "default"),
    };
    PageScale { measure, source, ready: true, persistent, error: None }
}

/// `PageScaleInputs`: the page's mirrored viewports and read state, and its
/// layout.
#[derive(Clone, Debug)]
pub struct PageScaleInputs<'a> {
    /// The last confirmed viewports; `None` until the first read lands.
    pub viewports: Option<&'a Value>,
    pub status: ResourceStatus,
    pub error: Option<&'a Value>,
    /// The page's size; `None` when the page is not in the registry.
    pub size: Option<PageSize>,
    pub fallback: &'a Value,
    pub persistent: bool,
}

/// `pageScaleOf(inputs)`: a page is ready once its viewports were read; a
/// failed re-read keeps the last confirmed viewports in effect and reports
/// its error alongside them.
pub fn page_scale_of(inputs: &PageScaleInputs) -> PageScale {
    let error = (inputs.status == ResourceStatus::Error)
        .then_some(inputs.error)
        .flatten()
        .filter(|e| truthy(Some(e)))
        .cloned();
    let (Some(viewports), Some(size)) = (inputs.viewports, inputs.size) else {
        return PageScale {
            measure: Value::Null,
            source: "default",
            ready: false,
            persistent: inputs.persistent,
            error,
        };
    };
    let list = viewports.as_array().map(Vec::as_slice).unwrap_or(&[]);
    PageScale { error, ..select_page_scale(list, size, inputs.fallback, inputs.persistent) }
}
