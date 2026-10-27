//! `sync/signatures.ts`: the signature facts, mirrored from the engine:
//! every signature field with its signed state, the protection in force, and
//! the parked two-phase signing. They change only by loads and by confirmed
//! document events, whoever caused them, and this is the one place the fact
//! events fire (`onSigned`, `onProtectionChanged`). The judgement reacts
//! here too: a load re-judges at once when anything is signed; a changed
//! snapshot and every edit of the working copy re-judge once the edits
//! pause.

use std::rc::Rc;

use epdf_kernel::mirror::{Fold, Loaded, Mirror, MirrorSpec};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::{Value, json};

use crate::contract::{SignatureProtectionChangedEvent, SignatureSignedEvent};
use crate::controller::SignatureCapability;
use crate::model::{
    complete_result_of, empty_signature_record, fold_signature_event, has_signed_field, pending_of,
    snapshot_of,
};

/// Edits of the working copy: what a save would write changed, so a
/// verdict may have too.
const WORKING_COPY_EDITS: [&str; 6] = [
    "annotations.created",
    "annotations.updated",
    "annotations.deleted",
    "annotations.reordered",
    "forms.valueSet",
    "forms.updated",
];

/// `JSON.stringify(left) === JSON.stringify(right)`.
fn same_protection(left: &Value, right: &Value) -> bool {
    serde_json::to_string(left).ok() == serde_json::to_string(right).ok()
}

/// `record.snapshot?.protection ?? null`.
fn protection_of(record: &Value) -> Value {
    snapshot_of(record)
        .and_then(|s| s.get("protection"))
        .filter(|p| !p.is_null() && !epdf_kernel::is_undefined(p))
        .cloned()
        .unwrap_or(Value::Null)
}

impl SignatureCapability {
    /// `createSignaturesMirror(ctx, services, validation)`.
    pub(crate) fn create_signatures_mirror(&self) -> Result<Mirror, Error> {
        // The engine's snapshot says nothing about a parked signing: a load
        // keeps the one the events announced, unless the engine disowned it.
        let loading = Rc::downgrade(&self.0);
        let load: epdf_kernel::mirror::Load = Rc::new(move |doc, _signal| {
            let core = loading.clone();
            let call = doc
                .filter(|d| d.has("signatures"))
                .map(|d| d.call("signatures.list", None, vec![]));
            Promise::spawn(async move {
                let snapshot = match call {
                    Some(call) => call.await?,
                    None => Value::Null,
                };
                let pending = core.upgrade().and_then(|core| {
                    let this = SignatureCapability(core);
                    let record = this.mirror().get();
                    let pending = pending_of(&record)?.clone();
                    let id = crate::js::template(pending.get("signingId"));
                    (!this.0.disowned.borrow().contains(&id)).then_some(pending)
                });
                Ok(Loaded {
                    value: json!({ "snapshot": snapshot, "pending": pending.unwrap_or(Value::Null) }),
                    cursor: None,
                })
            })
        });
        let fold: Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>> =
            Rc::new(|record, event| Ok(fold_signature_event(&record, event)));
        let mut spec = MirrorSpec::new("signatures", empty_signature_record(), load, fold);

        let changing = Rc::downgrade(&self.0);
        spec.changed = Some(Rc::new(move |change| {
            let Some(core) = changing.upgrade() else { return Ok(()) };
            let this = SignatureCapability(core);
            let protection = protection_of(&change.next);
            if crate::js::truthy(&protection)
                && !same_protection(&protection_of(&change.previous), &protection)
            {
                this.0
                    .events
                    .protection_changed
                    .emit(&SignatureProtectionChangedEvent { protection });
            }
            let event = change.event.as_ref();
            let completed = event.is_some_and(|e| {
                e.get("type").and_then(Value::as_str) == Some("signatures.completed")
            });
            if let (true, Some(event)) = (completed, event) {
                this.0.events.signed.emit(&SignatureSignedEvent {
                    field: event
                        .get("signature")
                        .and_then(|s| s.get("field"))
                        .cloned()
                        .unwrap_or_else(epdf_kernel::undefined),
                    result: complete_result_of(event),
                    origin: event.get("origin").cloned().unwrap_or_else(epdf_kernel::undefined),
                });
            }
            if !has_signed_field(snapshot_of(&change.next)) {
                return Ok(());
            }
            if change.cause == "load" {
                this.validate_now();
            } else if completed && snapshot_of(&change.previous).is_some() {
                // `previous.snapshot !== next.snapshot`: only a completion
                // over a loaded snapshot makes a new one; a parked or
                // cancelled signing keeps it.
                this.revalidate_soon();
            }
            Ok(())
        }));
        self.cx().mirror(spec)
    }

    /// `disown(signingId)`: the engine does not know this parked signing (its
    /// cancel answered `unknown`), so no event will ever clear it: reload
    /// without it.
    pub(crate) fn disown(&self, signing_id: &str) -> Promise<()> {
        self.0.disowned.borrow_mut().insert(signing_id.to_string());
        self.mirror().refresh()
    }

    /// `connect()`: every edit of the working copy re-judges once the edits
    /// pause, when anything is signed.
    pub(crate) fn connect_signatures(&self) -> Result<(), Error> {
        let doc = self.cx().doc()?;
        let weak = Rc::downgrade(&self.0);
        let unsubscribe: Unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
            let Some(core) = weak.upgrade() else { return };
            let this = SignatureCapability(core);
            let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
            if WORKING_COPY_EDITS.contains(&kind)
                && has_signed_field(snapshot_of(&this.mirror().get()))
            {
                this.revalidate_soon();
            }
        }));
        self.cx().listen(unsubscribe);
        Ok(())
    }
}
