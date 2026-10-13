//! The `@embedpdf/engine-core/runtime` measurement the model reads: the
//! `/Measure` DTOs (`dto/Measure.ts`) and the readout that turns a dimension
//! annotation's geometry into its label (`measure/derive.ts`,
//! `measure/compute.ts`, `measure/format.ts`).
//!
//! Only what `measurement.ts` and `measurement-shape.ts` call is ported:
//! `measurementReadout` and what it needs. Calibration and the write-side
//! validation stay with the engine.

use serde::{Deserialize, Serialize};

use crate::js;
use crate::types::PdfPoint;

/// `MeasureFraction`: the last component's numeric representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasureFraction {
    Decimal,
    Fraction,
    Round,
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelPosition {
    Suffix,
    Prefix,
}

/// `PdfNumberFormat`: one ISO number format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfNumberFormat {
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<MeasureFraction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thousands: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_spacing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix_spacing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_position: Option<LabelPosition>,
}

/// `PdfMeasurement = PdfMeasure | PdfForeignMeasure`. A rectilinear measure
/// has `subtype: 'RL'` and its format lists; a foreign one carries only its
/// subtype. The lists are optional here so one struct reads both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMeasurement {
    pub subtype: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<PdfPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cyx: Option<f64>,
}

impl PdfMeasurement {
    fn list(list: &Option<Vec<PdfNumberFormat>>) -> &[PdfNumberFormat] {
        list.as_deref().unwrap_or(&[])
    }
}

/// `MeasurementReadout['kind']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadoutKind {
    Distance,
    Perimeter,
    Area,
}

/// `MeasurementReadout`: the value in the first format's unit and its label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeasurementReadout {
    pub kind: ReadoutKind,
    pub value: f64,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perimeter: Option<String>,
}

/// `MeasurementUnavailable['unavailable']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unavailable {
    NotDimension,
    NoMeasure,
    ForeignMeasure,
    NoScale,
    NoDistanceFormat,
    NoAreaFormat,
    InvalidFormat,
    InvalidGeometry,
}

/// `MeasurementReadout | MeasurementUnavailable`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Readout {
    Readout(MeasurementReadout),
    Unavailable { unavailable: Unavailable },
}

impl Readout {
    /// `isReadout(r) ? r : undefined`.
    pub fn readout(&self) -> Option<&MeasurementReadout> {
        match self {
            Readout::Readout(r) => Some(r),
            Readout::Unavailable { .. } => None,
        }
    }

    pub fn unavailable(&self) -> Option<Unavailable> {
        match self {
            Readout::Readout(_) => None,
            Readout::Unavailable { unavailable } => Some(*unavailable),
        }
    }
}

fn unavailable(reason: Unavailable) -> Readout {
    Readout::Unavailable { unavailable: reason }
}

/// `Math.fround`.
fn fround(x: f64) -> f64 {
    x as f32 as f64
}

fn positive(n: f64) -> bool {
    n.is_finite() && n > 0.0
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n
}

// ── compute.ts ──────────────────────────────────────────────────────────────

/// `MeasuredScale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredScale {
    pub sx: f64,
    pub sy: f64,
}

/// `scaleOf`: the user-space-to-unit scale per axis, or `None`.
pub fn scale_of(measure: &PdfMeasurement) -> Option<MeasuredScale> {
    let first = |list: &Option<Vec<PdfNumberFormat>>| {
        PdfMeasurement::list(list).first().and_then(|f| f.conversion).unwrap_or(f64::NAN)
    };
    let sx = fround(first(&measure.x));
    if !positive(sx) {
        return None;
    }
    if measure.y.is_none() {
        return Some(MeasuredScale { sx, sy: sx });
    }
    let cy = fround(first(&measure.y));
    let cyx = fround(measure.cyx.unwrap_or(f64::NAN));
    let sy = cy * cyx;
    (positive(cy) && positive(cyx) && positive(sy)).then_some(MeasuredScale { sx, sy })
}

fn measurement_point(p: &PdfPoint) -> PdfPoint {
    PdfPoint { x: fround(p.x), y: fround(p.y) }
}

/// `pathLength`.
pub fn path_length(points: &[PdfPoint], closed: bool, scale: MeasuredScale) -> f64 {
    let normalized: Vec<PdfPoint> = points.iter().map(measurement_point).collect();
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
pub fn polygon_area(points: &[PdfPoint], scale: MeasuredScale) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let normalized: Vec<PdfPoint> = points.iter().map(measurement_point).collect();
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
pub fn valid_area_boundary(points: &[PdfPoint]) -> bool {
    let mut boundary: Vec<PdfPoint> = points.iter().map(measurement_point).collect();
    let same = |a: &PdfPoint, b: &PdfPoint| a.x == b.x && a.y == b.y;
    if boundary.len() > 1 && same(&boundary[0], &boundary[boundary.len() - 1]) {
        boundary.pop();
    }
    if boundary.len() < 3 || polygon_area(&boundary, MeasuredScale { sx: 1.0, sy: 1.0 }) == 0.0 {
        return false;
    }
    let cross = |a: &PdfPoint, b: &PdfPoint, c: &PdfPoint| {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    };
    let on_segment = |a: &PdfPoint, b: &PdfPoint, p: &PdfPoint| {
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

/// `value.replace(/\B(?=(\d{3})+(?!\d))/g, () => separator)`: a separator
/// before every run of digits whose length is a multiple of three, where the
/// position is not a word boundary.
fn group(value: &str, separator: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && is_word(chars[i - 1]) == is_word(*c) {
            let run = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            if run > 0 && run % 3 == 0 {
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
pub fn valid_number_format(nf: &PdfNumberFormat) -> bool {
    let c = fround(nf.conversion.unwrap_or(f64::NAN));
    if !c.is_finite() || c <= 0.0 {
        return false;
    }
    let d = nf.precision.unwrap_or(if nf.fraction == Some(MeasureFraction::Fraction) {
        16.0
    } else {
        100.0
    });
    if !is_integer(d) || d < 1.0 || d > 2147483647.0 {
        return false;
    }
    let mode = nf.fraction.unwrap_or(MeasureFraction::Decimal);
    !(mode == MeasureFraction::Decimal && !is_integer(d.log10()))
}

fn last(value: f64, nf: &PdfNumberFormat) -> String {
    let n = js::number_to_string;
    let sep = nf.thousands.as_deref().unwrap_or(",");
    match nf.fraction.unwrap_or(MeasureFraction::Decimal) {
        MeasureFraction::Round => group(&n(js::round(value)), sep),
        MeasureFraction::Truncate => group(&n(value.trunc()), sep),
        MeasureFraction::Fraction => {
            let denominator = nf.precision.unwrap_or(16.0);
            let mut whole = value.floor();
            let mut numerator = js::round((value - whole) * denominator);
            if numerator == denominator {
                whole += 1.0;
                numerator = 0.0;
            }
            if numerator == 0.0 || numerator.is_nan() {
                return group(&n(whole), sep);
            }
            let divisor = if nf.fixed == Some(true) { 1.0 } else { gcd(numerator, denominator) };
            let lead = if whole != 0.0 && !whole.is_nan() {
                format!("{} ", group(&n(whole), sep))
            } else {
                String::new()
            };
            format!("{lead}{}/{}", n(numerator / divisor), n(denominator / divisor))
        }
        MeasureFraction::Decimal => {
            let digits = nf.precision.unwrap_or(100.0).log10();
            let fixed = js::to_fixed(value, digits as u32);
            let (integer, decimal) = fixed.split_once('.').unwrap_or((&fixed, ""));
            let kept = if nf.fixed == Some(true) { decimal } else { decimal.trim_end_matches('0') };
            let point = match nf.decimal.as_deref() {
                Some(d) if !d.is_empty() => d,
                _ => ".",
            };
            let tail = if kept.is_empty() { String::new() } else { format!("{point}{kept}") };
            group(integer, sep) + &tail
        }
    }
}

/// `formatMeasurement`: the ISO 32000-2 cascade, `None` where the
/// TypeScript throws a `RangeError`.
pub fn format_measurement(value: f64, formats: &[PdfNumberFormat]) -> Option<String> {
    if !value.is_finite() || formats.is_empty() || !formats.iter().all(valid_number_format) {
        return None;
    }
    let mut remainder = value.abs();
    let mut parts = String::new();
    for (index, nf) in formats.iter().enumerate() {
        remainder *= fround(nf.conversion.unwrap_or(f64::NAN));
        if !remainder.is_finite() {
            return None;
        }
        let whole = remainder.floor();
        let fraction = remainder - whole;
        let terminal = index == formats.len() - 1 || fraction == 0.0;
        let number = if terminal {
            last(remainder, nf)
        } else {
            group(&js::number_to_string(whole), nf.thousands.as_deref().unwrap_or(","))
        };
        let prefix = nf.label_position == Some(LabelPosition::Prefix);
        let ps = nf
            .prefix_spacing
            .clone()
            .unwrap_or_else(|| if prefix && index == 0 { String::new() } else { " ".into() });
        let ss = nf
            .suffix_spacing
            .clone()
            .unwrap_or_else(|| if !prefix && terminal { String::new() } else { " ".into() });
        let label = format!("{ps}{}{ss}", nf.unit);
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
    Some(if value < 0.0 { format!("-{parts}") } else { parts })
}

// ── derive.ts ───────────────────────────────────────────────────────────────

/// The dimension annotation a readout measures: `DimensionInput` with its
/// points already in PDF user space.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionInput<'a> {
    pub subtype: &'a str,
    pub intent: &'a str,
    pub measure: Option<&'a PdfMeasurement>,
    /// A line's start and end.
    pub line_points: Option<(PdfPoint, PdfPoint)>,
    pub vertices: Vec<PdfPoint>,
}

/// `isDimension`.
pub fn is_dimension(subtype: &str, intent: &str) -> bool {
    (subtype == "line" && intent == "LineDimension")
        || (subtype == "polyline" && intent == "PolyLineDimension")
        || (subtype == "polygon" && intent == "PolygonDimension")
}

/// `measurementReadout`.
pub fn measurement_readout(dto: &DimensionInput) -> Readout {
    if !is_dimension(dto.subtype, dto.intent) {
        return unavailable(Unavailable::NotDimension);
    }
    let Some(m) = dto.measure else {
        return unavailable(Unavailable::NoMeasure);
    };
    if m.subtype != "RL" {
        return unavailable(Unavailable::ForeignMeasure);
    }
    let Some(scale) = scale_of(m) else {
        return unavailable(Unavailable::NoScale);
    };
    let points: Vec<PdfPoint> = if dto.subtype == "line" {
        dto.line_points.map_or_else(Vec::new, |(a, b)| vec![a, b])
    } else {
        dto.vertices.clone()
    };
    let min = if dto.subtype == "polygon" { 3 } else { 2 };
    if points.len() < min
        || points.iter().any(|p| !fround(p.x).is_finite() || !fround(p.y).is_finite())
    {
        return unavailable(Unavailable::InvalidGeometry);
    }
    let area = dto.subtype == "polygon";
    if area && !valid_area_boundary(&points) {
        return unavailable(Unavailable::InvalidGeometry);
    }
    let formats = PdfMeasurement::list(if area { &m.area } else { &m.distance });
    if formats.is_empty() {
        return unavailable(if area {
            Unavailable::NoAreaFormat
        } else {
            Unavailable::NoDistanceFormat
        });
    }
    if !formats.iter().all(valid_number_format) {
        return unavailable(Unavailable::InvalidFormat);
    }
    let value =
        if area { polygon_area(&points, scale) } else { path_length(&points, false, scale) };
    let Some(label) = format_measurement(value, formats) else {
        return unavailable(Unavailable::InvalidFormat);
    };
    let distance = PdfMeasurement::list(&m.distance);
    let perimeter = if area && !distance.is_empty() && distance.iter().all(valid_number_format) {
        match format_measurement(path_length(&points, true, scale), distance) {
            Some(p) => Some(p),
            None => return unavailable(Unavailable::InvalidFormat),
        }
    } else {
        None
    };
    Readout::Readout(MeasurementReadout {
        kind: if area {
            ReadoutKind::Area
        } else if dto.subtype == "line" {
            ReadoutKind::Distance
        } else {
            ReadoutKind::Perimeter
        },
        value: value * fround(formats[0].conversion.unwrap_or(f64::NAN)),
        label,
        perimeter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_matches_the_regex() {
        assert_eq!(group("1234567", ","), "1,234,567");
        assert_eq!(group("123", ","), "123");
        assert_eq!(group("1e+21", ","), "1e+21");
        assert_eq!(group("12345.5", " "), "12 345.5");
    }
}
