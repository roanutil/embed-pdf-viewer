//! `scale.ts`: the default presets and scale, and unit and precision edits
//! of a scale.

use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::contract::ScalePreset;
use crate::units::{area_unit_label, measure_from_ratio, metres, number, square_metres, square_of};

/// `DEFAULT_PRESETS`: metric 1:1 to 1:200, and two imperial ones.
pub fn default_presets() -> Vec<ScalePreset> {
    let metric = [1.0, 10.0, 20.0, 50.0, 100.0, 200.0].into_iter().map(|real: f64| {
        let n = epdf_geometry::js::number_to_string(real);
        ScalePreset {
            id: format!("metric-{n}"),
            label: format!("1:{n}"),
            paper: 1.0,
            real,
            unit: "m".into(),
        }
    });
    let imperial =
        [("imperial-quarter", "1/4 in = 1 ft", 48.0), ("imperial-eighth", "1/8 in = 1 ft", 96.0)]
            .into_iter()
            .map(|(id, label, real)| ScalePreset {
                id: id.into(),
                label: label.into(),
                paper: 1.0,
                real,
                unit: "ft".into(),
            });
    metric.chain(imperial).collect()
}

/// `defaultMeasure(defaultScale, userUnit = 1)`: the scale of a page that
/// has none. An object (`typeof === 'object'`, `null` included) is the
/// scale as it is; `'imperial'` is 1:1 in feet, anything else 1:1 in
/// metres, in the page's user unit.
pub fn default_measure(default_scale: &Value, user_unit: f64) -> Result<Value, Error> {
    match default_scale {
        Value::Object(_) | Value::Array(_) | Value::Null => Ok(default_scale.clone()),
        Value::String(s) if s == "imperial" => measure_from_ratio(1.0, 1.0, "ft", user_unit),
        _ => measure_from_ratio(1.0, 1.0, "m", user_unit),
    }
}

fn range_error(message: &str) -> Error {
    Error::named("RangeError", message)
}

fn type_error(message: String) -> Error {
    Error::named("TypeError", message)
}

/// `measure.x[0]?.unit.trim()` checked against `METRES`. The TypeScript's
/// `basis in METRES` also admits `Object.prototype` keys such as
/// `constructor`; the port looks the unit up in the table.
fn basis_metres(measure: &Value) -> Result<f64, Error> {
    let unit = match measure.get("x").and_then(|x| x.get(0)) {
        None | Some(Value::Null) => None,
        Some(first) => match first.get("unit") {
            Some(Value::String(unit)) => Some(unit.trim().to_string()),
            None | Some(Value::Null) => {
                return Err(type_error(
                    "Cannot read properties of undefined (reading 'trim')".into(),
                ));
            }
            Some(_) => {
                return Err(type_error("measure.x[0]?.unit.trim is not a function".into()));
            }
        },
    };
    unit.as_deref()
        .and_then(metres)
        .ok_or_else(|| range_error("This scale has an unknown base unit; calibrate it first"))
}

/// The last format's precision a unit edit keeps: 100 after a fractional
/// format, else its own precision, else 100.
fn kept_precision(formats: Option<&Value>) -> Value {
    let previous = formats.and_then(Value::as_array).and_then(|list| list.last());
    let Some(previous) = previous else { return json!(100) };
    if previous.get("fraction").and_then(Value::as_str) == Some("fraction") {
        return json!(100);
    }
    match previous.get("precision") {
        None | Some(Value::Null) => json!(100),
        Some(precision) if epdf_kernel::is_undefined(precision) => json!(100),
        Some(precision) => precision.clone(),
    }
}

/// `{ ...measure, [key]: value, ... }`: a member the measure already has
/// keeps its place.
fn spread(measure: &Value, members: Vec<(&str, Value)>) -> Value {
    let mut out = measure.as_object().cloned().unwrap_or_default();
    for (key, value) in members {
        out.insert(key.into(), value);
    }
    Value::Object(out)
}

/// `withUnit(measure, unit, area = squareOf(unit))`: show distances in
/// `unit` and areas in `area`, converted from the scale's base unit.
pub fn with_unit(measure: &Value, unit: &str, area: Option<&str>) -> Result<Value, Error> {
    let area = area.map_or_else(|| square_of(unit), str::to_string);
    let basis = basis_metres(measure)?;
    let precision = kept_precision(measure.get("distance"));
    let target = metres(unit).unwrap_or(f64::NAN);
    Ok(spread(
        measure,
        vec![
            (
                "distance",
                json!([{
                    "unit": unit,
                    "conversion": number(basis / target),
                    "precision": precision,
                    "fraction": "decimal",
                }]),
            ),
            (
                "area",
                json!([{
                    "unit": area_unit_label(&area),
                    "conversion": number(basis.powi(2) / square_metres(&area)),
                    "precision": precision,
                    "fraction": "decimal",
                }]),
            ),
        ],
    ))
}

/// `withPrecision(measure, precision)`: the last distance and area formats
/// show `precision` (a power of ten from 1 to 1e9) as decimals.
pub fn with_precision(measure: &Value, precision: f64) -> Result<Value, Error> {
    let log = precision.log10();
    if !(log.is_finite() && log.trunc() == log) || precision < 1.0 || precision > 1e9 {
        return Err(range_error("Invalid decimal precision"));
    }
    let update = |key: &str| -> Result<Value, Error> {
        let Some(Value::Array(formats)) = measure.get(key) else {
            return Err(type_error(format!("formats.map is not a function ({key})")));
        };
        let last = formats.len().saturating_sub(1);
        Ok(Value::Array(
            formats
                .iter()
                .enumerate()
                .map(|(index, format)| {
                    if index != last {
                        return format.clone();
                    }
                    let mut next: Map<String, Value> =
                        format.as_object().cloned().unwrap_or_default();
                    next.insert("precision".into(), number(precision));
                    next.insert("fraction".into(), json!("decimal"));
                    Value::Object(next)
                })
                .collect(),
        ))
    };
    let distance = update("distance")?;
    let area = update("area")?;
    Ok(spread(measure, vec![("distance", distance), ("area", area)]))
}

/// `withAreaUnit(measure, unit)`: change the area display unit, keeping the
/// imported distance formatting.
pub fn with_area_unit(measure: &Value, unit: &str) -> Result<Value, Error> {
    let basis = basis_metres(measure)?;
    let precision = kept_precision(measure.get("area"));
    Ok(spread(
        measure,
        vec![(
            "area",
            json!([{
                "unit": area_unit_label(unit),
                "conversion": number(basis.powi(2) / square_metres(unit)),
                "precision": precision,
                "fraction": "decimal",
            }]),
        )],
    ))
}
