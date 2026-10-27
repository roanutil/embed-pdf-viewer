//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. The generator is
//! `tooling/plugin-conformance/src/suites/measurement.ts`, whose
//! `measurement-current` factory is `measurementPlugin(config)` with no shim.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::geometry::Point;
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, is_undefined, undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::model::{
    MeasurementState, PageScaleInputs, PageSize, begin_scale_change, clear_load_error,
    end_scale_change, set_calibration, set_local_viewports, set_reports,
};
use crate::write::scale::reports_json;
use crate::*;

/// The adapter.
pub struct MeasurementAdapter;

/// The capability object literal's member order: the areas' slices as
/// `composeApi` spreads them (settings, reads, scale writes, calibration,
/// measuring), then the service-owned members.
pub const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "canMeasure",
    "getPageScale",
    "isBusy",
    "listLastReports",
    "listPresets",
    "listUnits",
    "listAreaUnits",
    "getReadout",
    "measureDistance",
    "measureArea",
    "getCalibrationRequest",
    "setScale",
    "calibrate",
    "setUnit",
    "setAreaUnit",
    "setPrecision",
    "setPreset",
    "clearScale",
    "startCalibration",
    "dismissCalibration",
    "createMeasurement",
    "canCalibrate",
    "ensureLoaded",
    "onScaleChanged",
    "onCalibrationRequested",
    "onCalibrationCompleted",
    "onCalibrationDismissed",
];

fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(undefined)
}

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

fn page_arg(args: &[Value], index: usize) -> PageInput {
    PageInput::from_json(&arg(args, index))
}

fn point_arg(args: &[Value], index: usize) -> Result<Point, Error> {
    from_value(&arg(args, index))
}

/// `ScaleChangeOptions`: only `recalculate` can travel in a vector.
fn change_options(options: Option<&Value>) -> ScaleChangeOptions {
    ScaleChangeOptions {
        signal: None,
        recalculate: options.and_then(|o| o.get("recalculate")).and_then(Value::as_bool),
    }
}

fn str_arg(args: &[Value], index: usize) -> String {
    match arg(args, index) {
        Value::String(s) => s,
        other => epdf_annotation_core::json::stringify(&other),
    }
}

fn reports(promise: epdf_kernel::Promise<Rc<Vec<ScaleChangeReport>>>) -> Outcome {
    Outcome::Promise(promise.map(|reports| Ok(reports_json(&reports))))
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: 'static>(
    h: &Harness,
    spec: Option<&Value>,
    print: fn(&E) -> Value,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![print(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn calibrate_input(value: &Value) -> Result<CalibrateInput, Error> {
    let distance = value.get("distance").unwrap_or(&Value::Null);
    Ok(CalibrateInput {
        page: PageInput::from_json(value.get("page").unwrap_or(&Value::Null)),
        from: from_value(value.get("from").unwrap_or(&Value::Null))?,
        to: from_value(value.get("to").unwrap_or(&Value::Null))?,
        distance: KnownLength {
            value: distance.get("value").and_then(Value::as_f64).unwrap_or(f64::NAN),
            unit: distance.get("unit").and_then(Value::as_str).unwrap_or("undefined").into(),
        },
    })
}

fn create_input(value: &Value) -> Result<CreateMeasurementInput, Error> {
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or_default();
    Ok(CreateMeasurementInput {
        kind: MeasurementKind::parse(kind).ok_or_else(|| {
            Error::named("TypeError", format!("unknown measurement kind {kind:?}"))
        })?,
        page: PageInput::from_json(value.get("page").unwrap_or(&Value::Null)),
        points: from_value(value.get("points").unwrap_or(&json!([])))?,
        tool: value.get("tool").and_then(Value::as_str).map(str::to_string),
    })
}

fn call_capability(
    cap: &MeasurementCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    Ok(match method {
        "getSettings" => Outcome::Returned((*cap.settings_json()).clone()),
        "updateSettings" => {
            cap.update_settings_json(&arg(args, 0));
            Outcome::undefined()
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let callback = h.callback(args.first().unwrap_or(&Value::Null));
            let off = cap.on_settings_changed_json(move |settings, changed| {
                match callback.call(vec![json!({ "settings": settings, "changed": changed })]) {
                    Answer::Throws(error) => Err(error),
                    _ => Ok(()),
                }
            });
            Outcome::Returned(h.register_fn(Rc::new(move |_| {
                off.call();
                Outcome::undefined()
            })))
        }
        "canMeasure" => Outcome::Returned(json!(cap.can_measure(page_arg(args, 0))?)),
        "getPageScale" => Outcome::Returned(cap.get_page_scale(page_arg(args, 0))?.to_json()),
        "isBusy" => Outcome::Returned(json!(cap.is_busy())),
        "listLastReports" => Outcome::Returned(reports_json(&cap.list_last_reports())),
        "listPresets" => Outcome::Returned(cap.list_presets_json()),
        "listUnits" => Outcome::Returned(json!(cap.list_units())),
        "listAreaUnits" => Outcome::Returned(json!(cap.list_area_units())),
        "getReadout" => Outcome::Returned(cap.get_readout(&arg(args, 0))),
        "measureDistance" => Outcome::Returned(cap.measure_distance(
            page_arg(args, 0),
            point_arg(args, 1)?,
            point_arg(args, 2)?,
        )?),
        "measureArea" => {
            let vertices: Vec<Point> = from_value(&arg(args, 1))?;
            Outcome::Returned(cap.measure_area(page_arg(args, 0), &vertices)?)
        }
        "getCalibrationRequest" => Outcome::Returned(to_value(&cap.get_calibration_request())),
        "setScale" => reports(cap.set_scale(
            PageTarget::from_json(&arg(args, 0)),
            arg(args, 1),
            change_options(present(args, 2)),
        )),
        "calibrate" => {
            let options = present(args, 1);
            let apply_to = options
                .and_then(|o| o.get("applyTo"))
                .filter(|v| !is_undefined(v))
                .map(PageTarget::from_json);
            reports(cap.calibrate(
                calibrate_input(&arg(args, 0))?,
                CalibrateOptions { change: change_options(options), apply_to },
            ))
        }
        "setUnit" => reports(cap.set_unit(
            PageTarget::from_json(&arg(args, 0)),
            &str_arg(args, 1),
            change_options(present(args, 2)),
        )),
        "setAreaUnit" => reports(cap.set_area_unit(
            PageTarget::from_json(&arg(args, 0)),
            &str_arg(args, 1),
            change_options(present(args, 2)),
        )),
        "setPrecision" => reports(cap.set_precision(
            PageTarget::from_json(&arg(args, 0)),
            arg(args, 1).as_f64().unwrap_or(f64::NAN),
            change_options(present(args, 2)),
        )),
        "setPreset" => reports(cap.set_preset(
            PageTarget::from_json(&arg(args, 0)),
            &str_arg(args, 1),
            change_options(present(args, 2)),
        )),
        "clearScale" => reports(
            cap.clear_scale(PageTarget::from_json(&arg(args, 0)), change_options(present(args, 1))),
        ),
        "startCalibration" => {
            cap.start_calibration()?;
            Outcome::undefined()
        }
        "dismissCalibration" => {
            cap.dismiss_calibration();
            Outcome::undefined()
        }
        "createMeasurement" => Outcome::Promise(
            cap.create_measurement(create_input(&arg(args, 0))?, OperationOptions::default()),
        ),
        "canCalibrate" => Outcome::Returned(json!(cap.can_calibrate())),
        "ensureLoaded" => {
            Outcome::promise_unit(cap.ensure_loaded(page_arg(args, 0), OperationOptions::default()))
        }
        "onScaleChanged" => hook(h, args.first(), MeasurementScaleChangedEvent::to_json, |l| {
            cap.on_scale_changed(l)
        }),
        "onCalibrationRequested" => {
            hook(h, args.first(), CalibrationRequestedEvent::to_json, |l| {
                cap.on_calibration_requested(l)
            })
        }
        "onCalibrationCompleted" => {
            hook(h, args.first(), CalibrationCompletedEvent::to_json, |l| {
                cap.on_calibration_completed(l)
            })
        }
        "onCalibrationDismissed" => {
            hook(h, args.first(), CalibrationDismissedEvent::to_json, |l| {
                cap.on_calibration_dismissed(l)
            })
        }
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for MeasurementAdapter {
    fn factories(&self) -> Vec<&'static str> {
        vec!["measurement-current"]
    }

    fn build(&self, _factory: &str, config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        let registered = Some(config.clone()).filter(|c| !c.is_null() && !is_undefined(c));
        Ok(measurement_plugin_json(registered))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![MEASUREMENT.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<MeasurementCapability>()?;
        Some(call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<MeasurementCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn state_arg(args: &[Value], index: usize) -> Result<Rc<MeasurementState>, Error> {
    from_value(&arg(args, index)).map(Rc::new)
}

fn state_out(state: Rc<MeasurementState>) -> Result<Value, Error> {
    Ok(to_value(&*state))
}

fn number_arg(args: &[Value], index: usize) -> f64 {
    match arg(args, index) {
        v if is_undefined(&v) => f64::NAN,
        v => v.as_f64().unwrap_or(f64::NAN),
    }
}

fn pon_arg(args: &[Value], index: usize) -> Result<u32, Error> {
    let n = number_arg(args, index);
    if n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
        Ok(n as u32)
    } else {
        Err(Error::named("TypeError", "a page object number is not a whole number"))
    }
}

fn initial_measurement_state_fn(_: &[Value]) -> Result<Value, Error> {
    state_out(Rc::new(initial_measurement_state()))
}

fn begin_scale_change_fn(args: &[Value]) -> Result<Value, Error> {
    state_out(begin_scale_change(&state_arg(args, 0)?))
}

fn end_scale_change_fn(args: &[Value]) -> Result<Value, Error> {
    state_out(end_scale_change(&state_arg(args, 0)?))
}

fn set_calibration_fn(args: &[Value]) -> Result<Value, Error> {
    let request = present(args, 1).map(from_value).transpose()?;
    state_out(set_calibration(&state_arg(args, 0)?, request))
}

fn set_reports_fn(args: &[Value]) -> Result<Value, Error> {
    let reports: Vec<ScaleChangeReport> = from_value(&arg(args, 1))?;
    state_out(set_reports(&state_arg(args, 0)?, Rc::new(reports)))
}

fn set_local_viewports_fn(args: &[Value]) -> Result<Value, Error> {
    state_out(set_local_viewports(&state_arg(args, 0)?, pon_arg(args, 1)?, arg(args, 2)))
}

fn clear_load_error_fn(args: &[Value]) -> Result<Value, Error> {
    state_out(clear_load_error(&state_arg(args, 0)?, pon_arg(args, 1)?))
}

fn default_presets_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&default_presets()))
}

/// `defaultMeasure(defaultScale, userUnit = 1)`.
fn default_measure_fn(args: &[Value]) -> Result<Value, Error> {
    let user_unit = match present(args, 1) {
        None => 1.0,
        Some(_) => number_arg(args, 1),
    };
    default_measure(&arg(args, 0), user_unit)
}

fn size_of(value: &Value) -> Option<PageSize> {
    (!is_undefined(value) && !value.is_null()).then(|| PageSize::of(value))
}

fn select_page_scale_fn(args: &[Value]) -> Result<Value, Error> {
    let viewports = arg(args, 0);
    let list = viewports.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let size = size_of(&arg(args, 1)).unwrap_or(PageSize { width: f64::NAN, height: f64::NAN });
    let persistent = epdf_annotation_core::json::truthy(Some(&arg(args, 3)));
    Ok(select_page_scale(list, size, &arg(args, 2), persistent).to_json())
}

fn status_of(value: Option<&Value>) -> ResourceStatus {
    match value.and_then(Value::as_str) {
        Some("loading") => ResourceStatus::Loading,
        Some("ready") => ResourceStatus::Ready,
        Some("forbidden") => ResourceStatus::Forbidden,
        Some("error") => ResourceStatus::Error,
        _ => ResourceStatus::Idle,
    }
}

fn page_scale_of_fn(args: &[Value]) -> Result<Value, Error> {
    let inputs = arg(args, 0);
    let member = |key: &str| inputs.get(key).filter(|v| !is_undefined(v));
    let fallback = member("fallback").cloned().unwrap_or_else(undefined);
    Ok(page_scale_of(&PageScaleInputs {
        viewports: member("viewports"),
        status: status_of(member("status")),
        error: member("error"),
        size: member("size").and_then(size_of),
        fallback: &fallback,
        persistent: epdf_annotation_core::json::truthy(member("persistent")),
    })
    .to_json())
}

fn with_unit_fn(args: &[Value]) -> Result<Value, Error> {
    let area = present(args, 2).and_then(Value::as_str);
    with_unit(&arg(args, 0), &str_arg(args, 1), area)
}

fn with_area_unit_fn(args: &[Value]) -> Result<Value, Error> {
    with_area_unit(&arg(args, 0), &str_arg(args, 1))
}

fn with_precision_fn(args: &[Value]) -> Result<Value, Error> {
    with_precision(&arg(args, 0), number_arg(args, 1))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialMeasurementState", initial_measurement_state_fn),
        ("beginScaleChange", begin_scale_change_fn),
        ("endScaleChange", end_scale_change_fn),
        ("setCalibration", set_calibration_fn),
        ("setReports", set_reports_fn),
        ("setLocalViewports", set_local_viewports_fn),
        ("clearLoadError", clear_load_error_fn),
        ("DEFAULT_PRESETS", default_presets_fn),
        ("defaultMeasure", default_measure_fn),
        ("selectPageScale", select_page_scale_fn),
        ("pageScaleOf", page_scale_of_fn),
        ("withUnit", with_unit_fn),
        ("withAreaUnit", with_area_unit_fn),
        ("withPrecision", with_precision_fn),
    ]
}
