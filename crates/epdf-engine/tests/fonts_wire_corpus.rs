//! The five font kinds upstream added, against the TypeScript oracle:
//! `fonts.list`, `fonts.program`, `document.fonts.list`,
//! `document.fonts.resolve` and `document.fonts.program`, as `WorkerHost`
//! answered them on the wire.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run fonts
//!
//! Each case replays its requests, in order, against one fresh host, the
//! bytes a request carried as payloads. A step the case is about compares
//! the Rust result with the TypeScript one, field for field and in the same
//! numbers; bytes are compared by length and SHA-256, which is how the vector
//! records them. A failure compares only its code: the message is
//! diagnosis. The opens and registers before them only have to succeed.
//!
//! PDFium's font registry is per thread, so every case runs on a thread of
//! its own.

use epdf_engine::host::Host;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    covers: String,
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    request: Value,
    #[serde(default)]
    payloads: Vec<String>,
    answer: Value,
}

fn vectors() -> VectorFile {
    let path = test_support::corpus::vectors_dir("fonts").join("wire.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("wire.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "fonts/wire.json schema version");
    assert_eq!(file.kind, "fonts.wire");
    file
}

fn hex(digest: [u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The Rust result as the vector records one: each `{ payload: n }` as the
/// length and SHA-256 of that payload.
fn recorded(value: &Value, payloads: &[Vec<u8>]) -> Value {
    match value {
        Value::Object(fields) => {
            if fields.len() == 1
                && let Some(index) = fields.get("payload").and_then(Value::as_u64)
            {
                let bytes = &payloads[index as usize];
                let digest = pdfium::sha256(bytes).expect("EPDF_DigestBuffer");
                return json!({ "$bytes": { "length": bytes.len(), "sha256": hex(digest) } });
            }
            let out: Map<String, Value> =
                fields.iter().map(|(k, v)| (k.clone(), recorded(v, payloads))).collect();
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| recorded(v, payloads)).collect()),
        other => other.clone(),
    }
}

fn replay(case: Case) -> Result<usize, String> {
    let mut host = Host::new();
    let mut compared = 0;
    for (index, step) in case.steps.iter().enumerate() {
        let payloads: Vec<Vec<u8>> =
            step.payloads.iter().map(|fixture| test_support::fixture(fixture)).collect();
        let response = host.call_filled(&step.request.to_string(), payloads);
        let json: Value = serde_json::from_str(&response.json).expect("every response is JSON");
        let at = format!("{} step {index} ({})", case.name, step.request["kind"]);
        if step.answer.get("ok").is_some() {
            if let Some(error) = json.get("error") {
                return Err(format!("{at}: the setup step failed: {error}"));
            }
            continue;
        }
        compared += 1;
        if let Some(want) = step.answer.get("error") {
            if json["error"]["code"] != want["code"] {
                return Err(format!("{at}: expected the error {want}, got {json}"));
            }
            continue;
        }
        let Some(result) = json.get("result") else {
            return Err(format!("{at}: expected a result, got {json}"));
        };
        let expected = test_support::corpus::quantized(&step.answer["result"]);
        let actual = test_support::corpus::quantized(&recorded(result, &response.payloads));
        if let Some(difference) = test_support::corpus::first_difference(&expected, &actual) {
            return Err(format!("{at}: {difference} ({})", case.covers));
        }
    }
    Ok(compared)
}

#[test]
fn the_font_kinds_answer_what_the_typescript_did() {
    let file = vectors();
    assert!(!file.cases.is_empty());
    let mut compared = 0;
    let mut failures = Vec::new();
    for case in file.cases {
        match std::thread::spawn(move || replay(case)).join().expect("a case panicked") {
            Ok(n) => compared += n,
            Err(failure) => failures.push(failure),
        }
    }
    assert!(failures.is_empty(), "{} cases differ:\n{}", failures.len(), failures.join("\n"));
    assert!(compared > 0, "no step was compared");
}

#[test]
fn every_new_font_kind_is_pinned() {
    let file = vectors();
    let kinds: std::collections::BTreeSet<String> = file
        .cases
        .iter()
        .flat_map(|case| case.steps.iter())
        .filter(|step| step.answer.get("ok").is_none())
        .filter_map(|step| step.request["kind"].as_str().map(str::to_string))
        .collect();
    for kind in [
        "fonts.list",
        "fonts.program",
        "document.fonts.list",
        "document.fonts.resolve",
        "document.fonts.program",
    ] {
        assert!(kinds.contains(kind), "no vector step pins {kind}");
    }
}
