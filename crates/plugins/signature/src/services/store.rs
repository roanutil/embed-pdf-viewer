//! `services/store.ts`: reads of the slice, the busy bracket, and the
//! engine doors every area needs.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::controller::SignatureCapability;
use crate::model::{ResourceStatus, SignatureAction, SignatureState};

/// `sameRef(a, b)`: two object-number refs by field object number, two
/// names by name, and a mixed pair never.
pub fn same_ref(a: &Value, b: &Value) -> bool {
    let kind = |v: &Value| v.get("kind").and_then(Value::as_str).map(str::to_string);
    match (kind(a).as_deref(), kind(b).as_deref()) {
        (Some("objectNumber"), Some("objectNumber")) => {
            number(a.get("fieldObjectNumber")) == number(b.get("fieldObjectNumber"))
        }
        (Some("fqn"), Some("fqn")) => a.get("name") == b.get("name"),
        _ => false,
    }
}

/// A member compared the way `===` compares it: numbers by value.
pub(crate) fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

impl SignatureCapability {
    /// `ctx.getState()`.
    pub(crate) fn state(&self) -> Rc<SignatureState> {
        self.cx().state()
    }

    pub(crate) fn set_status(&self, status: ResourceStatus) {
        self.dispatch(SignatureAction::Status { status });
    }

    /// `withBusy(work)`: busy while `work` runs, whatever it answers.
    pub(crate) fn with_busy<T: Clone + 'static>(
        &self,
        work: impl FnOnce() -> Promise<T> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        Promise::spawn(async move {
            this.dispatch(SignatureAction::Busy { busy: true });
            let outcome = work().await;
            this.dispatch(SignatureAction::Busy { busy: false });
            outcome
        })
    }

    /// `requireDoc()`: `not-ready` without a bound document.
    pub(crate) fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.cx().doc().map_err(|_| {
            plugin_error(code::NOT_READY, "signature", "no document bound", None, None)
        })
    }

    /// `requireSignatures()`: `unsupported` when the engine has no
    /// signatures service.
    pub(crate) fn require_signatures(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.require_doc()?;
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
