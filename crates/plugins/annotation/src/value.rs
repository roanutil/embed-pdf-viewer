//! Reading the engine's DTOs, which this port keeps as JSON (the model
//! carries each one verbatim in `Annot.data`), with JavaScript's rules for a
//! missing member: `undefined`, which arithmetic turns into `NaN`.

use epdf_annotation_core::{PdfPoint, PdfRect};
use serde_json::{Value, json};

/// `v[key]`, with a JSON `undefined` marker read as absent.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|m| !epdf_kernel::is_undefined(m))
}

/// `v[key]` as a number: `NaN` when absent or not a number.
pub fn f64_of(v: &Value, key: &str) -> f64 {
    get(v, key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// `v[key]` as a number when it is one.
pub fn opt_f64(v: &Value, key: &str) -> Option<f64> {
    get(v, key).and_then(Value::as_f64)
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    get(v, key).and_then(Value::as_str)
}

/// JavaScript truthiness of a member.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(v @ Value::Object(o)) => {
            if epdf_kernel::is_undefined(v) {
                false
            } else if let Some(n) = o.get("$number") {
                n != "NaN"
            } else {
                true
            }
        }
        Some(Value::Array(_)) => true,
    }
}

/// A number's truthiness: neither zero nor `NaN`.
pub fn truthy_num(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// A number as canonical JSON: the non-finite values spelled as the
/// conformance format spells them (`JSON.stringify` has no spelling).
pub fn number(x: f64) -> Value {
    if x.is_nan() {
        json!({ "$number": "NaN" })
    } else if x == f64::INFINITY {
        json!({ "$number": "Infinity" })
    } else if x == f64::NEG_INFINITY {
        json!({ "$number": "-Infinity" })
    } else if x == x.trunc() && x.abs() < 9_007_199_254_740_992.0 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

/// A wire `PdfRect`; missing edges read as `NaN`.
pub fn pdf_rect_of(v: Option<&Value>) -> PdfRect {
    let v = v.unwrap_or(&Value::Null);
    PdfRect {
        left: f64_of(v, "left"),
        bottom: f64_of(v, "bottom"),
        right: f64_of(v, "right"),
        top: f64_of(v, "top"),
    }
}

/// A wire `PdfRect` when the member is an object.
pub fn opt_pdf_rect(v: Option<&Value>) -> Option<PdfRect> {
    v.filter(|r| r.is_object() && !epdf_kernel::is_undefined(r)).map(|r| pdf_rect_of(Some(r)))
}

pub fn pdf_point_of(v: Option<&Value>) -> PdfPoint {
    let v = v.unwrap_or(&Value::Null);
    PdfPoint { x: f64_of(v, "x"), y: f64_of(v, "y") }
}

pub fn point_json(x: f64, y: f64) -> Value {
    json!({ "x": number(x), "y": number(y) })
}
