//! Replays `tooling/plugin-conformance/vectors/` against the Rust kernel and
//! plugins.
//!
//! The TypeScript generator is the oracle. This crate is the other half of
//! the same harness: the scenario runner ([`scenario`]), the fake engine
//! ([`fake_engine`]), the probe plugin ([`probe`]) and canonical comparison
//! ([`canonical`]), each a port of the file of the same name under
//! `tooling/plugin-conformance/src/`. A plugin crate adds a
//! [`harness::HarnessPlugin`] and replays its vectors with [`assert_suite`].

#![allow(clippy::type_complexity)]

pub mod canonical;
pub mod fake_engine;
pub mod harness;
/// Moved to the kernel, which prints actions and state with it.
pub use epdf_kernel::js;
pub mod probe;
pub mod scenario;

use std::path::PathBuf;

use serde_json::Value;

pub use harness::{Answer, Callback, Harness, HarnessPlugin, Outcome};
pub use scenario::Registry;

/// The envelope version the generator writes (`vectors.ts`).
pub const SCHEMA_VERSION: u64 = 1;

/// Drop object members that are JavaScript `undefined`, as JSON would. The
/// TypeScript's canonical form never prints them, so a Rust value carrying
/// one is the same value.
pub fn scrub(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !epdf_kernel::is_undefined(v))
                .map(|(k, v)| (k.clone(), scrub(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(scrub).collect()),
        other => other.clone(),
    }
}

pub fn vectors_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tooling/plugin-conformance/vectors")
}

pub fn load(file: &str) -> Value {
    let path = vectors_root().join(file);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let suite: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(suite["schemaVersion"].as_u64(), Some(SCHEMA_VERSION), "{file}: schema version");
    suite
}

/// Compare one case's trace, returning the first difference.
pub fn compare_trace(expected: &[Value], actual: &[Value]) -> Option<String> {
    for (index, (want, got)) in expected.iter().zip(actual.iter()).enumerate() {
        if let Some(diff) = canonical::diff(want, got) {
            return Some(format!("step {index}: {diff}"));
        }
    }
    if expected.len() != actual.len() {
        return Some(format!("expected {} trace entries, got {}", expected.len(), actual.len()));
    }
    None
}

/// A pure function a function suite may call by name. Arguments and result
/// are canonical JSON.
pub type TableFn = fn(&[Value]) -> Result<Value, epdf_kernel::Error>;
pub type FunctionTable = Vec<(&'static str, TableFn)>;

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Replay every case of a vector file; panic listing every case that
/// differs. `PLUGIN_CONFORMANCE_ONLY=<substring>` restricts the run to the
/// cases whose name contains it.
pub fn assert_suite(file: &str, registry: &Registry, functions: &FunctionTable) {
    let suite = load(file);
    let only = std::env::var("PLUGIN_CONFORMANCE_ONLY").ok();
    let verbose = std::env::var("PLUGIN_CONFORMANCE_VERBOSE").is_ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    for case in suite["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().unwrap_or("?");
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        ran += 1;
        match suite["kind"].as_str() {
            Some("scenario") => {
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scenario::run_case(case, registry)
                }));
                match run {
                    Err(panic) => {
                        failures.push(format!("{name}: panicked: {}", panic_message(panic)));
                        epdf_kernel::rt::reset();
                    }
                    Ok(run) => {
                        let expected = case["trace"].as_array().cloned().unwrap_or_default();
                        if verbose {
                            eprintln!(
                                "== {name}\n{}",
                                serde_json::to_string_pretty(&run.trace).unwrap()
                            );
                        }
                        if let Some(diff) = compare_trace(&expected, &run.trace) {
                            failures.push(format!("{name}: {diff}"));
                        }
                    }
                }
            }
            Some("function") => {
                let fn_name = case["fn"].as_str().unwrap_or_default();
                let Some((_, f)) = functions.iter().find(|(n, _)| *n == fn_name) else {
                    failures.push(format!("{name}: no Rust function \"{fn_name}\""));
                    continue;
                };
                let args = case["args"].as_array().cloned().unwrap_or_default();
                let run = std::panic::catch_unwind(|| f(&args));
                let outcome = match run {
                    Ok(Ok(value)) => serde_json::json!({ "returned": scrub(&value) }),
                    Ok(Err(error)) => serde_json::json!({ "threw": error.to_canonical() }),
                    Err(panic) => {
                        failures.push(format!("{name}: panicked: {}", panic_message(panic)));
                        continue;
                    }
                };
                if let Some(diff) = canonical::diff(&case["outcome"], &outcome) {
                    failures.push(format!("{name}: {diff}"));
                }
            }
            other => panic!("{file}: unknown suite kind {other:?}"),
        }
    }
    assert!(ran > 0, "{file}: no case ran");
    if !failures.is_empty() {
        panic!(
            "{file}: {} of {ran} cases differ from the TypeScript:\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}
