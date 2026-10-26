//! `read/signatures.ts`: reads over the signature facts and the last
//! validation. The lists are memoized per snapshot and verdicts, so they
//! stay the same `Rc` until one changes.

use std::rc::Rc;

use epdf_kernel::mirror::ResourceStatus;
use serde_json::Value;

use crate::contract::SignaturePending;
use crate::controller::SignatureCapability;
use crate::js::{strict_eq, truthy};
use crate::model::{pending_of, same_field_ref, snapshot_of, verdict_index};

/// The memoized lists and what they were made from.
pub(crate) struct Lists {
    record: Rc<Value>,
    verdicts: Option<Rc<Value>>,
    signed: Rc<Vec<Value>>,
    unsigned: Rc<Vec<Value>>,
}

fn has(value: &Value, key: &str) -> bool {
    value.get(key).is_some()
}

/// `widgetOf(address)`: the widget an address names (a `FormWidget`, or an
/// annotation ref, which names its page), or `None` for a field ref.
fn widget_of(field: &Value) -> Option<&Value> {
    if !has(field, "kind") {
        return Some(field);
    }
    if field.get("kind").and_then(Value::as_str) == Some("objectNumber") && has(field, "page") {
        return Some(field);
    }
    None
}

/// `isFieldRef(address)`.
fn is_field_ref(field: &Value) -> bool {
    let kind = field.get("kind").and_then(Value::as_str);
    has(field, "kind")
        && (kind == Some("fqn") || (kind == Some("objectNumber") && !has(field, "page")))
}

fn signatures_of(snapshot: &Value) -> &[Value] {
    snapshot.get("signatures").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn is_signed(signature: &Value) -> bool {
    signature.get("signed").is_some_and(truthy)
}

impl SignatureCapability {
    /// `snapshot()`: the mirrored snapshot, `None` before a load or without
    /// engine support.
    pub(crate) fn snapshot(&self) -> Option<Value> {
        snapshot_of(&self.mirror().get()).cloned()
    }

    /// `verdictOf(signature)` over the given verdicts.
    fn verdict_in(verdicts: Option<&Value>, signature: &Value) -> Option<Value> {
        verdicts?
            .as_array()?
            .iter()
            .find(|verdict| strict_eq(verdict_index(verdict), signature.get("index")))
            .cloned()
    }

    fn lists(&self) -> (Rc<Vec<Value>>, Rc<Vec<Value>>) {
        let record = self.mirror().get();
        let verdicts = self.state().verdicts.clone();
        if let Some(kept) = &*self.0.lists.borrow() {
            let same_verdicts = match (&kept.verdicts, &verdicts) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            };
            if Rc::ptr_eq(&kept.record, &record) && same_verdicts {
                return (kept.signed.clone(), kept.unsigned.clone());
            }
        }
        let (signed, unsigned) = match snapshot_of(&record) {
            None => (Vec::new(), Vec::new()),
            Some(current) => {
                let signatures = signatures_of(current);
                let signed = signatures
                    .iter()
                    .filter(|s| is_signed(s))
                    .map(|signature| {
                        let mut info = signature.as_object().cloned().unwrap_or_default();
                        let verdict = Self::verdict_in(verdicts.as_deref(), signature);
                        info.insert("verdict".into(), verdict.unwrap_or(Value::Null));
                        Value::Object(info)
                    })
                    .collect();
                let unsigned = signatures
                    .iter()
                    .filter(|s| !is_signed(s))
                    .map(|s| s.get("field").cloned().unwrap_or_else(epdf_kernel::undefined))
                    .collect();
                (signed, unsigned)
            }
        };
        let (signed, unsigned) = (Rc::new(signed), Rc::new(unsigned));
        *self.0.lists.borrow_mut() =
            Some(Lists { record, verdicts, signed: signed.clone(), unsigned: unsigned.clone() });
        (signed, unsigned)
    }

    /// `getSnapshot()`: every signature field and its facts, the file's
    /// versions and `protection`.
    pub fn get_snapshot(&self) -> Option<Value> {
        self.snapshot()
    }

    /// `getStatus()`: whether the signatures are read.
    pub fn get_status(&self) -> ResourceStatus {
        self.mirror().get_status()
    }

    /// `listSignatures()`: the signed fields, in document order, each with
    /// its verdict once checked.
    pub fn list_signatures(&self) -> Rc<Vec<Value>> {
        self.lists().0
    }

    /// `listUnsignedFields()`: signature fields still to sign.
    pub fn list_unsigned_fields(&self) -> Rc<Vec<Value>> {
        self.lists().1
    }

    /// `getSignature(address)`: one signature field's facts, signed or not,
    /// by its ref or its widget's, or `None`.
    pub fn get_signature(&self, field: &Value) -> Option<Value> {
        let current = self.snapshot()?;
        let signatures = signatures_of(&current);
        if let Some(widget) = widget_of(field) {
            let by_widget = signatures.iter().find(|signature| {
                let own = signature
                    .get("widget")
                    .filter(|w| !w.is_null() && !epdf_kernel::is_undefined(w))
                    .and_then(|w| w.get("objectNumber"));
                strict_eq(own, widget.get("objectNumber"))
            });
            if let Some(found) = by_widget {
                return Some(found.clone());
            }
            let owner = self.form().get_field_for_widget(widget)?;
            return self.get_signature(owner.get("ref").unwrap_or(&epdf_kernel::undefined()));
        }
        if !is_field_ref(field) {
            return None;
        }
        let by_name = field.get("kind").and_then(Value::as_str) == Some("fqn");
        signatures
            .iter()
            .find(|signature| {
                if by_name {
                    strict_eq(signature.get("fieldName"), field.get("name"))
                } else {
                    same_field_ref(signature.get("field").unwrap_or(&Value::Null), field)
                }
            })
            .cloned()
    }

    /// `listVerdicts()`: every verdict from the last check; `None` before
    /// the first.
    pub fn list_verdicts(&self) -> Option<Rc<Value>> {
        self.state().verdicts.clone()
    }

    /// `getVerdict(address)`: one signed field's verdict from the last
    /// check, or `None`.
    pub fn get_verdict(&self, field: &Value) -> Option<Value> {
        let signature = self.get_signature(field)?;
        Self::verdict_in(self.state().verdicts.as_deref(), &signature)
    }

    /// `getProtection()`: what the document's signatures allow, or `None`
    /// before they're read.
    pub fn get_protection(&self) -> Option<Value> {
        self.snapshot()?
            .get("protection")
            .filter(|p| !p.is_null() && !epdf_kernel::is_undefined(p))
            .cloned()
    }

    /// `getPending()`: a signature prepared in two steps and not completed.
    pub fn get_pending(&self) -> Option<SignaturePending> {
        let record = self.mirror().get();
        let pending = pending_of(&record)?;
        Some(SignaturePending {
            signing_id: crate::js::template(pending.get("signingId")),
            field: pending.get("field").cloned().unwrap_or_else(epdf_kernel::undefined),
        })
    }

    /// `getPending()` as the mirror holds it.
    pub fn get_pending_json(&self) -> Value {
        pending_of(&self.mirror().get()).cloned().unwrap_or(Value::Null)
    }

    /// `isBusy()`: whether a signing or a fill is running.
    pub fn is_busy(&self) -> bool {
        self.state().busy
    }

    /// `getTarget()`: the field the next picked mark goes to, or `None`.
    pub fn get_target(&self) -> Option<Value> {
        self.state().target.as_deref().cloned()
    }
}
