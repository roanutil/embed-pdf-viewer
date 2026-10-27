//! `model.ts`: the signature plugin's state, in two kinds. Session state
//! (the last validation, the sign-here target, the busy flag) changes only
//! through the pure transitions below. The signature facts (the snapshot and
//! the parked two-phase signing) are engine data: a mirror folds them from
//! confirmed document events with [`fold_signature_event`].
//!
//! The verdicts and the target are held by `Rc` so a transition can tell
//! the same value from an equal one, as the TypeScript's `===` does.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::mirror::Fold;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::js::{strict_eq, truthy};

/// `SignatureState`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureState {
    /// The last validation; `None` until one ran.
    pub verdicts: Option<Rc<Value>>,
    /// The field the user chose to sign next.
    pub target: Option<Rc<Value>>,
    /// A sign or fill call is in flight.
    pub busy: bool,
}

impl SignatureState {
    /// A state as the TypeScript passes one to a transition. A member that
    /// is not there reads as `null` (`undefined` here is never compared).
    pub fn from_json(value: &Value) -> Result<SignatureState, Error> {
        let member = |key: &str| value.get(key).filter(|v| !v.is_null()).cloned().map(Rc::new);
        Ok(SignatureState {
            verdicts: member("verdicts"),
            target: member("target"),
            busy: value.get("busy").is_some_and(truthy),
        })
    }
}

/// `initialSignatureState()`.
pub fn initial_signature_state() -> SignatureState {
    SignatureState { verdicts: None, target: None, busy: false }
}

/// `sameFieldRef(left, right)`: two object-number refs by object number,
/// two names by name, and a mixed pair never.
pub fn same_field_ref(left: &Value, right: &Value) -> bool {
    let kind = |v: &Value| v.get("kind").and_then(Value::as_str).map(str::to_string);
    match (kind(left).as_deref(), kind(right).as_deref()) {
        (Some("objectNumber"), Some("objectNumber")) => {
            strict_eq(left.get("objectNumber"), right.get("objectNumber"))
        }
        (Some("fqn"), Some("fqn")) => strict_eq(left.get("name"), right.get("name")),
        _ => false,
    }
}

fn same(a: &Option<Rc<Value>>, b: &Option<Rc<Value>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

// ── transitions ──

/// `setVerdicts(state, verdicts)`: the same verdicts (by reference) change
/// nothing.
pub fn set_verdicts(state: &Rc<SignatureState>, verdicts: Option<Rc<Value>>) -> Rc<SignatureState> {
    if same(&state.verdicts, &verdicts) {
        return state.clone();
    }
    Rc::new(SignatureState { verdicts, ..(**state).clone() })
}

/// `setTarget(state, field)`: name the sign-here target; the same field
/// (by ref) changes nothing.
pub fn set_target(state: &Rc<SignatureState>, field: Option<Rc<Value>>) -> Rc<SignatureState> {
    let unchanged = same(&state.target, &field)
        || matches!((&state.target, &field), (Some(c), Some(f)) if same_field_ref(c, f));
    if unchanged {
        return state.clone();
    }
    Rc::new(SignatureState { target: field, ..(**state).clone() })
}

/// `setBusy(state, busy)`.
pub fn set_busy(state: &Rc<SignatureState>, busy: bool) -> Rc<SignatureState> {
    if state.busy == busy {
        return state.clone();
    }
    Rc::new(SignatureState { busy, ..(**state).clone() })
}

// ── projections ──

/// The verdict's `signature.index`.
pub(crate) fn verdict_index(verdict: &Value) -> Option<&Value> {
    verdict.get("signature").and_then(|s| s.get("index"))
}

/// `newlyInvalidated(previous, next)`: the verdicts that turned invalid
/// because of unsaved edits: judged on the working copy, invalid now, and
/// not already invalid in the previous validation.
pub fn newly_invalidated(previous: Option<&Value>, next: &Value) -> Vec<Value> {
    next.as_array()
        .into_iter()
        .flatten()
        .filter(|verdict| {
            let basis =
                verdict.get("modifications").and_then(|m| m.get("basis")).and_then(Value::as_str);
            if basis != Some("working-copy")
                || verdict.get("summary").and_then(Value::as_str) != Some("invalid")
            {
                return false;
            }
            let before = previous.and_then(Value::as_array).and_then(|earlier| {
                earlier.iter().find(|e| strict_eq(verdict_index(e), verdict_index(verdict)))
            });
            before.is_none_or(|b| b.get("summary").and_then(Value::as_str) != Some("invalid"))
        })
        .cloned()
        .collect()
}

// ── the signatures mirror ──

/// `emptySignatureRecord()`: the mirrored facts, `{ snapshot, pending }`.
pub fn empty_signature_record() -> Value {
    json!({ "snapshot": null, "pending": null })
}

/// A record's `snapshot`, `None` for `null`.
pub fn snapshot_of(record: &Value) -> Option<&Value> {
    record.get("snapshot").filter(|s| !s.is_null())
}

/// A record's `pending`, `None` for `null`.
pub fn pending_of(record: &Value) -> Option<&Value> {
    record.get("pending").filter(|s| !s.is_null())
}

/// `hasSignedField(snapshot)`: something in the snapshot is signed (by the
/// truthiness of `signed`).
pub fn has_signed_field(snapshot: Option<&Value>) -> bool {
    snapshot.and_then(|s| s.get("signatures")).and_then(Value::as_array).is_some_and(|signatures| {
        signatures.iter().any(|signature| signature.get("signed").is_some_and(truthy))
    })
}

/// `completeResultOf(event)`: the result a `signatures.completed` event
/// carries, without the event envelope. A member the event lacks prints as
/// `undefined`.
pub fn complete_result_of(event: &Value) -> Value {
    let mut out = Map::new();
    for key in ["status", "signature", "version", "previous", "protection", "meta"] {
        out.insert(key.into(), event.get(key).cloned().unwrap_or_else(epdf_kernel::undefined));
    }
    Value::Object(out)
}

/// `withCompletedSignature(snapshot, event)`: the sealed field's facts and
/// the protection now in force.
fn with_completed_signature(snapshot: Option<&Value>, event: &Value) -> Value {
    let Some(snapshot) = snapshot else { return Value::Null };
    let sealed = event.get("signature").cloned().unwrap_or_else(epdf_kernel::undefined);
    let sealed_field = sealed.get("field").cloned().unwrap_or(Value::Null);
    let mut next = snapshot.as_object().cloned().unwrap_or_default();
    let signatures: Vec<Value> = snapshot
        .get("signatures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|signature| {
            let field = signature.get("field").cloned().unwrap_or(Value::Null);
            if same_field_ref(&field, &sealed_field) { sealed.clone() } else { signature.clone() }
        })
        .collect();
    next.insert("signatures".into(), Value::Array(signatures));
    next.insert(
        "protection".into(),
        event.get("protection").cloned().unwrap_or_else(epdf_kernel::undefined),
    );
    Value::Object(next)
}

/// Events that change which signature fields exist; the snapshot lists
/// fields, so it is re-read.
const FIELD_SET_EVENTS: [&str; 5] = [
    "forms.created",
    "forms.deleted",
    "forms.widgetAdded",
    "forms.widgetRemoved",
    "forms.repaired",
];

/// `foldSignatureEvent(record, event)`: one confirmed document event
/// applied to the signature facts. Pure, the same for every origin.
pub fn fold_signature_event(record: &Rc<Value>, event: &Value) -> Fold {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
    let with = |key: &str, value: Value| {
        let mut next = record.as_object().cloned().unwrap_or_default();
        next.insert(key.into(), value);
        Fold::Value(Rc::new(Value::Object(next)))
    };
    match kind {
        "signatures.prepared" => with(
            "pending",
            json!({
                "signingId": event.get("signingId").cloned().unwrap_or_else(epdf_kernel::undefined),
                "field": event.get("field").cloned().unwrap_or_else(epdf_kernel::undefined),
            }),
        ),
        "signatures.cancelled" => {
            if pending_of(record).is_some() {
                with("pending", Value::Null)
            } else {
                Fold::Value(record.clone())
            }
        }
        "signatures.completed" => Fold::Value(Rc::new(json!({
            "snapshot": with_completed_signature(snapshot_of(record), event),
            "pending": null,
        }))),
        kind if FIELD_SET_EVENTS.contains(&kind) => Fold::Reload,
        _ => Fold::Value(record.clone()),
    }
}
