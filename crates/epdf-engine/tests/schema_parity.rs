//! G1's validation gate: serde accepts exactly what the shipping zod schema
//! accepts, and reads it to mean the same thing.
//!
//! `multiplatform-plan.md` Section 11 keeps the zod schemas handwritten while
//! making the Rust types canonical, which only works if something holds the
//! two together. This is that something for `MetadataPatch` and
//! `CustomMetadataPatch`, the Phase 0 DTO proof; G7 and G11 extend the same
//! harness to annotation DTOs, drafts and patches.
//!
//! The vectors under `tooling/engine-conformance/vectors/schema/` carry zod's
//! verdict on each hand-written input, recorded by
//! `@embedpdf/tooling-engine-conformance`. Regenerate them, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run schema

use epdf_engine::dto::{CustomMetadataPatch, MetadataPatch};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct SchemaVectorFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    schema: String,
    cases: Vec<SchemaCase>,
}

#[derive(Debug, Deserialize)]
struct SchemaCase {
    name: String,
    covers: String,
    input: Value,
    /// Whether the shipping zod schema accepted the input.
    valid: bool,
    /// What it parsed to, re-serialized. Present only when `valid`.
    #[serde(default)]
    parsed: Option<Value>,
}

fn load(schema: &str) -> SchemaVectorFile {
    let path = test_support::workspace_root()
        .join("tooling/engine-conformance/vectors/schema")
        .join(format!("{schema}.json"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: SchemaVectorFile =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(file.schema_version, 1, "{} was written by a different generator", path.display());
    assert_eq!(file.schema, schema, "{} declares schema {}", path.display(), file.schema);
    assert!(!file.cases.is_empty(), "{} has no cases", path.display());
    file
}

/// Runs one vector file against a decode-and-re-encode round trip.
///
/// Reports every divergence rather than the first, because a schema change
/// usually moves several cases at once and fixing them one test run at a time
/// is how an afternoon goes.
fn check<T>(schema: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let file = load(schema);
    let mut failures: Vec<String> = Vec::new();

    for case in &file.cases {
        let decoded = serde_json::from_value::<T>(case.input.clone());
        match (case.valid, decoded) {
            (true, Err(e)) => failures.push(format!(
                "{}: zod accepts it, serde rejects it ({e}). {}",
                case.name, case.covers
            )),
            (false, Ok(_)) => failures
                .push(format!("{}: zod rejects it, serde accepts it. {}", case.name, case.covers)),
            (false, Err(_)) => {}
            (true, Ok(value)) => {
                let expected = case
                    .parsed
                    .as_ref()
                    .unwrap_or_else(|| panic!("{}: a valid case must carry `parsed`", case.name));
                let actual = serde_json::to_value(&value).expect("re-serialize");
                if let Some(difference) = test_support::corpus::first_difference(expected, &actual)
                {
                    failures.push(format!(
                        "{}: decodes, but re-encodes differently at {difference}. {}",
                        case.name, case.covers
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} {schema} case(s) diverged from the zod schema:\n  {}",
        failures.len(),
        file.cases.len(),
        failures.join("\n  "),
    );
}

#[test]
fn metadata_patch_agrees_with_its_zod_schema() {
    check::<MetadataPatch>("MetadataPatch");
}

#[test]
fn custom_metadata_patch_agrees_with_its_zod_schema() {
    check::<CustomMetadataPatch>("CustomMetadataPatch");
}

/// The corpus covers all three states and at least one rejection.
///
/// Without this, deleting the invalid cases would leave the gate green while
/// proving only that valid documents decode.
#[test]
fn the_schema_corpus_covers_acceptance_and_rejection() {
    let file = load("MetadataPatch");
    let rejected = file.cases.iter().filter(|case| !case.valid).count();
    let accepted = file.cases.len() - rejected;
    assert!(rejected >= 5, "only {rejected} rejection cases");
    assert!(accepted >= 5, "only {accepted} acceptance cases");

    let has_null =
        file.cases.iter().any(|case| case.valid && case.input.get("title") == Some(&Value::Null));
    let has_absent =
        file.cases.iter().any(|case| case.valid && case.input == serde_json::json!({}));
    let has_value = file
        .cases
        .iter()
        .any(|case| case.valid && case.input.get("title").is_some_and(Value::is_string));
    assert!(has_absent && has_null && has_value, "the three states are not all covered");
}
