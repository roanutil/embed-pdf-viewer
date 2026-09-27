//! `sync/hydration.ts`: re-reading the snapshot and judging the signatures.
//! The viewer judges the working copy by default (unsaved edits count); an
//! edit that turns a held signature invalid warns once, on the edge.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_signature_core::{ValidateOptions, ValidationTime, validate_signatures};
use serde_json::{Value, json};

use crate::contract::{
    SignatureInvalidatingEvent, SignatureProtectionChangedEvent, SignatureValidatedEvent,
};
use crate::controller::SignatureCapability;
use crate::model::{ResourceStatus, SignatureAction};
use crate::promise::await_engine;
use crate::read::signatures::{any_signed, truthy};
use crate::services::store::number;

/// `validate`'s options: `at` and `until` (`persisted` or `working-copy`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidateRequest {
    pub at: Option<ValidationTime>,
    pub until: Option<String>,
}

/// `JSON.stringify(a) === JSON.stringify(b)`.
fn same_protection(a: &Value, b: &Value) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

impl SignatureCapability {
    /// `refresh()`: re-read the snapshot. Null (and a null snapshot) when
    /// the engine has no signatures service.
    pub fn refresh(&self) -> Promise<Option<Value>> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.cx().doc()?;
            if !doc.has("signatures") {
                this.dispatch(SignatureAction::Snapshot { snapshot: None });
                return Ok(None);
            }
            let state = this.state();
            let before = state
                .snapshot
                .as_ref()
                .map(|s| s.get("protection").cloned().unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            if state.snapshot.is_none() {
                this.set_status(ResourceStatus::Loading);
            }
            let snapshot = match await_engine(doc.call("signatures.list", None, Vec::new())).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    this.set_status(ResourceStatus::Error);
                    return Err(error);
                }
            };
            this.dispatch(SignatureAction::Snapshot { snapshot: Some(snapshot.clone()) });
            this.set_status(ResourceStatus::Ready);
            let protection = snapshot.get("protection").cloned().unwrap_or(Value::Null);
            if !same_protection(&before, &protection) {
                this.0
                    .events
                    .protection_changed
                    .emit(&SignatureProtectionChangedEvent { protection });
            }
            Ok(Some(snapshot))
        })
    }

    /// `validate(options)`: judge every signature, the working copy unless
    /// `until` says `persisted`.
    pub fn validate(&self, request: ValidateRequest) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.cx().doc()?;
            if !doc.has("signatures") {
                return Ok(json!([]));
            }
            let options = ValidateOptions {
                trust: this.config().trust.clone(),
                at: request.at,
                until: Some(request.until.unwrap_or_else(|| "working-copy".into())),
            };
            let verdicts = rt::returned(validate_signatures(doc, options)).await?;
            let before = this.state().verdicts.clone();
            this.dispatch(SignatureAction::Verdicts { verdicts: Some(verdicts.clone()) });
            this.0.events.validated.emit(&SignatureValidatedEvent { verdicts: verdicts.clone() });
            // Acrobat's warning, after the fact and only on the edge: a
            // signature that held (or was never judged) now reads invalid
            // because of unsaved edits.
            for v in verdicts.as_array().into_iter().flatten() {
                let modifications = v.get("modifications").cloned().unwrap_or(Value::Null);
                if modifications.get("basis").and_then(Value::as_str) != Some("working-copy")
                    || v.get("summary").and_then(Value::as_str) != Some("invalid")
                {
                    continue;
                }
                let index = number(v.get("signature").and_then(|s| s.get("index")));
                let was = before.as_ref().and_then(Value::as_array).and_then(|b| {
                    b.iter()
                        .find(|b| number(b.get("signature").and_then(|s| s.get("index"))) == index)
                });
                if was.is_some_and(|w| w.get("summary").and_then(Value::as_str) == Some("invalid"))
                {
                    continue;
                }
                let detail = modifications
                    .get("detail")
                    .filter(|d| !d.is_null() && !epdf_kernel::is_undefined(d))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let field =
                    v.get("signature").and_then(|s| s.get("field")).cloned().unwrap_or(Value::Null);
                this.0.events.invalidating.emit(&SignatureInvalidatingEvent { field, detail });
            }
            Ok(verdicts)
        })
    }

    /// `validateField(address, options)`: judge, then answer one field's
    /// verdict; `not-found` for an unsigned or unknown field.
    pub fn validate_field(&self, field: Value, request: ValidateRequest) -> Promise<Value> {
        let this = self.clone();
        let validating = self.validate(request);
        Promise::spawn(async move {
            validating.await?;
            this.get_verdict(&field).ok_or_else(|| {
                plugin_error(
                    code::NOT_FOUND,
                    "signature",
                    "no signed field at that address",
                    None,
                    None,
                )
            })
        })
    }

    /// `revalidateSoon()`: every edit that could count as a modification
    /// re-judges the working copy, coalesced into one validation 300 ms
    /// after the last.
    pub(crate) fn revalidate_soon(&self) {
        if !any_signed(self.state().snapshot.as_ref()) {
            return;
        }
        if let Some(timer) = self.0.revalidate_timer.take() {
            rt::clear_timer(timer);
        }
        let this = self.clone();
        let timer = rt::set_timeout(300.0, move || {
            this.0.revalidate_timer.set(None);
            let reporting = this.clone();
            crate::promise::catch(this.validate(ValidateRequest::default()), move |error| {
                reporting.console_error("[signature] re-validation failed:", &error);
            });
        });
        self.0.revalidate_timer.set(Some(timer));
    }

    /// `analyzeChanges(input)`: what changed after a signature. `Err` is the
    /// synchronous `unsupported` throw.
    pub fn analyze_changes(&self, input: Value) -> Result<Promise<Value>, Error> {
        Ok(self.require_signatures()?.call("signatures.analyze", None, vec![input]))
    }

    /// `readRevision(target)`: the bytes a signature's revision covers, by
    /// revision index or by field. `Err` is the synchronous `unsupported`
    /// throw.
    pub fn read_revision(&self, target: &Value) -> Result<Promise<Value>, Error> {
        let doc = self.require_signatures()?;
        if let Some(index) = target.get("revisionIndex") {
            return Ok(doc.call("signatures.revisionBytes", None, vec![index.clone()]));
        }
        let signature = self.get_signature(target);
        let revision = signature
            .as_ref()
            .filter(|s| s.get("signed").is_some_and(truthy))
            .and_then(|s| s.get("revisionIndex"))
            .filter(|r| !r.is_null());
        match revision {
            Some(revision) => {
                Ok(doc.call("signatures.revisionBytes", None, vec![revision.clone()]))
            }
            None => Ok(Promise::rejected(plugin_error(
                code::NOT_FOUND,
                "signature",
                "no signed revision at that address",
                None,
                None,
            ))),
        }
    }
}
