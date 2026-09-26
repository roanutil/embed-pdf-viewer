//! `services/catalog.ts`: the one catalog-actions read. The catalog /AA is
//! immutable in-session, so the open sequence, the boot sources and the five
//! document events share one memoized read; a rejected read is evicted so a
//! transient failure cannot poison every later lifecycle event.

use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::DocumentTriggerEvent;

/// `DOC_EVENT_TREES`: the Table-200 key for each verb-shaped document event
/// (`open` reads `openAction` instead).
pub(crate) fn doc_event_tree_key(event: DocumentTriggerEvent) -> &'static str {
    match event {
        DocumentTriggerEvent::WillSave => "willSave",
        DocumentTriggerEvent::DidSave => "didSave",
        DocumentTriggerEvent::WillPrint => "willPrint",
        DocumentTriggerEvent::DidPrint => "didPrint",
        DocumentTriggerEvent::WillClose => "willClose",
        DocumentTriggerEvent::Open => "openAction",
    }
}

impl ActionsCapability {
    /// `readDocumentActions()`.
    pub(crate) fn read_document_actions(&self) -> Promise<Option<Value>> {
        if let Some(memo) = self.0.catalog.borrow().clone() {
            return memo;
        }
        // `Promise.resolve(doc.actions.read())` is the guarded call itself.
        let read: Option<Promise<Value>> = match self.doc() {
            Ok(doc) if doc.has("actions") => Some(doc.call("actions.read", None, Vec::new())),
            _ => None,
        };
        // `read.catch(...)`: one turn after the read settles.
        let (memo, resolver) = Promise::pending();
        let (weak, own) = (self.weak(), memo.clone());
        rt::spawn(async move {
            let result = match read {
                Some(call) => call.await.map(Some),
                None => Promise::<Option<Value>>::resolved(None).await,
            };
            if result.is_err()
                && let Some(this) = ActionsCapability::upgrade(&weak)
            {
                let current = this.0.catalog.borrow().as_ref().is_some_and(|m| m.same(&own));
                if current {
                    *this.0.catalog.borrow_mut() = None;
                }
            }
            resolver.settle(result);
        });
        *self.0.catalog.borrow_mut() = Some(memo.clone());
        memo
    }
}
