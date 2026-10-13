//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's one shim in
//! `tooling/plugin-conformance/src/suites/measurement.ts`: a `console.error`
//! whose message starts `[measurement]` logs as the callback
//! `console.error`, which comes in here as `MeasurementConfig::host`.

use std::any::Any;
use std::rc::Rc;

use epdf_annotation_core::measure::PdfMeasurement;
use epdf_annotation_core::{AnnotationRef, PageRef, PdfRect, Point};
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, is_undefined, undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::*;

/// The adapter.
pub struct MeasurementAdapter;

/// The capability object literal's member order: the areas' slices as
/// `composeApi` spreads them, then the service-owned members.
pub const MEMBERS: &[&str] = &[
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

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

fn arg<T: serde::de::DeserializeOwned>(args: &[Value], index: usize) -> Result<T, Error> {
    from_value(args.get(index).unwrap_or(&undefined()))
}

/// An optional options object.
fn opts<T: serde::de::DeserializeOwned + Default>(
    args: &[Value],
    index: usize,
) -> Result<T, Error> {
    present(args, index).map_or_else(|| Ok(T::default()), from_value)
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn reports(promise: Promise<Vec<ScaleChangeReport>>) -> Outcome {
    Outcome::promise(promise)
}

fn call_capability(
    cap: &MeasurementCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let returned = |value: Value| Outcome::Returned(value);
    Ok(match method {
        "canMeasure" => returned(json!(cap.can_measure(&arg(args, 0)?))),
        "getPageScale" => returned(to_value(&cap.get_page_scale(&arg(args, 0)?))),
        "isBusy" => returned(json!(cap.is_busy())),
        "listLastReports" => returned(to_value(&cap.list_last_reports())),
        "listPresets" => returned(to_value(&cap.list_presets())),
        "listUnits" => returned(to_value(&cap.list_units())),
        "listAreaUnits" => returned(to_value(&cap.list_area_units())),
        "getReadout" => returned(to_value(&cap.get_readout(&arg::<AnnotationRef>(args, 0)?))),
        "measureDistance" => {
            let page: PageRef = arg(args, 0)?;
            returned(to_value(&cap.measure_distance(&page, arg(args, 1)?, arg(args, 2)?)?))
        }
        "measureArea" => {
            let page: PageRef = arg(args, 0)?;
            let vertices: Vec<Point> = arg(args, 1)?;
            returned(to_value(&cap.measure_area(&page, &vertices)?))
        }
        "getCalibrationRequest" => returned(to_value(&cap.get_calibration_request())),
        "setScale" => reports(cap.set_scale(
            arg(args, 0)?,
            args.get(1).cloned().unwrap_or(Value::Null),
            opts(args, 2)?,
        )),
        "calibrate" => reports(cap.calibrate(arg(args, 0)?, opts(args, 1)?)),
        "setUnit" => {
            let unit: String = arg(args, 1)?;
            reports(cap.set_unit(arg(args, 0)?, &unit, opts(args, 2)?))
        }
        "setAreaUnit" => {
            let unit: String = arg(args, 1)?;
            reports(cap.set_area_unit(arg(args, 0)?, &unit, opts(args, 2)?))
        }
        "setPrecision" => reports(cap.set_precision(arg(args, 0)?, arg(args, 1)?, opts(args, 2)?)),
        "setPreset" => {
            let id: String = arg(args, 1)?;
            reports(cap.set_preset(arg(args, 0)?, &id, opts(args, 2)?))
        }
        "clearScale" => reports(cap.clear_scale(arg(args, 0)?, opts(args, 1)?)),
        "startCalibration" => {
            cap.start_calibration()?;
            Outcome::undefined()
        }
        "dismissCalibration" => {
            cap.dismiss_calibration();
            Outcome::undefined()
        }
        "createMeasurement" => Outcome::promise(cap.create_measurement(arg(args, 0)?)),
        "canCalibrate" => returned(json!(cap.can_calibrate())),
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(&arg(args, 0)?)?),
        "onScaleChanged" => hook(h, args.first(), |l| cap.on_scale_changed(l)),
        "onCalibrationRequested" => hook(h, args.first(), |l| cap.on_calibration_requested(l)),
        "onCalibrationCompleted" => hook(h, args.first(), |l| cap.on_calibration_completed(l)),
        "onCalibrationDismissed" => hook(h, args.first(), |l| cap.on_calibration_dismissed(l)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for MeasurementAdapter {
    fn factories(&self) -> Vec<&'static str> {
        vec!["measurement"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let mut config: MeasurementConfig = if config.is_object() && !is_undefined(config) {
            from_value(config)?
        } else {
            MeasurementConfig::default()
        };
        let log = h.clone();
        config.host = MeasurementHost {
            console_error: Some(Rc::new(move |message: &str, error: &Error| {
                log.log("console.error", vec![json!(message), error.to_canonical()]);
            })),
        };
        Ok(build_measurement_plugin(config))
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

fn ok<T: Serialize>(value: &T) -> Result<Value, Error> {
    Ok(to_value(value))
}

fn initial_measurement_state_fn(_: &[Value]) -> Result<Value, Error> {
    ok(&initial_measurement_state())
}

/// `{changed: next !== state, state: next}`.
fn measurement_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: MeasurementState = arg(a, 0)?;
    let action: MeasurementAction = arg(a, 1)?;
    let next = measurement_reducer(&state, &action);
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

fn default_presets_fn(_: &[Value]) -> Result<Value, Error> {
    ok(&default_presets())
}

fn default_measure_fn(a: &[Value]) -> Result<Value, Error> {
    let config: MeasurementConfig = arg(a, 0)?;
    // `userUnit = 1`: absence reads the default.
    let user_unit = present(a, 1).map_or(Ok(1.0), from_value::<f64>)?;
    ok(&default_measure(&config, user_unit)?)
}

fn select_page_scale_fn(a: &[Value]) -> Result<Value, Error> {
    let viewports: Vec<MeasurementViewport> = arg(a, 0)?;
    let crop: PdfRect = arg(a, 1)?;
    let fallback: PdfMeasurement = arg(a, 2)?;
    let persistent: bool = arg(a, 3)?;
    ok(&select_page_scale(&viewports, crop, &fallback, persistent))
}

fn with_unit_fn(a: &[Value]) -> Result<Value, Error> {
    let m: PdfMeasurement = arg(a, 0)?;
    let unit: String = arg(a, 1)?;
    let area: Option<String> = present(a, 2).map(from_value).transpose()?;
    let mut out = ok(&scale::with_unit(&m, &unit, area.as_deref())?)?;
    order_formats(&mut out, "distance", |_| NEW_FORMAT.to_vec());
    order_formats(&mut out, "area", |_| NEW_FORMAT.to_vec());
    Ok(out)
}

fn with_area_unit_fn(a: &[Value]) -> Result<Value, Error> {
    let m: PdfMeasurement = arg(a, 0)?;
    let unit: String = arg(a, 1)?;
    let mut out = ok(&scale::with_area_unit(&m, &unit)?)?;
    order_formats(&mut out, "area", |_| NEW_FORMAT.to_vec());
    Ok(out)
}

fn with_precision_fn(a: &[Value]) -> Result<Value, Error> {
    let m: PdfMeasurement = arg(a, 0)?;
    let mut out = ok(&scale::with_precision(&m, arg(a, 1)?)?)?;
    // `{ ...f, precision, fraction }` on the last format: the input's order,
    // with either key it lacked added at the end.
    for key in ["distance", "area"] {
        let input: Vec<Vec<String>> = a
            .first()
            .and_then(|m| m.get(key))
            .and_then(Value::as_array)
            .map(|formats| {
                formats
                    .iter()
                    .map(|f| f.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        order_formats(&mut out, key, |i| {
            let mut keys: Vec<&str> =
                input.get(i).map(|k| k.iter().map(String::as_str).collect()).unwrap_or_default();
            if i + 1 == input.len() {
                for added in ["precision", "fraction"] {
                    if !keys.contains(&added) {
                        keys.push(added);
                    }
                }
            }
            keys
        });
    }
    Ok(out)
}

/// The member order of a number format `withUnit` and `withAreaUnit` build:
/// `{ unit, conversion, precision, fraction }`. A calibrated or read
/// `PdfNumberFormat` keeps the DTO's `fraction` before `precision`.
const NEW_FORMAT: [&str; 4] = ["unit", "conversion", "precision", "fraction"];

/// Reorder each format in `out[key]` to `order(index)`; members it doesn't
/// name keep their place after the ones it does.
fn order_formats<'a>(out: &mut Value, key: &str, order: impl Fn(usize) -> Vec<&'a str>) {
    let Some(Value::Array(formats)) = out.get_mut(key) else { return };
    for (i, format) in formats.iter_mut().enumerate() {
        let Value::Object(members) = format else { continue };
        let mut ordered = serde_json::Map::new();
        for name in order(i) {
            if let Some(value) = members.remove(name) {
                ordered.insert(name.to_string(), value);
            }
        }
        ordered.append(members);
        *members = ordered;
    }
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialMeasurementState", initial_measurement_state_fn),
        ("measurementReducer", measurement_reducer_fn),
        ("defaultPresets", default_presets_fn),
        ("defaultMeasure", default_measure_fn),
        ("selectPageScale", select_page_scale_fn),
        ("withUnit", with_unit_fn),
        ("withAreaUnit", with_area_unit_fn),
        ("withPrecision", with_precision_fn),
    ]
}
