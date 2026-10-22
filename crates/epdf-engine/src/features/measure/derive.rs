//! A dimension annotation's label, worked out from its points and its
//! scale: `packages/engine/core/src/measure/compute.ts`, `format.ts` and
//! `derive.ts`, function for function.
//!
//! The arithmetic is the TypeScript's: every coordinate and conversion
//! factor goes through `Math.fround` first, `Math.hypot` and `Math.round`
//! are V8's, and numbers print as `String(n)` and `n.toFixed(d)` print them,
//! so a label written here is the label a viewer predicts.

use serde::Deserialize;
use serde_json::Value;

use crate::geom::PdfPoint;

/// `PdfNumberFormat`, as the readout reads it. Every member is optional:
/// an imported format can lack any of them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberFormat {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub conversion: Option<f64>,
    #[serde(default)]
    pub fraction: Option<String>,
    #[serde(default)]
    pub precision: Option<f64>,
    #[serde(default)]
    pub fixed: Option<bool>,
    #[serde(default)]
    pub thousands: Option<String>,
    #[serde(default)]
    pub decimal: Option<String>,
    #[serde(default)]
    pub prefix_spacing: Option<String>,
    #[serde(default)]
    pub suffix_spacing: Option<String>,
    #[serde(default)]
    pub label_position: Option<String>,
}

/// A rectilinear `PdfMeasure`, as the readout reads it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Measure {
    #[serde(default)]
    pub x: Vec<NumberFormat>,
    #[serde(default)]
    pub y: Option<Vec<NumberFormat>>,
    #[serde(default)]
    pub distance: Vec<NumberFormat>,
    #[serde(default)]
    pub area: Vec<NumberFormat>,
    #[serde(default)]
    pub cyx: Option<f64>,
}

// ── JavaScript's numbers ────────────────────────────────────────────────────

/// `Math.fround`.
pub fn fround(n: f64) -> f64 {
    f64::from(n as f32)
}

/// `Math.round`: ties toward +infinity.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.min(a, b)` and `Math.max(a, b)`: NaN wins.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `Math.sign`: `±0` and NaN map to themselves.
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.hypot(a, b)` as V8 computes it: the values normalized by the
/// largest and summed with Kahan compensation, an infinity winning over NaN.
pub(crate) fn js_hypot(a: f64, b: f64) -> f64 {
    let values = [a, b];
    let mut max = 0.0_f64;
    let mut one_is_nan = false;
    for v in values {
        if v.is_nan() {
            one_is_nan = true;
        } else if v.abs() > max {
            max = v.abs();
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if one_is_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `String(n)`: the shortest round-trip digits, plain from 1e-7 to 1e21
/// and scientific outside it.
fn js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x < 0.0 {
        return format!("-{}", js_string(-x));
    }
    if x.is_infinite() {
        return "Infinity".into();
    }
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        format!("{int}.{frac}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", e.abs())
        } else {
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    }
}

/// Whether `x * 10^digits` lies exactly halfway between two integers, for
/// a positive finite `x`.
fn exact_tie(x: f64, digits: u32) -> bool {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (m, e) =
        if exponent == 0 { (fraction, -1074) } else { (fraction | (1u64 << 52), exponent - 1075) };
    let Some(t) = 5u128.checked_pow(digits).and_then(|p| u128::from(m).checked_mul(p)) else {
        return false;
    };
    let shift = e + 1 + digits as i32;
    match shift {
        s if s > 0 => false,
        0 => t % 2 == 1,
        s => t != 0 && t.trailing_zeros() == (-s) as u32,
    }
}

/// `n.toFixed(digits)`: the exact value rounded half up on the magnitude.
/// Rust's `{:.N}` breaks an exact tie to even, so a tie is written one digit
/// longer and rounded up by hand.
fn js_to_fixed(x: f64, digits: u32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 {
        return js_string(x);
    }
    let negative = x < 0.0;
    let y = x.abs();
    let precision = digits as usize;
    let body = if y != 0.0 && exact_tie(y, digits) {
        let exact = format!("{:.*}", precision + 1, y);
        let mut kept: Vec<u8> = exact[..exact.len() - 1].bytes().collect();
        if kept.last() == Some(&b'.') {
            kept.pop();
        }
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            match kept[i] {
                b'.' => continue,
                b'9' => kept[i] = b'0',
                d => {
                    kept[i] = d + 1;
                    break;
                }
            }
        }
        String::from_utf8(kept).unwrap_or_default()
    } else {
        format!("{:.*}", precision, y)
    };
    if negative { format!("-{body}") } else { body }
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n
}

// ── compute.ts ──────────────────────────────────────────────────────────────

/// `MeasuredScale`: user space to the first format's unit, per axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredScale {
    pub sx: f64,
    pub sy: f64,
}

fn positive(n: f64) -> bool {
    n.is_finite() && n > 0.0
}

/// `scaleOf`.
pub fn scale_of(measure: &Measure) -> Option<MeasuredScale> {
    let first = |list: &[NumberFormat]| list.first().and_then(|f| f.conversion).unwrap_or(f64::NAN);
    let sx = fround(first(&measure.x));
    if !positive(sx) {
        return None;
    }
    let Some(y) = &measure.y else { return Some(MeasuredScale { sx, sy: sx }) };
    let cy = fround(first(y));
    let cyx = fround(measure.cyx.unwrap_or(f64::NAN));
    let sy = cy * cyx;
    (positive(cy) && positive(cyx) && positive(sy)).then_some(MeasuredScale { sx, sy })
}

/// `measurementPoint`.
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
        total += js_hypot((b.x - a.x) * scale.sx, (b.y - a.y) * scale.sy);
    }
    total
}

/// `polygonArea`: the shoelace over coordinates relative to the first
/// point, which avoids cancellation at a large page origin.
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

/// `validAreaBoundary`: one simple boundary of nonzero area. A repeated
/// closing vertex is allowed; crossings and overlapping edges are not.
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
            && p.x >= js_min(a.x, b.x)
            && p.x <= js_max(a.x, b.x)
            && p.y >= js_min(a.y, b.y)
            && p.y <= js_max(a.y, b.y)
    };
    let n = boundary.len();
    for i in 0..n {
        let a = &boundary[i];
        let b = &boundary[(i + 1) % n];
        if same(a, b) {
            return false;
        }
        let next = &boundary[(i + 2) % n];
        // Adjacent collinear edges may continue forward, never double back.
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
            if js_sign(cross(a, b, c)) != js_sign(cross(a, b, d))
                && js_sign(cross(c, d, a)) != js_sign(cross(c, d, b))
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
/// before every run of digits whose length is a multiple of three, where
/// the position is not a word boundary.
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

/// `gcd`: `b ? gcd(b, a % b) : a`, NaN being falsy.
fn gcd(a: f64, b: f64) -> f64 {
    if b != 0.0 && !b.is_nan() { gcd(b, a % b) } else { a }
}

/// `validNumberFormat`.
pub fn valid_number_format(nf: &NumberFormat) -> bool {
    let c = fround(nf.conversion.unwrap_or(f64::NAN));
    if !c.is_finite() || c <= 0.0 {
        return false;
    }
    let d = nf.precision.unwrap_or(if nf.fraction.as_deref() == Some("fraction") {
        16.0
    } else {
        100.0
    });
    if !is_integer(d) || !(1.0..=2147483647.0).contains(&d) {
        return false;
    }
    let decimal = nf.fraction.as_deref().unwrap_or("decimal") == "decimal";
    !(decimal && !is_integer(d.log10()))
}

/// `last`: the cascade's last component.
fn last(value: f64, nf: &NumberFormat) -> String {
    let sep = nf.thousands.as_deref().unwrap_or(",");
    match nf.fraction.as_deref().unwrap_or("decimal") {
        "round" => group(&js_string(js_round(value)), sep),
        "truncate" => group(&js_string(value.trunc()), sep),
        "fraction" => {
            let denominator = nf.precision.unwrap_or(16.0);
            let mut whole = value.floor();
            let mut numerator = js_round((value - whole) * denominator);
            if numerator == denominator {
                whole += 1.0;
                numerator = 0.0;
            }
            if numerator == 0.0 || numerator.is_nan() {
                return group(&js_string(whole), sep);
            }
            let divisor = if nf.fixed == Some(true) { 1.0 } else { gcd(numerator, denominator) };
            let lead = if whole != 0.0 && !whole.is_nan() {
                format!("{} ", group(&js_string(whole), sep))
            } else {
                String::new()
            };
            format!("{lead}{}/{}", js_string(numerator / divisor), js_string(denominator / divisor))
        }
        // Every digit /RD asks for, /FD or not, as Acrobat shows it.
        _ => {
            let digits = nf.precision.unwrap_or(100.0).log10();
            let fixed = js_to_fixed(value, digits as u32);
            let (integer, decimal) = fixed.split_once('.').unwrap_or((&fixed, ""));
            let point = match nf.decimal.as_deref() {
                Some(d) if !d.is_empty() => d,
                _ => ".",
            };
            let tail = if decimal.is_empty() { String::new() } else { format!("{point}{decimal}") };
            group(integer, sep) + &tail
        }
    }
}

/// `formatMeasurement`: the ISO 32000-2 §12.9.2 cascade. `None` where the
/// TypeScript throws (a value that isn't finite, no formats, an invalid
/// format, an overflow).
pub fn format_measurement(value: f64, formats: &[NumberFormat]) -> Option<String> {
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
        let last_format = index == formats.len() - 1;
        let terminal = last_format || fraction == 0.0;
        // A cascade that stops early on a whole unit shows it whole.
        let number = if last_format {
            last(remainder, nf)
        } else {
            group(&js_string(whole), nf.thousands.as_deref().unwrap_or(","))
        };
        let prefix = nf.label_position.as_deref() == Some("prefix");
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

/// `DimensionInput`: what a readout measures, in the file's coordinates.
#[derive(Debug, Clone, Default)]
pub struct DimensionInput<'a> {
    pub subtype: &'a str,
    pub intent: Option<&'a str>,
    pub measure: Option<&'a Value>,
    pub line_points: Option<(PdfPoint, PdfPoint)>,
    pub vertices: Option<Vec<PdfPoint>>,
}

/// `MeasurementReadout`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementReadout {
    /// `distance`, `perimeter` or `area`.
    pub kind: &'static str,
    pub label: String,
    /// In the first display format's unit.
    pub value: f64,
    pub perimeter: Option<String>,
}

/// `MeasurementReadout | MeasurementUnavailable`: the readout, or why there
/// is none (`not-dimension`, `no-measure`, `foreign-measure`, `no-scale`,
/// `no-distance-format`, `no-area-format`, `invalid-format`,
/// `invalid-geometry`).
pub type Readout = Result<MeasurementReadout, &'static str>;

/// `isDimension`.
pub fn is_dimension(subtype: &str, intent: Option<&str>) -> bool {
    matches!(
        (subtype, intent),
        ("line", Some("line-dimension"))
            | ("polyline", Some("polyline-dimension"))
            | ("polygon", Some("polygon-dimension"))
    )
}

/// `measurementReadout`.
pub fn measurement_readout(dto: &DimensionInput<'_>) -> Readout {
    if !is_dimension(dto.subtype, dto.intent) {
        return Err("not-dimension");
    }
    let Some(measure) = dto.measure.filter(|m| !m.is_null()) else { return Err("no-measure") };
    if measure.get("subtype").and_then(Value::as_str) != Some("rectilinear") {
        return Err("foreign-measure");
    }
    let Ok(m) = serde_json::from_value::<Measure>(measure.clone()) else {
        return Err("invalid-format");
    };
    let Some(scale) = scale_of(&m) else { return Err("no-scale") };
    let points: Vec<PdfPoint> = if dto.subtype == "line" {
        dto.line_points.map_or_else(Vec::new, |(a, b)| vec![a, b])
    } else {
        dto.vertices.clone().unwrap_or_default()
    };
    let area = dto.subtype == "polygon";
    if points.len() < if area { 3 } else { 2 }
        || points.iter().any(|p| !fround(p.x).is_finite() || !fround(p.y).is_finite())
    {
        return Err("invalid-geometry");
    }
    if area && !valid_area_boundary(&points) {
        return Err("invalid-geometry");
    }
    let formats = if area { &m.area } else { &m.distance };
    if formats.is_empty() {
        return Err(if area { "no-area-format" } else { "no-distance-format" });
    }
    if !formats.iter().all(valid_number_format) {
        return Err("invalid-format");
    }
    let value =
        if area { polygon_area(&points, scale) } else { path_length(&points, false, scale) };
    let label = format_measurement(value, formats).ok_or("invalid-format")?;
    let perimeter = if area && !m.distance.is_empty() && m.distance.iter().all(valid_number_format)
    {
        Some(
            format_measurement(path_length(&points, true, scale), &m.distance)
                .ok_or("invalid-format")?,
        )
    } else {
        None
    };
    Ok(MeasurementReadout {
        kind: if area {
            "area"
        } else if dto.subtype == "line" {
            "distance"
        } else {
            "perimeter"
        },
        value: value * fround(formats[0].conversion.unwrap_or(f64::NAN)),
        label,
        perimeter,
    })
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/measure.test.ts`, the cases over the
    //! arithmetic and formatting this module ports.
    use super::*;
    use serde_json::json;

    fn nf(value: Value) -> NumberFormat {
        serde_json::from_value(value).unwrap()
    }

    fn format(value: f64, formats: &[Value]) -> String {
        let formats: Vec<NumberFormat> = formats.iter().cloned().map(nf).collect();
        format_measurement(value, &formats).unwrap()
    }

    fn p(x: f64, y: f64) -> PdfPoint {
        PdfPoint { x, y }
    }

    /// `measureFromKnownLength(userSpaceLength, { value, unit: 'm' })` with
    /// the default precision: its x, distance and area formats.
    fn known_length(user_space_length: f64, value: f64) -> Value {
        json!({
            "subtype": "rectilinear",
            "x": [{ "unit": "m", "conversion": fround(value / user_space_length), "precision": 100 }],
            "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }],
            "area": [{ "unit": "m²", "conversion": 1, "precision": 100 }],
        })
    }

    #[test]
    fn iso_multi_unit_example_and_exact_stop() {
        let formats = [
            json!({ "unit": "mi", "conversion": 1 }),
            json!({ "unit": "ft", "conversion": 5280 }),
            json!({ "unit": "in", "conversion": 12, "fraction": "fraction", "precision": 8 }),
        ];
        assert_eq!(format(1.4505, &formats), "1 mi 2,378 ft 7 5/8 in");
        assert_eq!(
            format(2.0, &[json!({ "unit": "ft", "conversion": 1 }), formats[2].clone()]),
            "2 ft"
        );
    }

    #[test]
    fn fractions_reduce_fixed_denominators_stay_fixed_and_rounding_carries() {
        let inch =
            json!({ "unit": "in", "conversion": 1, "fraction": "fraction", "precision": 16 });
        assert_eq!(format(1.5, std::slice::from_ref(&inch)), "1 1/2 in");
        let mut fixed = inch.clone();
        fixed["fixed"] = json!(true);
        assert_eq!(format(1.5, &[fixed]), "1 8/16 in");
        assert_eq!(format(1.999, &[inch]), "2 in");
    }

    #[test]
    fn decimal_empty_means_dot_grouping_and_literal_spacing_stay_distinct() {
        assert_eq!(
            format(
                1234.5,
                &[json!({ "unit": "m ", "conversion": 1, "precision": 100, "fixed": true,
                    "thousands": "", "decimal": "", "suffixSpacing": "" })]
            ),
            "1234.50 m "
        );
        assert_eq!(
            format(
                1234.5,
                &[json!({ "unit": "$", "conversion": 1, "labelPosition": "prefix",
                    "prefixSpacing": "", "suffixSpacing": "", "fixed": true })]
            ),
            "$1,234.50"
        );
        assert_eq!(
            format(-1234.5, &[json!({ "unit": "m", "conversion": 1, "thousands": "$" })]),
            "-1$234.50 m"
        );
    }

    #[test]
    fn decimals_show_every_digit_rd_asks_for_with_or_without_fd() {
        let metric = json!({ "unit": "m ", "conversion": 1, "precision": 100 });
        assert_eq!(format(1.5, std::slice::from_ref(&metric)), "1.50 m ");
        assert_eq!(format(2.0, std::slice::from_ref(&metric)), "2.00 m ");
        let mut one = metric;
        one["precision"] = json!(1);
        assert_eq!(format(2.0, &[one]), "2 m ");
    }

    #[test]
    fn round_and_truncate_modes() {
        assert_eq!(
            format(1.8, &[json!({ "unit": "m", "conversion": 1, "fraction": "round" })]),
            "2 m"
        );
        assert_eq!(
            format(1.8, &[json!({ "unit": "m", "conversion": 1, "fraction": "truncate" })]),
            "1 m"
        );
    }

    #[test]
    fn coordinates_and_scale_factors_are_normalized_to_pdf_float32() {
        let m = known_length(1.0, 1.00000001);
        let at = |x: f64| DimensionInput {
            subtype: "line",
            intent: Some("line-dimension"),
            measure: Some(&m),
            line_points: Some((p(0.0, 0.0), p(x, 0.0))),
            vertices: None,
        };
        let a = measurement_readout(&at(3.4450000001));
        let b = measurement_readout(&at(fround(3.4450000001)));
        assert_eq!(a, b);
        assert_eq!(a.unwrap().label, "3.44 m");
    }

    #[test]
    fn anisotropic_scale_requires_cyx_and_a_missing_conversion_is_unavailable() {
        let m = json!({
            "subtype": "rectilinear",
            "x": [{ "unit": "m", "conversion": 1 }],
            "y": [{ "unit": "m", "conversion": 2 }],
            "distance": [{ "unit": "m", "conversion": 1 }],
            "area": [],
        });
        let read = |measure: &Value| {
            measurement_readout(&DimensionInput {
                subtype: "line",
                intent: Some("line-dimension"),
                measure: Some(measure),
                line_points: Some((p(0.0, 0.0), p(3.0, 2.0))),
                vertices: None,
            })
        };
        assert_eq!(read(&m), Err("no-scale"));
        let mut with_cyx = m.clone();
        with_cyx["cyx"] = json!(1);
        let r = read(&with_cyx).unwrap();
        assert_eq!((r.value, r.label.as_str()), (5.0, "5.00 m"));
        let mut no_conversion = m.clone();
        no_conversion["x"] = json!([{ "unit": "m" }]);
        assert_eq!(read(&no_conversion), Err("no-scale"));
        let mut unwritable = m;
        unwritable["distance"] = json!([{ "unit": "m" }]);
        assert!(super::super::assert_writable_measure(&unwritable).is_err());
    }

    #[test]
    fn area_uses_stable_shoelace_at_shifted_origins_and_the_value_is_in_display_units() {
        let points = [
            p(1e6, 1e6),
            p(1e6 + 4.0, 1e6),
            p(1e6 + 4.0, 1e6 + 4.0),
            p(1e6 + 2.0, 1e6 + 2.0),
            p(1e6, 1e6 + 4.0),
        ];
        assert_eq!(polygon_area(&points, MeasuredScale { sx: 2.0, sy: 3.0 }), 72.0);
        // `measureFromKnownLength(1, {value: 1, unit: 'm'}, {areaUnit: 'ha'})`.
        let measure = json!({
            "subtype": "rectilinear",
            "x": [{ "unit": "m", "conversion": 1, "precision": 100 }],
            "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }],
            "area": [{ "unit": "ha", "conversion": fround(1.0 / 10000.0), "precision": 100 }],
        });
        let r = measurement_readout(&DimensionInput {
            subtype: "polygon",
            intent: Some("polygon-dimension"),
            measure: Some(&measure),
            line_points: None,
            vertices: Some(points.to_vec()),
        })
        .unwrap();
        assert!((r.value - 0.0012).abs() < 0.005, "{}", r.value);
    }

    fn area_of(points: &[[f64; 2]]) -> Readout {
        let measure = known_length(1.0, 1.0);
        measurement_readout(&DimensionInput {
            subtype: "polygon",
            intent: Some("polygon-dimension"),
            measure: Some(&measure),
            line_points: None,
            vertices: Some(points.iter().map(|[x, y]| p(*x, *y)).collect()),
        })
    }

    #[test]
    fn rejects_crossing_touching_overlapping_and_degenerate_boundaries() {
        for points in [
            vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]],
            vec![[0.0, 0.0], [10.0, 10.0], [20.0, 20.0]],
            vec![[0.0, 0.0], [10.0, 0.0], [5.0, 0.0], [10.0, 10.0]],
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [5.0, 0.0], [10.0, 10.0]],
        ] {
            assert_eq!(area_of(&points), Err("invalid-geometry"), "{points:?}");
        }
    }

    #[test]
    fn accepts_either_winding_collinear_forward_edges_a_closing_vertex_and_a_large_origin() {
        let points =
            vec![[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]];
        let reversed: Vec<[f64; 2]> = points.iter().rev().copied().collect();
        let shifted: Vec<[f64; 2]> =
            points.iter().map(|[x, y]| [x + 10000.0, y - 20000.0]).collect();
        for ring in [points, reversed, shifted] {
            let r = area_of(&ring).unwrap();
            assert_eq!(r.label, "100.00 m²", "{ring:?}");
            assert_eq!(r.perimeter.as_deref(), Some("40.00 m"), "{ring:?}");
        }
    }

    #[test]
    fn js_numbers_print_as_javascript_prints_them() {
        assert_eq!(js_string(2378.0), "2378");
        assert_eq!(js_string(0.5), "0.5");
        assert_eq!(js_string(1e21), "1e+21");
        assert_eq!(js_to_fixed(1.005, 2), "1.00");
        assert_eq!(js_to_fixed(0.125, 2), "0.13");
        assert_eq!(js_to_fixed(2.5, 0), "3");
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(group("1234567", ","), "1,234,567");
        assert_eq!(group("1e+21", ","), "1e+21");
    }
}
