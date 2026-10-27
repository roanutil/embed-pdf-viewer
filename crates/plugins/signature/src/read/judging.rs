//! `read/judging.ts`: the judging verbs: re-reading the facts, validating,
//! and the byte-level questions a verdict rests on (what changed after a
//! signature, the exact revision it covers).

use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::contract::{OperationOptions, SignatureValidateOptions};
use crate::controller::SignatureCapability;
use crate::js::truthy;
use crate::promise::adopt;
use crate::services::errors::verb;

impl SignatureCapability {
    /// `refresh(options)`: read the signatures again; resolves the
    /// snapshot.
    pub fn refresh(&self, options: OperationOptions) -> Promise<Option<Value>> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            this.cancellable(options.signal, this.mirror().refresh()).await?;
            Ok(this.snapshot())
        }))
    }

    /// `validate(options)`: check every signature again and resolve the
    /// verdicts. Fires `onValidated`.
    pub fn validate(&self, options: SignatureValidateOptions) -> Promise<Rc<Value>> {
        verb(self.validate_raw(options))
    }

    /// `validateField(field, options)`: check one signed field. Rejects
    /// `not-found` for an unsigned or unknown field.
    pub fn validate_field(
        &self,
        field: Value,
        options: SignatureValidateOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            this.validate_raw(options).await?;
            this.get_verdict(&field).ok_or_else(|| {
                plugin_error(
                    code::NOT_FOUND,
                    "signature",
                    "no signed field at that address",
                    None,
                    None,
                )
            })
        }))
    }

    /// `analyzeChanges(input, options)`: what changed after a signature or
    /// a version of the file, rule by rule.
    pub fn analyze_changes(&self, input: Value, options: OperationOptions) -> Promise<Value> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            let doc = this.require_signatures()?;
            let call = doc.call("signatures.analyze", None, vec![input]);
            adopt(this.cancellable(options.signal, call)).await
        }))
    }

    /// `readRevision(target, options)`: the exact bytes of the file version
    /// a signature covers, by revision index or by field. Rejects
    /// `permission-denied` without `doc.download`, `not-found` for an
    /// unsigned field.
    pub fn read_revision(&self, target: Value, options: OperationOptions) -> Promise<Value> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            this.cx().assert_allowed("doc.download", "signature.readRevision")?;
            let doc = this.require_signatures()?;
            let revision_index = if target.get("revisionIndex").is_some() {
                target.get("revisionIndex").cloned().unwrap_or_else(epdf_kernel::undefined)
            } else {
                // `!signature?.signed || signature.revisionIndex === null`.
                let signature =
                    this.get_signature(&target).filter(|s| s.get("signed").is_some_and(truthy));
                let revision = signature.as_ref().map(|s| s.get("revisionIndex"));
                match revision {
                    Some(Some(Value::Null)) | None => {
                        return Err(plugin_error(
                            code::NOT_FOUND,
                            "signature",
                            "no signed revision at that address",
                            None,
                            None,
                        ));
                    }
                    Some(revision) => revision.cloned().unwrap_or_else(epdf_kernel::undefined),
                }
            };
            let call = doc.call("signatures.downloadRevision", None, vec![revision_index]);
            adopt(this.cancellable(options.signal, call)).await
        }))
    }
}
