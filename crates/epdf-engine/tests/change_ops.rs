//! The change ops and reads upstream added after 074a190c9:
//! `vectors/changes/*.json` replayed through `Host::call`.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run changes
//!
//! Each case is a list of steps against a freshly opened fixture, and each
//! step records the worker requests one call of the TypeScript API sent,
//! with the worker's answer to each. This sends the same requests in order
//! and compares each answer: the result (the worker's payload without its
//! `tag`, as the `Host` answers it) or the refusal's code. The answers are
//! masked as the generator masks them: a name a copy made up (a UUIDv7) is
//! `"<uuid>"`, and a timestamp the clock wrote (any after 2025; the
//! fixtures' are from 2024) is `"<now>"`.

mod common;

use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
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
    exchanges: Vec<Exchange>,
}

#[derive(Debug, Deserialize)]
struct Exchange {
    request: Value,
    answer: Value,
}

fn load(file: &str, kind: &str) -> VectorFile {
    let path = test_support::corpus::vectors_dir("changes").join(file);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let vectors: VectorFile = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
    assert_eq!(vectors.schema_version, SCHEMA_VERSION, "{file}: vector envelope moved");
    assert_eq!(vectors.kind, kind, "{file}");
    vectors
}

fn is_uuid_v7(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b.iter().enumerate().all(|(i, c)| {
            [8, 13, 18, 23].contains(&i) || c.is_ascii_digit() || (b'a'..=b'f').contains(c)
        })
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

/// An ISO 8601 timestamp from 2025 on, which only the clock writes here.
fn is_clock_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let shaped = b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[..4].iter().all(u8::is_ascii_digit);
    shaped && s[..4].parse::<u32>().is_ok_and(|year| year >= 2025)
}

fn mask(value: &Value) -> Value {
    match value {
        Value::String(s) if is_uuid_v7(s) => json!("<uuid>"),
        Value::String(s) if is_clock_timestamp(s) => json!("<now>"),
        Value::Array(items) => Value::Array(items.iter().map(mask).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| match k.as_str() {
                    "jobId" => (k.clone(), json!("<job>")),
                    "docId" => (k.clone(), json!("<doc>")),
                    _ => (k.clone(), mask(v)),
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// The worker's payload as the `Host` answers it: without its `tag`, and a
/// read whose one field is its `snapshot` or `list` as that value, when the
/// `Host` answers the value itself (`pages.list`, `annotations.list`).
fn expected_result(resolve: &Value, actual: &Value) -> Value {
    let mut payload: Map<String, Value> = resolve.as_object().cloned().unwrap_or_default();
    payload.remove("tag");
    for wrapper in ["snapshot", "list"] {
        if payload.len() == 1 && payload.contains_key(wrapper) && actual.get(wrapper).is_none() {
            return payload.remove(wrapper).unwrap();
        }
    }
    Value::Object(payload)
}

/// What the `Host` adds to `document.apply`'s answer beside the worker's
/// payload: the events it publishes and the mutation sequence, the events
/// envelope this port keeps (not part of the TypeScript worker's answer).
const HOST_ONLY: [&str; 2] = ["events", "mutationSequence"];

/// The paths where `actual` and `expected` differ, up to `limit`.
fn differences(actual: &Value, expected: &Value, at: &str, out: &mut Vec<String>, limit: usize) {
    if out.len() >= limit || actual == expected {
        return;
    }
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            for key in a.keys().chain(e.keys().filter(|k| !a.contains_key(*k))) {
                let path = format!("{at}/{key}");
                match (a.get(key), e.get(key)) {
                    (Some(x), Some(y)) => differences(x, y, &path, out, limit),
                    (Some(x), None) => out.push(format!("{path}: only in Rust: {x}")),
                    (None, Some(y)) => out.push(format!("{path}: missing, expected {y}")),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            for (i, (x, y)) in a.iter().zip(e).enumerate() {
                differences(x, y, &format!("{at}/{i}"), out, limit);
            }
        }
        _ => out.push(format!("{at}: {actual} expected {expected}")),
    }
}

/// The fixture opened as `LocalEngine` opens a document: with the first 64
/// object numbers reserved for the session (`OBJECT_NUMBER_POOL_SIZE`), which
/// is where a create's next free number starts and which numbers a copy may
/// be given.
fn open(fixture: &str, doc_id: &str) -> Replay {
    let mut host = epdf_engine::host::Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": doc_id,
            "password": null, "reserveObjectNumbers": 64 })
        .to_string(),
        vec![test_support::fixture(fixture)],
    );
    let envelope: Value = serde_json::from_str(&response.json).expect("the open answers JSON");
    assert!(envelope.get("error").is_none(), "{fixture} would not open: {envelope}");
    Replay::adopt(host, doc_id)
}

fn replay_file(file: &str, kind: &str, cases: usize) {
    let vectors = load(file, kind);
    assert_eq!(vectors.cases.len(), cases, "{file}: the case list moved");
    let mut failures = Vec::new();
    for case in &vectors.cases {
        let mut replay = open(&vectors.fixture, &format!("changes:{}", case.name));
        'steps: for (s, step) in case.steps.iter().enumerate() {
            for exchange in &step.exchanges {
                let what = format!("{} step {s} {}", case.name, exchange.request["kind"]);
                let answer = replay.replay(&exchange.request);
                match (exchange.answer.get("resolve"), exchange.answer.get("reject")) {
                    (Some(resolve), None) => match answer {
                        Ok(mut actual) => {
                            if let Some(fields) = actual.as_object_mut() {
                                for key in HOST_ONLY {
                                    fields.remove(key);
                                }
                            }
                            let actual = test_support::canonical_numbers(&mask(&actual));
                            let expected =
                                test_support::canonical_numbers(&expected_result(resolve, &actual));
                            if actual != expected {
                                let mut found = Vec::new();
                                differences(&actual, &expected, "", &mut found, 8);
                                failures.push(format!(
                                    "{what}: {}\n    {}",
                                    case.covers,
                                    found.join("\n    ")
                                ));
                                break 'steps;
                            }
                        }
                        Err(error) => {
                            failures.push(format!("{what}: refused {error}; {}", case.covers));
                            break 'steps;
                        }
                    },
                    (None, Some(code)) => match answer {
                        Err(error) if error["code"] == *code => {}
                        Err(error) => {
                            failures.push(format!("{what}: refused {error}, expected {code}"));
                            break 'steps;
                        }
                        Ok(actual) => {
                            failures.push(format!("{what}: answered {actual}, expected {code}"));
                            break 'steps;
                        }
                    },
                    _ => panic!("{what}: records neither a resolve nor a reject"),
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_layout_carries_each_pages_viewports() {
    replay_file("viewports.json", "pages.list.viewports", 1);
}

#[test]
fn a_scale_is_a_change() {
    replay_file("scale.json", "document.apply.pages.setScale", 10);
}

#[test]
fn an_update_renames() {
    replay_file("rename.json", "document.apply.annotations.rename", 7);
}

#[test]
fn annotations_copy() {
    replay_file("annotation-copy.json", "document.apply.annotations.copy", 10);
}

#[test]
fn forms_copy() {
    replay_file("form-copy.json", "document.apply.forms.copy", 8);
}

/// `kinds.json`: the version each change kind arrived in, which kinds an
/// older or newer side lacks, and `lacksWhatArrivedIn` itself (a4805a492).
#[test]
fn every_change_kind_has_the_version_it_arrived_in() {
    use epdf_engine::features::changes::kinds::{
        API_VERSION, CHANGE_KIND_SINCE, CHANGE_KINDS, change_kind_since, change_kinds_unknown_to,
        lacks_what_arrived_in,
    };
    let path = test_support::corpus::vectors_dir("changes").join("kinds.json");
    let text = std::fs::read_to_string(&path).expect("kinds.json");
    let vectors: Value = serde_json::from_str(&text).expect("kinds.json");
    assert_eq!(vectors["schemaVersion"], json!(SCHEMA_VERSION));
    assert_eq!(vectors["kind"], json!("wire.changeKinds"));
    assert_eq!(vectors["apiVersion"], json!(API_VERSION), "the API version moved");

    let since = vectors["since"].as_object().expect("since");
    let kinds: Vec<&str> = since.keys().map(String::as_str).collect();
    let mut ours = CHANGE_KINDS.to_vec();
    let mut theirs = kinds.clone();
    ours.sort_unstable();
    theirs.sort_unstable();
    assert_eq!(ours, theirs, "the change kinds moved");
    for (kind, version) in since {
        assert_eq!(change_kind_since(kind), version, "{kind}");
    }
    let rows: Map<String, Value> =
        CHANGE_KIND_SINCE.iter().map(|(kind, since)| ((*kind).to_owned(), json!(since))).collect();
    assert_eq!(Value::Object(rows), vectors["rows"]);

    let asked: Vec<&str> =
        kinds.iter().copied().chain(["pages.someKindFromANewerServer"]).collect();
    for row in vectors["unknownTo"].as_array().expect("unknownTo") {
        let version = row["version"].as_str();
        let unknown = change_kinds_unknown_to(version, asked.iter().copied());
        let mut unknown: Vec<&str> = unknown;
        let mut expected: Vec<&str> =
            row["unknown"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
        // The kinds are asked in the vector's object order, which a map need
        // not keep; the set is what is compared.
        unknown.sort_unstable();
        expected.sort_unstable();
        assert_eq!(unknown, expected, "unknown to {version:?}");
    }
    for row in vectors["lacks"].as_array().expect("lacks") {
        let (version, since) = (row["version"].as_str(), row["since"].as_str());
        assert_eq!(
            json!(lacks_what_arrived_in(version, since)),
            row["lacks"],
            "{version:?} lacks {since:?}"
        );
    }
}
