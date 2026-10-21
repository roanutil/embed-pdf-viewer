//! Replaying a vector's recorded `request` through [`Host::call`].
//!
//! The generators whose case literals keep an older spelling record the
//! wire request they actually sent, in a `request` field, with the job and
//! op ids fixed as `"<job>"` and `"<op>"` and the generator's own `docId`.
//! A test sends that request rather than rebuilding one from the case's
//! fields (`docs/research/catchup-engine-vector-diffs.md`, "Recorded
//! requests"), after [`substitute`] puts this run's ids in.
//!
//! Here rather than in `crates/test-support`: that crate depending on
//! `epdf-engine` would make a dependency cycle.

use epdf_engine::host::{Host, Payloads};
use serde_json::{Value, json};

/// The placeholder a recorded request carries for its job id.
pub const JOB: &str = "<job>";
/// The placeholder a recorded request carries for its op id.
pub const OP: &str = "<op>";

/// `recorded` with `doc_id` as its `docId`, `job_id` in place of every
/// `"<job>"` and `op_id` in place of every `"<op>"`, at any depth (a
/// `document.applyChanges` names an op per change). Nothing else changes.
pub fn substitute(recorded: &Value, doc_id: &str, job_id: u64, op_id: &str) -> Value {
    fn walk(value: &mut Value, job_id: u64, op_id: &str) {
        match value {
            Value::String(s) if s == JOB => *value = json!(job_id),
            Value::String(s) if s == OP => *value = json!(op_id),
            Value::Array(items) => items.iter_mut().for_each(|v| walk(v, job_id, op_id)),
            Value::Object(map) => map.values_mut().for_each(|v| walk(v, job_id, op_id)),
            _ => {}
        }
    }
    let mut request = recorded.clone();
    walk(&mut request, job_id, op_id);
    if let Value::Object(map) = &mut request {
        map.insert("docId".into(), json!(doc_id));
    }
    request
}

/// What one replayed request answered: its `result` or its `error`, the
/// event a mutation or a notice published, and the binary payloads.
#[derive(Debug)]
pub struct Answer {
    pub outcome: Result<Value, Value>,
    pub event: Option<Value>,
    pub payloads: Payloads,
}

/// A host with one fixture open, replaying recorded requests against it.
pub struct Replay {
    pub host: Host,
    pub doc_id: String,
    next_job: u64,
}

impl Replay {
    /// Open the repo-relative `fixture` in a fresh [`Host`] as `doc_id`.
    pub fn open(fixture: &str, doc_id: &str) -> Self {
        Self::open_in(Host::new(), fixture, doc_id)
    }

    /// [`Replay::open`] in a host the caller built (a fixed clock, an
    /// encoder).
    pub fn open_in(mut host: Host, fixture: &str, doc_id: &str) -> Self {
        let response = host.call(
            &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": doc_id,
                "password": null })
            .to_string(),
            vec![test_support::fixture(fixture)],
        );
        let envelope: Value = serde_json::from_str(&response.json).expect("the open answers JSON");
        assert!(envelope.get("error").is_none(), "{fixture} would not open: {envelope}");
        Self { host, doc_id: doc_id.to_owned(), next_job: 2 }
    }

    /// Send `recorded`, with this run's ids, through [`Host::call`].
    pub fn send(&mut self, recorded: &Value, payloads: Payloads) -> Answer {
        let job_id = self.next_job;
        self.next_job += 1;
        let request = substitute(recorded, &self.doc_id, job_id, &format!("op-{job_id}"));
        let response = self.host.call(&request.to_string(), payloads);
        let mut envelope: Value =
            serde_json::from_str(&response.json).expect("every response is JSON");
        let event = envelope.get_mut("event").map(Value::take);
        let outcome = match envelope.get_mut("error") {
            Some(error) => Err(error.take()),
            None => Ok(envelope.get_mut("result").map(Value::take).unwrap_or(Value::Null)),
        };
        Answer { outcome, event, payloads: response.payloads }
    }

    /// [`Replay::send`] with no payloads, answering the result or the error.
    pub fn replay(&mut self, recorded: &Value) -> Result<Value, Value> {
        self.send(recorded, Vec::new()).outcome
    }
}
