//! The engine's measurement labels (`packages/engine/core/src/measure/`):
//! the scale a `/Measure` dictionary sets (`compute.ts`), the ISO 32000-2
//! number-format cascade (`format.ts`), the readout a dimension's points
//! give (`derive.ts`) and the checks a written scale passes (`validate.ts`).
//!
//! Measures are read straight off the annotation's JSON. A missing number
//! reads as `NaN`, as in the TypeScript. Other malformed members don't throw
//! where the TypeScript does: a format list entry that isn't an object is
//! skipped, a missing `x` or `area` list reads as empty (no scale, or `NaN`),
//! a missing `unit` prints as `""` where the TypeScript prints
//! `"undefined"`, and a `fraction` that isn't a string reads as `decimal`.
//! The README lists these as kept differences.

use epdf_geometry::{Point, js};
use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::json::{self, Object};

/// `Math.fround`.
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

fn positive(n: f64) -> bool {
    n.is_finite() && n > 0.0
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n
}

/// One `PdfNumberFormat`, read off its JSON.
#[derive(Clone, Copy, Debug)]
pub struct NumberFormat<'a>(pub &'a Object);

impl<'a> NumberFormat<'a> {
    fn num(&self, name: &str) -> Option<f64> {
        json::get(self.0, name).and_then(json::number)
    }
    fn text(&self, name: &str) -> Option<&'a str> {
        json::get(self.0, name).and_then(Value::as_str)
    }
    /// `nf.conversion` (`undefined` as `None`).
    pub fn conversion(&self) -> Option<f64> {
        self.num("conversion")
    }
    fn fraction(&self) -> &'a str {
        self.text("fraction").unwrap_or("decimal")
    }
    fn precision(&self) -> Option<f64> {
        self.num("precision")
    }
    fn unit(&self) -> &'a str {
        self.text("unit").unwrap_or("")
    }
    fn thousands(&self) -> &'a str {
        self.text("thousands").unwrap_or(",")
    }
}

/// A measure's format list (`measure.x`, `.distance`…), empty when absent.
fn formats<'a>(measure: &'a Object, name: &str) -> Vec<NumberFormat<'a>> {
    match measure.get(name).and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(Value::as_object).map(NumberFormat).collect(),
        None => Vec::new(),
    }
}

/// `MeasuredScale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredScale {
    pub sx: f64,
    pub sy: f64,
}

/// `scaleOf(measure)`.
pub fn scale_of(measure: &Object) -> Option<MeasuredScale> {
    let first = |name: &str| {
        formats(measure, name).first().and_then(|f| f.conversion()).unwrap_or(f64::NAN)
    };
    let sx = fround(first("x"));
    if !positive(sx) {
        return None;
    }
    if json::nullish(measure.get("y")) {
        return Some(MeasuredScale { sx, sy: sx });
    }
    let cy = fround(first("y"));
    let cyx = fround(json::get_num(measure, "cyx").unwrap_or(f64::NAN));
    let sy = cy * cyx;
    (positive(cy) && positive(cyx) && positive(sy)).then_some(MeasuredScale { sx, sy })
}

fn measurement_point(p: &Point) -> Point {
    Point { x: fround(p.x), y: fround(p.y) }
}

/// `pathLength`.
pub fn path_length(points: &[Point], closed: bool, scale: MeasuredScale) -> f64 {
    let normalized: Vec<Point> = points.iter().map(measurement_point).collect();
    let n = normalized.len();
    let count = if closed { n } else { n.saturating_sub(1) };
    let mut total = 0.0;
    for i in 0..count {
        let a = normalized[i];
        let b = normalized[(i + 1) % n];
        total += js::hypot((b.x - a.x) * scale.sx, (b.y - a.y) * scale.sy);
    }
    total
}

/// `polygonArea`.
pub fn polygon_area(points: &[Point], scale: MeasuredScale) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let normalized: Vec<Point> = points.iter().map(measurement_point).collect();
    let origin = normalized[0];
    let mut twice = 0.0;
    for i in 1..normalized.len() - 1 {
        let a = normalized[i];
        let b = normalized[i + 1];
        twice += (a.x - origin.x) * (b.y - origin.y) - (b.x - origin.x) * (a.y - origin.y);
    }
    (twice * scale.sx * scale.sy).abs() / 2.0
}

/// `validAreaBoundary`: one simple boundary of nonzero area.
pub fn valid_area_boundary(points: &[Point]) -> bool {
    let mut boundary: Vec<Point> = points.iter().map(measurement_point).collect();
    let same = |a: &Point, b: &Point| a.x == b.x && a.y == b.y;
    if boundary.len() > 1 && same(&boundary[0], &boundary[boundary.len() - 1]) {
        boundary.pop();
    }
    if boundary.len() < 3 || polygon_area(&boundary, MeasuredScale { sx: 1.0, sy: 1.0 }) == 0.0 {
        return false;
    }
    let cross =
        |a: &Point, b: &Point, c: &Point| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let on_segment = |a: &Point, b: &Point, p: &Point| {
        cross(a, b, p) == 0.0
            && p.x >= js::min(a.x, b.x)
            && p.x <= js::max(a.x, b.x)
            && p.y >= js::min(a.y, b.y)
            && p.y <= js::max(a.y, b.y)
    };
    let n = boundary.len();
    for i in 0..n {
        let a = &boundary[i];
        let b = &boundary[(i + 1) % n];
        if same(a, b) {
            return false;
        }
        let next = &boundary[(i + 2) % n];
        let doubles_back = (b.x - a.x) * (next.x - b.x) + (b.y - a.y) * (next.y - b.y) < 0.0;
        if cross(a, b, next) == 0.0 && doubles_back {
            return false;
        }
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let c = &boundary[j];
            let d = &boundary[(j + 1) % n];
            if on_segment(a, b, c)
                || on_segment(a, b, d)
                || on_segment(c, d, a)
                || on_segment(c, d, b)
            {
                return false;
            }
            if js::sign(cross(a, b, c)) != js::sign(cross(a, b, d))
                && js::sign(cross(c, d, a)) != js::sign(cross(c, d, b))
            {
                return false;
            }
        }
    }
    true
}

// ── format.ts ───────────────────────────────────────────────────────────────

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `value.replace(/\B(?=(\d{3})+(?!\d))/g, () => separator)`.
fn group(value: &str, separator: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && is_word(chars[i - 1]) == is_word(*c) {
            let run = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            if run > 0 && run % 3 == 0 && chars.get(i + run).is_none_or(|c| !c.is_ascii_digit()) {
                out.push_str(separator);
            }
        }
        out.push(*c);
    }
    out
}

fn gcd(a: f64, b: f64) -> f64 {
    if b != 0.0 && !b.is_nan() { gcd(b, a % b) } else { a }
}

/// `validNumberFormat`.
pub fn valid_number_format(nf: &NumberFormat) -> bool {
    let c = fround(nf.conversion().unwrap_or(f64::NAN));
    if !c.is_finite() || c <= 0.0 {
        return false;
    }
    let d = nf.precision().unwrap_or(if nf.fraction() == "fraction" { 16.0 } else { 100.0 });
    if !is_integer(d) || !(1.0..=2147483647.0).contains(&d) {
        return false;
    }
    !(nf.fraction() == "decimal" && !is_integer(d.log10()))
}

fn last(value: f64, nf: &NumberFormat) -> String {
    let n = js::number_to_string;
    let sep = nf.thousands();
    match nf.fraction() {
        "round" => group(&n(js::round(value)), sep),
        "truncate" => group(&n(value.trunc()), sep),
        "fraction" => {
            let denominator = nf.precision().unwrap_or(16.0);
            let mut whole = value.floor();
            let mut numerator = js::round((value - whole) * denominator);
            if numerator == denominator {
                whole += 1.0;
                numerator = 0.0;
            }
            if numerator == 0.0 || numerator.is_nan() {
                return group(&n(whole), sep);
            }
            let fixed = json::truthy(nf.0.get("fixed"));
            let divisor = if fixed { 1.0 } else { gcd(numerator, denominator) };
            let lead = if whole != 0.0 && !whole.is_nan() {
                format!("{} ", group(&n(whole), sep))
            } else {
                String::new()
            };
            format!("{lead}{}/{}", n(numerator / divisor), n(denominator / divisor))
        }
        _ => {
            // Every digit /RD asks for, /FD or not, as Acrobat shows it.
            let digits = nf.precision().unwrap_or(100.0).log10();
            let fixed = js::to_fixed(value, digits as u32);
            let (integer, decimal) = fixed.split_once('.').unwrap_or((&fixed, ""));
            let point = match nf.text("decimal") {
                Some(d) if !d.is_empty() => d,
                _ => ".",
            };
            let tail = if decimal.is_empty() { String::new() } else { format!("{point}{decimal}") };
            group(integer, sep) + &tail
        }
    }
}

/// `formatMeasurement(value, formats)`: the ISO 32000-2 §12.9.2 cascade.
pub fn format_measurement(value: f64, formats: &[NumberFormat]) -> CoreResult<String> {
    if !value.is_finite() || formats.is_empty() || !formats.iter().all(valid_number_format) {
        return Err(CoreError::invalid(
            "Measurement requires a finite value and valid number formats",
        ));
    }
    let mut remainder = value.abs();
    let mut parts = String::new();
    for (index, nf) in formats.iter().enumerate() {
        remainder *= fround(nf.conversion().unwrap_or(f64::NAN));
        if !remainder.is_finite() {
            return Err(CoreError::invalid("Measurement overflow"));
        }
        let whole = remainder.floor();
        let fraction = remainder - whole;
        let is_final = index == formats.len() - 1;
        let terminal = is_final || fraction == 0.0;
        let number = if is_final {
            last(remainder, nf)
        } else {
            group(&js::number_to_string(whole), nf.thousands())
        };
        let prefix = nf.text("labelPosition") == Some("prefix");
        let ps = match nf.0.get("prefixSpacing").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => {
                if prefix && index == 0 {
                    String::new()
                } else {
                    " ".into()
                }
            }
        };
        let ss = match nf.0.get("suffixSpacing").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => {
                if !prefix && terminal {
                    String::new()
                } else {
                    " ".into()
                }
            }
        };
        let label = format!("{ps}{}{ss}", nf.unit());
        if prefix {
            parts.push_str(&label);
            parts.push_str(&number);
        } else {
            parts.push_str(&number);
            parts.push_str(&label);
        }
        if terminal {
            break;
        }
        remainder = fraction;
    }
    Ok(if value < 0.0 { format!("-{parts}") } else { parts })
}

// ── derive.ts ───────────────────────────────────────────────────────────────

/// `MeasurementReadout` | `MeasurementUnavailable`.
#[derive(Clone, Debug, PartialEq)]
pub enum Readout {
    Readout { kind: &'static str, value: f64, label: String, perimeter: Option<String> },
    Unavailable(&'static str),
}

impl Readout {
    /// The label, for a readout.
    pub fn label(&self) -> Option<&str> {
        match self {
            Readout::Readout { label, .. } => Some(label),
            Readout::Unavailable(_) => None,
        }
    }

    pub fn unavailable(&self) -> Option<&'static str> {
        match self {
            Readout::Unavailable(reason) => Some(reason),
            Readout::Readout { .. } => None,
        }
    }

    /// The readout as the TypeScript object prints it.
    pub fn to_value(&self) -> Value {
        match self {
            Readout::Readout { kind, value, label, perimeter } => {
                let mut out = json::object([
                    ("kind", Value::String((*kind).into())),
                    ("value", json::num(*value)),
                    ("label", Value::String(label.clone())),
                ]);
                if let Some(p) = perimeter {
                    out.insert("perimeter".into(), Value::String(p.clone()));
                }
                Value::Object(out)
            }
            Readout::Unavailable(reason) => {
                Value::Object(json::object([("unavailable", Value::String((*reason).into()))]))
            }
        }
    }
}

/// `isDimension({ subtype, intent })`.
pub fn is_dimension(subtype: &str, intent: Option<&str>) -> bool {
    matches!(
        (subtype, intent),
        ("line", Some("line-dimension"))
            | ("polyline", Some("polyline-dimension"))
            | ("polygon", Some("polygon-dimension"))
    )
}

/// `DimensionInput`: the fields a readout reads.
#[derive(Clone, Debug, Default)]
pub struct DimensionInput<'a> {
    pub subtype: &'a str,
    pub intent: Option<&'a str>,
    pub measure: Option<&'a Value>,
    /// A line's two ends.
    pub line_points: Option<(Point, Point)>,
    pub vertices: Vec<Point>,
}

impl<'a> DimensionInput<'a> {
    /// The input an annotation (or a draft, or the two merged) gives.
    pub fn of(object: &'a Object) -> Self {
        let line_points = json::get(object, "linePoints").map(|lp| {
            (
                lp.get("start")
                    .map(json::point_loose)
                    .unwrap_or(Point { x: f64::NAN, y: f64::NAN }),
                lp.get("end").map(json::point_loose).unwrap_or(Point { x: f64::NAN, y: f64::NAN }),
            )
        });
        let vertices = json::get(object, "vertices")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(json::point_loose).collect())
            .unwrap_or_default();
        DimensionInput {
            subtype: json::get_str(object, "subtype").unwrap_or(""),
            intent: json::get_str(object, "intent"),
            measure: json::get(object, "measure"),
            line_points,
            vertices,
        }
    }
}

/// `measurementReadout(dto)`.
pub fn measurement_readout(dto: &DimensionInput) -> Readout {
    if !is_dimension(dto.subtype, dto.intent) {
        return Readout::Unavailable("not-dimension");
    }
    let Some(m) = dto.measure.and_then(Value::as_object) else {
        return Readout::Unavailable("no-measure");
    };
    if json::get_str(m, "subtype") != Some("rectilinear") {
        return Readout::Unavailable("foreign-measure");
    }
    let Some(scale) = scale_of(m) else { return Readout::Unavailable("no-scale") };
    let points: Vec<Point> = if dto.subtype == "line" {
        dto.line_points.map_or_else(Vec::new, |(a, b)| vec![a, b])
    } else {
        dto.vertices.clone()
    };
    let min = if dto.subtype == "polygon" { 3 } else { 2 };
    if points.len() < min
        || points.iter().any(|p| !fround(p.x).is_finite() || !fround(p.y).is_finite())
    {
        return Readout::Unavailable("invalid-geometry");
    }
    let area = dto.subtype == "polygon";
    if area && !valid_area_boundary(&points) {
        return Readout::Unavailable("invalid-geometry");
    }
    let list = formats(m, if area { "area" } else { "distance" });
    if list.is_empty() {
        return Readout::Unavailable(if area { "no-area-format" } else { "no-distance-format" });
    }
    if !list.iter().all(valid_number_format) {
        return Readout::Unavailable("invalid-format");
    }
    let value =
        if area { polygon_area(&points, scale) } else { path_length(&points, false, scale) };
    let Ok(label) = format_measurement(value, &list) else {
        return Readout::Unavailable("invalid-format");
    };
    let distance = formats(m, "distance");
    let perimeter = if area && !distance.is_empty() && distance.iter().all(valid_number_format) {
        match format_measurement(path_length(&points, true, scale), &distance) {
            Ok(p) => Some(p),
            Err(_) => return Readout::Unavailable("invalid-format"),
        }
    } else {
        None
    };
    Readout::Readout {
        kind: if area {
            "area"
        } else if dto.subtype == "line" {
            "distance"
        } else {
            "perimeter"
        },
        value: value * fround(list[0].conversion().unwrap_or(f64::NAN)),
        label,
        perimeter,
    }
}

// ── validate.ts ─────────────────────────────────────────────────────────────

/// `assertPdfFloat(n)`.
pub fn assert_pdf_float(value: Option<&Value>) -> CoreResult<()> {
    match value.and_then(json::number) {
        Some(n) if fround(n).is_finite() => Ok(()),
        _ => Err(CoreError::invalid("Value is outside PDF float range")),
    }
}

/// `assertWritableMeasure(value)`.
pub fn assert_writable_measure(value: &Value) -> CoreResult<()> {
    let invalid = |m: &str| Err(CoreError::invalid(m));
    let Some(measure) = value.as_object() else {
        return invalid("Only rectilinear measures can be authored");
    };
    if json::get_str(measure, "subtype") != Some("rectilinear") {
        return invalid("Only rectilinear measures can be authored");
    }
    if let Some(ratio) = measure.get("ratio").filter(|v| !json::undefined(Some(v)))
        && !ratio.as_str().is_some_and(|r| !r.contains('\0'))
    {
        return invalid("Invalid scale ratio");
    }
    for key in ["x", "y", "distance", "area", "angle", "slope"] {
        let list = measure.get(key).filter(|v| !json::undefined(Some(v)));
        if list.is_none() && matches!(key, "y" | "angle" | "slope") {
            continue;
        }
        let Some(list) = list.and_then(Value::as_array) else {
            return invalid(&format!("Missing {key} formats"));
        };
        for nf in list {
            let Some(object) = nf.as_object() else { return invalid("Invalid number format") };
            let format = NumberFormat(object);
            if !object.get("unit").is_some_and(Value::is_string) || !valid_number_format(&format) {
                return invalid("Invalid number format");
            }
            let defined = |name: &str| object.get(name).filter(|v| !json::undefined(Some(v)));
            if let Some(f) = defined("fraction")
                && !matches!(f.as_str(), Some("decimal" | "fraction" | "round" | "truncate"))
            {
                return invalid("Invalid fraction mode");
            }
            if let Some(p) = defined("labelPosition")
                && !matches!(p.as_str(), Some("prefix" | "suffix"))
            {
                return invalid("Invalid label position");
            }
            if let Some(f) = defined("fixed")
                && !f.is_boolean()
            {
                return invalid("Invalid fixed precision");
            }
            for name in ["unit", "thousands", "decimal", "prefixSpacing", "suffixSpacing"] {
                if let Some(text) = defined(name)
                    && !text.as_str().is_some_and(|t| !t.contains('\0'))
                {
                    return invalid("Invalid measurement text");
                }
            }
        }
    }
    if let Some(origin) = measure.get("origin").filter(|v| !json::undefined(Some(v))) {
        assert_pdf_float(origin.get("x"))?;
        assert_pdf_float(origin.get("y"))?;
    }
    if let Some(cyx) = measure.get("cyx").filter(|v| !json::undefined(Some(v))) {
        assert_pdf_float(Some(cyx))?;
        if json::number(cyx).is_some_and(|c| fround(c) <= 0.0) {
            return invalid("Invalid CYX");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_matches_the_regex() {
        assert_eq!(group("1234567", ","), "1,234,567");
        assert_eq!(group("123", ","), "123");
        assert_eq!(group("1e+21", ","), "1e+21");
        assert_eq!(group("12345", " "), "12 345");
    }
}
