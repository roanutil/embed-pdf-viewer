//! A vector with too few arguments fails its case with an error (or reads
//! the missing ones as `null`); it never panics the runner.

use epdf_ui_core::harness::functions;
use serde_json::{Value, json};

#[test]
fn every_function_survives_missing_arguments() {
    for (_, f) in functions() {
        for args in [Vec::new(), vec![Value::Null], vec![json!({})]] {
            let _ = f(&args);
        }
    }
}
