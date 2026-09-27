//! JavaScript's numeric built-ins, where Rust's differ.
//!
//! The TypeScript is the oracle, so arithmetic that reaches a `Math.*` call
//! uses these. Every downstream port that rounds, clamps or prints a number
//! the way the TypeScript does uses them too.

/// `Math.round`: the nearest integer, ties toward +infinity. `f64::round`
/// breaks ties away from zero (`-2.5` is `-3` there, `-2` here), and the
/// `floor(x + 0.5)` shortcut is wrong at `0.49999999999999994` and past 2^52.
pub fn round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.max(a, b)`: NaN if either is NaN (`f64::max` returns the other),
/// and `+0` over `-0`.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        // Equal is only interesting for zeros: `+0` wins.
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: NaN if either is NaN, and `-0` over `+0`.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        // Equal is only interesting for zeros: `-0` wins.
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.max(...values)`; `-Infinity` for none.
pub fn max_of(values: &[f64]) -> f64 {
    values.iter().fold(f64::NEG_INFINITY, |acc, &v| max(acc, v))
}

/// `Math.min(...values)`; `Infinity` for none.
pub fn min_of(values: &[f64]) -> f64 {
    values.iter().fold(f64::INFINITY, |acc, &v| min(acc, v))
}

/// `Math.sign`: `±0` and NaN map to themselves (`f64::signum(0.0)` is `1`).
pub fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.hypot(a, b)`, as V8 computes it (`math.tq`): an infinity wins over
/// NaN, then the values are normalized by the largest and summed with Kahan
/// compensation. The platform `hypot` can differ from it in the last bit, and
/// `normalizeQuad` compares these lengths against a threshold.
pub fn hypot(a: f64, b: f64) -> f64 {
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

/// `Number.prototype.toString()` (ECMAScript `Number::toString`, radix 10):
/// the shortest round-trip digits, laid out in plain notation for exponents
/// from -7 to 20 and scientific (`1e+21`, `1.5e-7`) outside it. Rust's `{}`
/// never switches to scientific notation and prints `-0` as `-0`.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    if x.is_infinite() {
        return "Infinity".into();
    }
    // `{:e}` gives the shortest round-trip digits as `d[.ddd]e<exp>`.
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("LowerExp has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("integer exponent") + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_javascript() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.2345e-7, "1.2345e-7"),
            (5e-324, "5e-324"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
            (-12.5, "-12.5"),
            (123456789012345680000.0, "123456789012345680000"),
        ];
        for (x, want) in cases {
            assert_eq!(number_to_string(*x), *want, "{x}");
        }
    }

    #[test]
    fn round_is_math_round() {
        for (x, want) in
            [(2.5, 3.0), (-2.5, -2.0), (-0.5, 0.0), (0.49999999999999994, 0.0), (-1.5, -1.0)]
        {
            assert_eq!(round(x), want, "{x}");
        }
        assert_eq!(round(4503599627370497.0), 4503599627370497.0);
        assert!(round(f64::NAN).is_nan());
        assert_eq!(round(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn min_max_propagate_nan() {
        assert!(max(1.0, f64::NAN).is_nan());
        assert!(min(f64::NAN, 1.0).is_nan());
        assert!(max(0.0, -0.0).is_sign_positive());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert_eq!(min_of(&[]), f64::INFINITY);
    }

    /// Only `normalizeQuad` calls `sign` inside this crate, never with 0;
    /// the downstream ports (annotation-core, stage) can pass either zero.
    #[test]
    fn sign_keeps_signed_zeros_and_nan() {
        assert_eq!(sign(3.0), 1.0);
        assert_eq!(sign(-3.0), -1.0);
        assert!(sign(0.0) == 0.0 && sign(0.0).is_sign_positive());
        assert!(sign(-0.0) == 0.0 && sign(-0.0).is_sign_negative());
        assert!(sign(f64::NAN).is_nan());
    }
}
