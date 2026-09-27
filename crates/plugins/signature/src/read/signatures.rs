//! `read/signatures.ts`: reads over the last snapshot and validation. The
//! TypeScript memoizes the lists per snapshot so they stay reference-stable
//! for selectors; a Rust caller gets owned copies.

use serde_json::{Value, json};

use crate::contract::SignaturePending;
use crate::controller::SignatureCapability;
use crate::model::ResourceStatus;
use crate::services::store::{number, same_ref};

fn signatures_of(snapshot: &Value) -> Vec<Value> {
    snapshot.get("signatures").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn is_signed(signature: &Value) -> bool {
    signature.get("signed").is_some_and(truthy)
}

/// JavaScript truthiness of a JSON value.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => !epdf_kernel::is_undefined(value),
    }
}

/// `snapshot?.signatures.some((s) => s.signed)`.
pub(crate) fn any_signed(snapshot: Option<&Value>) -> bool {
    snapshot.is_some_and(|s| signatures_of(s).iter().any(is_signed))
}

/// The widget object number an address names, or `None` for a field ref.
fn widget_object_of(field: &Value) -> Option<Option<f64>> {
    if field.get("kind").is_none() {
        return Some(number(field.get("annotObjectNumber")));
    }
    if field.get("kind").and_then(Value::as_str) == Some("objectNumber")
        && field.get("annotObjectNumber").is_some()
    {
        return Some(number(field.get("annotObjectNumber")));
    }
    None
}

fn is_field_ref(field: &Value) -> bool {
    field.get("kind").is_some()
        && (field.get("kind").and_then(Value::as_str) == Some("fqn")
            || field.get("fieldObjectNumber").is_some())
}

impl SignatureCapability {
    /// `getSnapshot()`: every signature field and its facts.
    pub fn get_snapshot(&self) -> Option<Value> {
        self.state().snapshot.clone()
    }

    /// `getStatus()`.
    pub fn get_status(&self) -> ResourceStatus {
        self.state().status
    }

    /// `listSignatures()`: signed fields, in document order.
    pub fn list_signatures(&self) -> Vec<Value> {
        self.state()
            .snapshot
            .as_ref()
            .map(|s| signatures_of(s).into_iter().filter(is_signed).collect())
            .unwrap_or_default()
    }

    /// `listUnsignedFields()`: signature fields still to sign.
    pub fn list_unsigned_fields(&self) -> Vec<Value> {
        self.state()
            .snapshot
            .as_ref()
            .map(|s| {
                signatures_of(s)
                    .into_iter()
                    .filter(|s| !is_signed(s))
                    .map(|s| s.get("field").cloned().unwrap_or(Value::Null))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `getSignature(address)`: by field ref, by name, or by widget.
    pub fn get_signature(&self, field: &Value) -> Option<Value> {
        let snapshot = self.state().snapshot.clone()?;
        let signatures = signatures_of(&snapshot);
        if let Some(widget_object) = widget_object_of(field) {
            let by_widget = signatures.iter().find(|s| {
                let annot = s
                    .get("widget")
                    .filter(|w| !w.is_null())
                    .and_then(|w| number(w.get("annotObjectNumber")));
                annot == widget_object
            });
            if let Some(found) = by_widget {
                return Some(found.clone());
            }
            let widget = json!({ "annotObjectNumber": widget_object });
            let owner = self.form().get_field_for_widget(&widget)?;
            return self.get_signature(owner.get("ref").unwrap_or(&Value::Null));
        }
        if !is_field_ref(field) {
            return None;
        }
        let by_name = field.get("kind").and_then(Value::as_str) == Some("fqn");
        signatures.into_iter().find(|s| {
            if by_name {
                s.get("fieldName") == field.get("name")
            } else {
                same_ref(s.get("field").unwrap_or(&Value::Null), field)
            }
        })
    }

    /// `listVerdicts()`: the last validation, null until one ran.
    pub fn list_verdicts(&self) -> Option<Value> {
        self.state().verdicts.clone()
    }

    /// `getVerdict(address)`: one signed field's verdict from the last
    /// validation.
    pub fn get_verdict(&self, field: &Value) -> Option<Value> {
        let signature = self.get_signature(field)?;
        let index = number(signature.get("index"));
        self.state()
            .verdicts
            .as_ref()?
            .as_array()?
            .iter()
            .find(|v| number(v.get("signature").and_then(|s| s.get("index"))) == index)
            .cloned()
    }

    /// `getProtection()`: what the signatures forbid, null before a read.
    pub fn get_protection(&self) -> Option<Value> {
        self.state().snapshot.as_ref().map(|s| s.get("protection").cloned().unwrap_or(Value::Null))
    }

    /// `getPending()`.
    pub fn get_pending(&self) -> Option<SignaturePending> {
        self.state().pending.clone()
    }

    /// `isBusy()`.
    pub fn is_busy(&self) -> bool {
        self.state().busy
    }

    /// `getTarget()`: the field the next picked mark goes to.
    pub fn get_target(&self) -> Option<Value> {
        self.state().target.clone()
    }
}
