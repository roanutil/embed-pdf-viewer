//! Reading engine answers, which this port keeps as JSON, with JavaScript's
//! rules: a missing member is `undefined`.

use serde_json::Value;

pub use epdf_plugin_annotation::value::{get, str_of, truthy};

/// `v[key]` as a number, when it is one.
pub fn num(v: &Value, key: &str) -> Option<f64> {
    get(v, key).and_then(Value::as_f64)
}
