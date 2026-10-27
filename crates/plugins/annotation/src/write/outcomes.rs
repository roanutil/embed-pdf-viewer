//! `write/outcomes.ts`: what a verb returns once the engine answered its
//! change: a `BatchResult` over the annotations it addressed, the ref of the
//! record it created, or the refusal as an error.

use std::collections::HashMap;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::controller::CAPABILITY;
use crate::js::{key_of, key_or_none};
use crate::services::store::{Applied, AppliedOutcome, Commit, Written};

/// `batchResultOf(refs, outcome)`: each ref applied, or failed with the refusal.
pub fn batch_result_of(refs: &[Value], failed: &[(Vec<String>, Error)]) -> Value {
    let mut errors: HashMap<String, Error> = HashMap::new();
    for (ids, error) in failed {
        for id in ids {
            errors.insert(id.clone(), error.clone());
        }
    }
    let applied: Vec<Value> =
        refs.iter().filter(|r| !errors.contains_key(&key_or_none(r))).cloned().collect();
    let failed: Vec<Value> = refs
        .iter()
        .filter_map(|r| {
            errors.get(&key_or_none(r)).map(|e| json!({ "ref": r, "error": e.plugin_error_info() }))
        })
        .collect();
    json!({ "applied": applied, "skipped": [], "failed": failed })
}

/// `throwIfFailed(outcome)`: the first refusal.
pub fn throw_if_failed(outcome: &Written) -> Result<(), Error> {
    match outcome.failed.first() {
        Some((_, error)) => Err(error.clone()),
        None => Ok(()),
    }
}

fn not_created() -> Error {
    plugin_error(
        code::OPERATION_FAILED,
        CAPABILITY,
        "the annotation could not be created",
        None,
        None,
    )
}

/// The annotation the engine created for record `id`, from its answer.
fn created_in(outcome: &Written, id: &str) -> Option<Value> {
    let items = outcome.result.as_ref()?.get("items")?.as_array()?;
    items
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("annotations.create")
                && item.pointer("/annotation/ref").and_then(key_of).as_deref() == Some(id)
        })
        .and_then(|item| item.get("annotation").cloned())
}

/// `createdRefOf(commit)`: the ref of the record a create message made (its
/// first), once the engine wrote it.
pub fn created_ref_of(commit: &Commit) -> Promise<Value> {
    let effect = commit.effects.iter().find_map(|e| match e {
        epdf_annotation_core::types::Effect::Create(c) => Some(c.id.clone()),
        _ => None,
    });
    let written = commit.written.clone();
    Promise::spawn(async move {
        let outcome = written.await?;
        throw_if_failed(&outcome)?;
        let created = effect.and_then(|id| created_in(&outcome, &id));
        match created {
            Some(annotation) => Ok(annotation.get("ref").cloned().unwrap_or(Value::Null)),
            None => Err(not_created()),
        }
    })
}

/// `appliedOrThrow(applied)`: wait for stated changes; rejects with the first refusal.
pub fn applied_or_throw(applied: &Applied) -> Promise<AppliedOutcome> {
    let written = applied.written.clone();
    Promise::spawn(async move {
        let outcome = written.await?;
        throw_if_failed(&outcome.written)?;
        Ok(outcome)
    })
}

/// `appliedAnnotationOf(applied, index)`: what the engine wrote for stated change `index`.
pub fn applied_annotation_of(applied: &Applied, index: usize) -> Promise<Option<Value>> {
    let outcome = applied_or_throw(applied);
    Promise::spawn(async move {
        let outcome = outcome.await?;
        Ok(outcome.annotations.get(index).cloned().flatten())
    })
}

/// `appliedRefOf(applied, index)`: the ref of the record a stated create made.
pub fn applied_ref_of(applied: &Applied, index: usize) -> Promise<Value> {
    let annotation = applied_annotation_of(applied, index);
    Promise::spawn(async move {
        match annotation.await? {
            Some(annotation) => Ok(annotation.get("ref").cloned().unwrap_or(Value::Null)),
            None => Err(not_created()),
        }
    })
}
