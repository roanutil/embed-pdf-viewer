//! `services/store.ts`: the busy bracket and the engine doors every area
//! needs.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;

use crate::controller::SignatureCapability;
use crate::model::set_busy;

impl SignatureCapability {
    /// `withBusy(work)`: busy while `work` runs, whatever it answers.
    pub(crate) fn with_busy<T: Clone + 'static>(
        &self,
        work: impl FnOnce() -> Promise<T> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        Promise::spawn(async move {
            this.update_state(|s| set_busy(s, true));
            let outcome = work().await;
            this.update_state(|s| set_busy(s, false));
            outcome
        })
    }

    /// `requireSignatures()`: the document, which has the signatures service;
    /// `unsupported` without it.
    pub(crate) fn require_signatures(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.cx().doc()?;
        if !doc.has("signatures") {
            return Err(plugin_error(
                code::UNSUPPORTED,
                "signature",
                "this engine does not implement signatures",
                None,
                None,
            ));
        }
        Ok(doc)
    }

    /// `documentId()`: `not-ready` without one.
    pub(crate) fn document_id(&self) -> Result<String, Error> {
        self.cx()
            .document_id()
            .map(str::to_string)
            .ok_or_else(|| plugin_error(code::NOT_READY, "signature", "no document id", None, None))
    }
}
