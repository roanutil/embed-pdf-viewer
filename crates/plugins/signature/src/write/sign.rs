//! `write/sign.ts`: the act of sealing. One-shot: `sign` (prepare, the
//! signer, complete, through `epdf-signature-core`). Two-phase:
//! `prepareSignature` parks the signing and hands out the digest;
//! `completeSignature` seals it with a detached CMS; `abortPending` cancels.

use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_signature_core::SignerPort;
use epdf_signature_core::signers::certificate_common_name;
use serde_json::{Map, Value, json};

use crate::contract::{
    Mark, PrepareSignatureInput, SignFieldInput, SignaturePending, SignatureSignedEvent,
};
use crate::controller::SignatureCapability;
use crate::model::SignatureAction;
use crate::promise::{adopt, await_engine, catch};
use crate::services::events::origin_api;
use crate::services::store::same_ref;
use crate::sync::hydration::ValidateRequest;

/// `isTarget(target, field)`: whether `field` is the sign-here target.
pub fn is_target(target: Option<&Value>, field: &Value) -> bool {
    target.is_some_and(|t| same_ref(t, field))
}

fn appearance_json(pdf: Vec<u8>) -> Value {
    json!({ "pdf": Bytes(pdf).to_json(), "pageIndex": 0 })
}

impl SignatureCapability {
    /// `appearanceOf(input)`: the explicit appearance, else the mark's
    /// bytes, else null.
    fn appearance_of(
        &self,
        mark: Option<&Mark>,
        appearance: Option<&Value>,
    ) -> Promise<Option<Vec<u8>>> {
        let pending = match (appearance, mark) {
            (Some(appearance), _) => Some(self.bytes_of(appearance)),
            (None, Some(mark)) => Some(self.mark_bytes(mark)),
            (None, None) => None,
        };
        Promise::spawn(async move {
            match pending {
                Some(bytes) => Ok(Some(adopt(bytes).await?)),
                None => Ok(None),
            }
        })
    }

    /// `settle(field, result)`: clear the target, re-read, start a
    /// validation and announce the signature.
    fn settle(&self, field: Value, result: Value) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            this.clear_if_target(&field);
            this.refresh().await?;
            let reporting = this.clone();
            catch(this.validate(ValidateRequest::default()), move |error| {
                reporting.console_error("[signature] validation after signing failed:", &error);
            });
            this.0.events.signed.emit(&SignatureSignedEvent {
                field,
                result: result.clone(),
                origin: origin_api(),
            });
            Ok(result)
        })
    }

    /// `sign(input)`: seal the field. The mark's page becomes the widget's
    /// appearance, the signer signs.
    pub fn sign(&self, input: SignFieldInput) -> Promise<Value> {
        let this = self.clone();
        self.with_busy(move || {
            Promise::spawn(async move {
                let doc = this.require_signatures()?;
                let signer = this.resolve_signer(input.signer.clone()).await?;
                // The certificate's subject is the default /Name; the
                // caller's facts win.
                let subject = match &signer {
                    SignerPort::Raw(raw) => match raw.certificate_chain.first() {
                        Some(first) => certificate_common_name(first)?,
                        None => None,
                    },
                    SignerPort::Cms(_) => None,
                };
                let mut attribution = Map::new();
                if let Some(subject) = subject {
                    attribution.insert("name".into(), json!(subject));
                }
                if let Some(Value::Object(given)) = &input.attribution {
                    for (k, v) in given {
                        attribution.insert(k.clone(), v.clone());
                    }
                }
                // The appearance IS the mark: its page is drawn into the
                // widget by the engine.
                let appearance =
                    this.appearance_of(input.mark.as_ref(), input.appearance.as_ref()).await?;
                let mut request = Map::new();
                request.insert("field".into(), input.field.clone());
                request.insert("attribution".into(), Value::Object(attribution));
                if let Some(certify) = &input.certify {
                    request.insert("certify".into(), certify.clone());
                }
                if let Some(lock) = &input.lock {
                    request.insert("lock".into(), lock.clone());
                }
                request.insert(
                    "appearance".into(),
                    match appearance {
                        Some(pdf) => appearance_json(pdf),
                        None => json!({ "pdf": null, "pageIndex": 0 }),
                    },
                );
                let result = rt::returned(epdf_signature_core::sign(doc, request, signer)).await?;
                adopt(this.settle(input.field.clone(), result)).await
            })
        })
    }

    /// `prepareSignature(input)`: step one of two. The digest to sign comes
    /// back and the signing is parked.
    pub fn prepare_signature(&self, input: PrepareSignatureInput) -> Promise<Value> {
        let this = self.clone();
        self.with_busy(move || {
            Promise::spawn(async move {
                let doc = this.require_signatures()?;
                let appearance =
                    this.appearance_of(input.mark.as_ref(), input.appearance.as_ref()).await?;
                let mut request = Map::new();
                request.insert("field".into(), input.field.clone());
                for (key, value) in [
                    ("attribution", &input.attribution),
                    ("certify", &input.certify),
                    ("lock", &input.lock),
                    ("subFilter", &input.sub_filter),
                    ("digest", &input.digest),
                ] {
                    if let Some(value) = value {
                        request.insert(key.into(), value.clone());
                    }
                }
                if let Some(pdf) = appearance {
                    request.insert("appearance".into(), appearance_json(pdf));
                }
                let result = await_engine(doc.call(
                    "signatures.prepare",
                    None,
                    vec![Value::Object(request)],
                ))
                .await?;
                let signing_id =
                    result.get("signingId").and_then(Value::as_str).unwrap_or_default().to_string();
                this.0.prepared.borrow_mut().insert(signing_id.clone(), result.clone());
                this.dispatch(SignatureAction::Pending {
                    pending: Some(SignaturePending { signing_id, field: input.field.clone() }),
                });
                Ok(result)
            })
        })
    }

    /// `completeSignature(signingId, cms)`: step two. The detached CMS
    /// seals the parked signing.
    pub fn complete_signature(&self, signing_id: String, cms: Vec<u8>) -> Promise<Value> {
        let this = self.clone();
        self.with_busy(move || {
            Promise::spawn(async move {
                let doc = this.require_signatures()?;
                let parked = this.0.prepared.borrow().get(&signing_id).cloned();
                let Some(parked) = parked else {
                    return Err(plugin_error(
                        code::NOT_FOUND,
                        "signature",
                        format!("no signing '{signing_id}' was prepared in this session"),
                        None,
                        None,
                    ));
                };
                let result = await_engine(doc
                    .call(
                        "signatures.complete",
                        None,
                        vec![json!({
                            "signingId": signing_id,
                            "cms": Bytes(cms).to_json(),
                            "expectedVersion": parked.get("expectedVersion").cloned().unwrap_or(Value::Null),
                        })],
                    ))
                    .await?;
                this.0.prepared.borrow_mut().remove(&signing_id);
                this.dispatch(SignatureAction::Pending { pending: None });
                let field = result
                    .get("signature")
                    .and_then(|s| s.get("field"))
                    .cloned()
                    .unwrap_or(Value::Null);
                adopt(this.settle(field, result)).await
            })
        })
    }

    /// `abortPending()`: cancel the parked signing, if any.
    pub fn abort_pending(&self) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let Some(pending) = this.state().pending.clone() else { return Ok(()) };
            let doc = this.require_signatures()?;
            await_engine(doc.call("signatures.abort", None, vec![json!(pending.signing_id)]))
                .await?;
            this.0.prepared.borrow_mut().remove(&pending.signing_id);
            this.dispatch(SignatureAction::Pending { pending: None });
            Ok(())
        })
    }
}
