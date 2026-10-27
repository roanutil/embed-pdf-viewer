//! `sync/validation.ts`: judging the signatures. The viewer judges the
//! working copy by default (unsaved edits count), so the verdict is the one
//! the file a save produces will get. Verdicts are session state: each
//! validation commits its own, and `onValidated` fires where one completes.
//! Re-judging after an edit waits for a pause: a pen stroke is many events,
//! one analysis.

use std::rc::Rc;

use epdf_kernel::rt::{self, Promise};
use epdf_signature_core::{ValidateOptions, validate_signatures};
use serde_json::Value;

use crate::contract::{SignatureValidateOptions, SignatureValidatedEvent};
use crate::controller::SignatureCapability;
use crate::model::set_verdicts;

const REVALIDATE_DELAY_MS: f64 = 300.0;

/// `await import('@embedpdf/core-signature')` of the already loaded module,
/// as one turn. Node 24.20.0 resumes it eight turns after the call; the
/// vectors pass at one and at two, so they don't observe it (README, "Turn
/// order").
pub(crate) async fn import_module() {
    rt::tick().await;
}

impl SignatureCapability {
    /// `validate(options)`: judge every signature, the working copy unless
    /// `until` says `persisted`; `[]` without the engine's signatures
    /// service. Commits the verdicts and fires `onValidated`.
    pub(crate) fn validate_raw(&self, options: SignatureValidateOptions) -> Promise<Rc<Value>> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.cx().doc()?;
            if !doc.has("signatures") {
                return Ok(Rc::new(Value::Array(Vec::new())));
            }
            // The cryptography loads the first time a signature is checked.
            let signal = options.signal.clone();
            this.cancellable(
                signal.clone(),
                Promise::spawn(async {
                    import_module().await;
                    Ok(())
                }),
            )
            .await?;
            let run = ValidateOptions {
                trust: this.settings().trust,
                at: options.at.clone(),
                until: Some(options.until.clone().unwrap_or_else(|| "working-copy".into())),
            };
            let judging = Promise::spawn(validate_signatures(doc, run));
            let verdicts = Rc::new(this.cancellable(signal, judging).await?);
            this.update_state(|s| set_verdicts(s, Some(verdicts.clone())));
            this.0
                .events
                .validated
                .emit(&SignatureValidatedEvent { verdicts: (*verdicts).clone() });
            Ok(verdicts)
        })
    }

    fn cancel_scheduled(&self) {
        if let Some(cancel) = self.0.cancel_revalidate.borrow_mut().take() {
            cancel.call();
        }
    }

    fn validate_in_background(&self) {
        crate::promise::catch(self.validate_raw(SignatureValidateOptions::default()), |error| {
            crate::console::error("[signature] validation failed:", &error);
        });
    }

    /// `validateNow()`: judge now; a scheduled re-judgement is superseded.
    pub(crate) fn validate_now(&self) {
        self.cancel_scheduled();
        self.validate_in_background();
    }

    /// `revalidateSoon()`: judge once the edits pause.
    pub(crate) fn revalidate_soon(&self) {
        self.cancel_scheduled();
        let weak = Rc::downgrade(&self.0);
        let cancel = self.cx().clock().after(REVALIDATE_DELAY_MS, move || {
            let Some(core) = weak.upgrade() else { return };
            let this = SignatureCapability(core);
            this.0.cancel_revalidate.borrow_mut().take();
            this.validate_in_background();
        });
        *self.0.cancel_revalidate.borrow_mut() = Some(cancel);
    }
}
