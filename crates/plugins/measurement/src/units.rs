//! The `@embedpdf/engine-core/runtime` measurement helpers this plugin
//! calls and `epdf_annotation_core::measure` does not carry: the unit table
//! (`measure/units.ts`), scale authoring (`measure/calibrate.ts`, the
//! default-format path only, which is the one this plugin takes), the write
//! preflight (`measure/validate.ts`), `measurementPoint` and
//! `viewportForPoint`.
//!
//! The preflight reads the measure as JSON, as the TypeScript reads an
//! untyped object: `setScale` hands it whatever the caller passed, and a
//! malformed member has to fail with the TypeScript's message rather than a
//! decode error.

use epdf_annotation_core::measure::PdfMeasurement;
use epdf_annotation_core::{PdfPoint, PdfRect};
use epdf_kernel::Error;
use serde_json::Value;

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

/// `METRES[unit]` in arithmetic: `undefined` becomes `NaN`.
fn metres_or_nan(unit: &str) -> f64 {
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

/// `Math.fround`.
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

/// `measurementPoint(p)`: both coordinates through `Math.fround`.
pub fn measurement_point(p: PdfPoint) -> PdfPoint {
    PdfPoint { x: fround(p.x), y: fround(p.y) }
}

/// A `RangeError`, the class every helper here throws.
pub fn range_error(message: &str) -> Error {
    Error::named("RangeError", message)
}

fn positive(value: f64) -> Result<(), Error> {
    if !value.is_finite() || value <= 0.0 {
        return Err(range_error("Calibration requires finite positive lengths and scales"));
    }
    Ok(())
}

/// `Number(x.toPrecision(digits))`: `x` rounded to `digits` significant
/// digits and read back.
fn to_precision(x: f64, digits: usize) -> f64 {
    format!("{:.*e}", digits - 1, x).parse().unwrap_or(f64::NAN)
}

/// `build(perPoint, unit, ratio, { precision: 100 })`.
fn build(per_point: f64, unit: &str, ratio: String) -> Result<Value, Error> {
    positive(fround(per_point))?;
    let area_unit = square_of(unit);
    let measure = serde_json::json!({
        "subtype": "RL",
        "ratio": ratio,
        "x": [{ "unit": unit, "conversion": fround(per_point), "precision": 100 }],
        "distance": [{ "unit": unit, "conversion": 1, "precision": 100 }],
        "area": [{
            "unit": area_unit_label(&area_unit),
            "conversion": number(fround(metres_or_nan(unit).powi(2) / square_metres(&area_unit))),
            "precision": 100,
        }],
    });
    assert_writable_measure(&measure)?;
    Ok(measure)
}

/// A number as JSON; `NaN` has no JSON spelling, so it travels as the
/// conformance format's `{"$number": "NaN"}`, which the preflight rejects
/// like the `NaN` it is.
fn number(x: f64) -> Value {
    epdf_kernel::js::to_value(&x)
}

/// `measureFromKnownLength(userSpaceLength, real)`: the scale that makes a
/// length of `user_space_length` PDF units read `value` `unit`s.
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

/// `measureFromRatio(paper, real, unit, userUnit)`: `paper` on the page is
/// `real` `unit`s.
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

/// `measureFromRatio` read back as the typed measure.
pub fn measure_from_ratio_typed(
    paper: f64,
    real: f64,
    unit: &str,
    user_unit: f64,
) -> Result<PdfMeasurement, Error> {
    let value = measure_from_ratio(paper, real, unit, user_unit)?;
    decode_measure(&value)
}

/// A validated measure as the typed DTO.
pub fn decode_measure(value: &Value) -> Result<PdfMeasurement, Error> {
    serde_json::from_value(value.clone())
        .map_err(|e| Error::new(format!("[measurement] malformed measure: {e}")))
}

/// The JavaScript `typeof x === 'number'` read of a member: `undefined` for
/// absence, `NaN` for anything that is not a number.
fn num_member(nf: &Value, key: &str) -> Option<f64> {
    match nf.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().unwrap_or(f64::NAN)),
    }
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n
}

/// `validNumberFormat(nf)` over JSON.
fn valid_number_format(nf: &Value) -> bool {
    let c = fround(num_member(nf, "conversion").unwrap_or(f64::NAN));
    if !c.is_finite() || c <= 0.0 {
        return false;
    }
    let fraction = nf.get("fraction").filter(|f| !f.is_null());
    let d = num_member(nf, "precision")
        .unwrap_or(if fraction.and_then(Value::as_str) == Some("fraction") { 16.0 } else { 100.0 });
    if !is_integer(d) || !(1.0..=2147483647.0).contains(&d) {
        return false;
    }
    let decimal = fraction.is_none_or(|f| f.as_str() == Some("decimal"));
    !(decimal && !is_integer(d.log10()))
}

fn text_ok(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(Value::String(s)) => !s.contains('\0'),
        Some(_) => false,
    }
}

/// `assertPdfFloat(n)`.
fn assert_pdf_float(n: Option<&Value>) -> Result<(), Error> {
    match n.and_then(Value::as_f64) {
        Some(n) if fround(n).is_finite() => Ok(()),
        _ => Err(range_error("Value is outside PDF float range")),
    }
}

/// `assertWritableMeasure(value)`: the preflight every scale write runs.
pub fn assert_writable_measure(value: &Value) -> Result<(), Error> {
    if value.get("subtype").and_then(Value::as_str) != Some("RL") {
        return Err(range_error("Only rectilinear measures can be authored"));
    }
    if let Some(ratio) = value.get("ratio")
        && !matches!(ratio, Value::String(s) if !s.contains('\0'))
    {
        return Err(range_error("Invalid scale ratio"));
    }
    for key in ["x", "y", "distance", "area", "angle", "slope"] {
        let list = value.get(key);
        if list.is_none() && matches!(key, "y" | "angle" | "slope") {
            continue;
        }
        let Some(Value::Array(list)) = list else {
            return Err(range_error(&format!("Missing {key} formats")));
        };
        for nf in list {
            let usable = nf.is_object()
                && nf.get("unit").is_some_and(Value::is_string)
                && valid_number_format(nf);
            if !usable {
                return Err(range_error("Invalid number format"));
            }
            if let Some(fraction) = nf.get("fraction")
                && !matches!(fraction.as_str(), Some("decimal" | "fraction" | "round" | "truncate"))
            {
                return Err(range_error("Invalid fraction mode"));
            }
            if let Some(position) = nf.get("labelPosition")
                && !matches!(position.as_str(), Some("prefix" | "suffix"))
            {
                return Err(range_error("Invalid label position"));
            }
            if let Some(fixed) = nf.get("fixed")
                && !fixed.is_boolean()
            {
                return Err(range_error("Invalid fixed precision"));
            }
            for text in ["unit", "thousands", "decimal", "prefixSpacing", "suffixSpacing"] {
                if !text_ok(nf.get(text)) {
                    return Err(range_error("Invalid measurement text"));
                }
            }
        }
    }
    if let Some(origin) = value.get("origin") {
        assert_pdf_float(origin.get("x"))?;
        assert_pdf_float(origin.get("y"))?;
    }
    if let Some(cyx) = value.get("cyx") {
        assert_pdf_float(Some(cyx))?;
        if fround(cyx.as_f64().unwrap_or(f64::NAN)) <= 0.0 {
            return Err(range_error("Invalid CYX"));
        }
    }
    Ok(())
}

/// `viewportForPoint(viewports, point)`: the last viewport containing the
/// point.
pub fn viewport_for_point<T>(
    viewports: &[T],
    bbox: impl Fn(&T) -> PdfRect,
    point: PdfPoint,
) -> Option<&T> {
    viewports.iter().rev().find(|v| {
        let b = bbox(v);
        point.x >= b.left && point.x <= b.right && point.y >= b.bottom && point.y <= b.top
    })
}
