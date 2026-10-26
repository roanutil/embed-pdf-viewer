//! The `@embedpdf/engine-core/runtime` measurement helpers this plugin calls
//! and `epdf_annotation_core::engine::measure` does not carry: the unit
//! table (`measure/units.ts`), scale authoring on the default format
//! (`measure/calibrate.ts`, the only path this plugin takes),
//! `measurementPoint` (`measure/compute.ts`), `viewportForPoint`
//! (`measure/viewport.ts`) and `serializeError` (`errors/EngineError.ts`).
//! The write preflight, `assertWritableMeasure`, is annotation-core's.
//!
//! Measures are JSON, as the TypeScript reads an untyped object: `setScale`
//! hands the preflight whatever the caller passed, and a malformed member
//! has to fail with the TypeScript's message rather than a decode error.

use epdf_annotation_core::CoreError;
use epdf_annotation_core::engine::measure::fround;
use epdf_kernel::Error;
use epdf_kernel::geometry::Point;
use serde_json::{Map, Value, json};

/// `METRES`: metres per unit, in the TypeScript's key order.
pub const METRES: &[(&str, f64)] = &[
    ("pt", 0.0254 / 72.0),
    ("mm", 0.001),
    ("cm", 0.01),
    ("m", 1.0),
    ("km", 1000.0),
    ("in", 0.0254),
    ("ft", 0.3048),
    ("yd", 0.9144),
    ("mi", 1609.344),
];

/// `METRES[unit]`: `None` for a key the table lacks.
pub fn metres(unit: &str) -> Option<f64> {
    METRES.iter().find(|(u, _)| *u == unit).map(|(_, m)| *m)
}

/// `METRES[unit]` in arithmetic, where `undefined` becomes `NaN`.
pub(crate) fn metres_or_nan(unit: &str) -> f64 {
    metres(unit).unwrap_or(f64::NAN)
}

/// `squareOf(unit)`.
pub fn square_of(unit: &str) -> String {
    format!("{unit}2")
}

/// `squareMetres(unit)`.
pub fn square_metres(unit: &str) -> f64 {
    match unit {
        "ha" => 10000.0,
        "acre" => 4046.8564224,
        _ => {
            let mut base = unit.chars();
            base.next_back();
            metres_or_nan(base.as_str()).powi(2)
        }
    }
}

/// `areaUnitLabel(unit)`: `m2` reads `m²`.
pub fn area_unit_label(unit: &str) -> String {
    match unit.strip_suffix('2') {
        Some(base) => format!("{base}²"),
        None => unit.to_string(),
    }
}

/// `measurementPoint(p)`: both coordinates through `Math.fround`.
pub fn measurement_point(p: Point) -> Point {
    Point { x: fround(p.x), y: fround(p.y) }
}

/// A number as canonical JSON: `NaN` and the infinities have no JSON
/// spelling, so they travel as the conformance format's `$number`.
pub(crate) fn number(x: f64) -> Value {
    epdf_kernel::js::number(x)
}

/// `new EngineError(EngineErrorCode.InvalidArg, message)`.
fn invalid(message: &str) -> Error {
    Error::engine("InvalidArg", message)
}

/// What annotation-core threw, as the kernel's error.
pub fn core_error(error: CoreError) -> Error {
    epdf_plugin_annotation::sync::records::core_error(error)
}

/// `assertWritableMeasure(value)`: the preflight every scale write runs.
pub fn assert_writable_measure(value: &Value) -> Result<(), Error> {
    epdf_annotation_core::engine::measure::assert_writable_measure(value).map_err(core_error)
}

fn positive(value: f64) -> Result<(), Error> {
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid("Calibration requires finite positive lengths and scales"));
    }
    Ok(())
}

/// `Number(x.toPrecision(digits))`: `x` rounded to `digits` significant
/// digits and read back.
fn to_precision(x: f64, digits: usize) -> f64 {
    format!("{:.*e}", digits - 1, x).parse().unwrap_or(f64::NAN)
}

/// `build(perPoint, unit, ratio, {})`: the default format, a precision of
/// 100 and the unit's own square for areas.
fn build(per_point: f64, unit: &str, ratio: String) -> Result<Value, Error> {
    positive(fround(per_point))?;
    let precision = 100;
    let area_unit = square_of(unit);
    let measure = json!({
        "subtype": "rectilinear",
        "ratio": ratio,
        "x": [{ "unit": unit, "conversion": number(fround(per_point)), "precision": precision }],
        "distance": [{ "unit": unit, "conversion": 1, "precision": precision }],
        "area": [{
            "unit": area_unit_label(&area_unit),
            "conversion": number(fround(metres_or_nan(unit).powi(2) / square_metres(&area_unit))),
            "precision": precision,
        }],
    });
    assert_writable_measure(&measure)?;
    Ok(measure)
}

/// `measureFromKnownLength(userSpaceLength, { value, unit })`: the scale
/// that makes a length of `user_space_length` PDF units read `value`
/// `unit`s.
pub fn measure_from_known_length(
    user_space_length: f64,
    value: f64,
    unit: &str,
) -> Result<Value, Error> {
    positive(user_space_length)?;
    positive(value)?;
    let per_point = value / user_space_length;
    let shown = epdf_geometry::js::number_to_string(to_precision(per_point * 72.0, 8));
    build(per_point, unit, format!("1 in = {shown} {unit}"))
}

/// `measureFromRatio(paper, real, unit, { userUnit })`: `paper` on the page
/// is `real` `unit`s.
pub fn measure_from_ratio(
    paper: f64,
    real: f64,
    unit: &str,
    user_unit: f64,
) -> Result<Value, Error> {
    positive(paper)?;
    positive(real)?;
    positive(user_unit)?;
    let n = epdf_geometry::js::number_to_string;
    build(
        ((user_unit / 72.0) * 0.0254 * (real / paper)) / metres_or_nan(unit),
        unit,
        format!("{}:{}", n(paper), n(real)),
    )
}

/// A member as a number, `NaN` when it is missing or not one.
fn coordinate(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(epdf_annotation_core::json::number).unwrap_or(f64::NAN)
}

/// `viewportForPoint(viewports, point)`: the last viewport whose page-space
/// `bbox` holds the point (the one drawn on top), edges included.
pub fn viewport_for_point(viewports: &[Value], point: Point) -> Option<&Value> {
    viewports.iter().rev().find(|viewport| {
        let b = viewport.get("bbox").unwrap_or(&Value::Null);
        let (x, y) = (coordinate(b, "x"), coordinate(b, "y"));
        let (width, height) = (coordinate(b, "width"), coordinate(b, "height"));
        point.x >= x && point.x <= x + width && point.y >= y && point.y <= y + height
    })
}

/// `serializeError(error)`: an `EngineError` as its `toJSON` (`details`
/// only when it has some), anything else as `Unknown`, or `Aborted` for an
/// `AbortError`.
pub fn serialize_error(error: &Error) -> Value {
    let mut out = Map::new();
    out.insert("name".into(), json!("EngineError"));
    if error.name == "EngineError" {
        out.insert("code".into(), error.member("code").cloned().unwrap_or(Value::Null));
        out.insert("message".into(), json!(error.message));
        if let Some(details) = error.member("details").filter(|d| !epdf_kernel::is_undefined(d)) {
            out.insert("details".into(), details.clone());
        }
    } else {
        let code = if error.name == "AbortError" { "Aborted" } else { "Unknown" };
        out.insert("code".into(), json!(code));
        out.insert("message".into(), json!(error.message));
    }
    Value::Object(out)
}
