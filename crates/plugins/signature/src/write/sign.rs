//! `write/sign.ts`: the act of sealing. One-shot: `sign` (prepare, the key,
//! complete, through `epdf-signature-core`). Two-phase: `prepareSignature`
//! parks the signing and hands out the digest; `completeSignature` seals it
//! with a detached CMS; `cancelPending` cancels. The parked signing, the
//! sealed field's facts and `onSigned` come from the confirmed events the
//! engine publishes for each step (`sync/signatures.rs`), for every session
//! alike.

use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::{Answer, CallFacts, DocumentHandle, DocumentSecurity, EngineCall};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use epdf_signature_core::SignerPort;
use epdf_signature_core::signers::certificate_common_name;
use serde_json::{Map, Value, json};

use crate::contract::{Mark, OperationOptions, PrepareSignatureInput, SignFieldInput};
use crate::controller::SignatureCapability;
use crate::js::truthy;
use crate::promise::adopt;
use crate::services::errors::verb;
use crate::sync::validation::import_module;

/// `cancelled(signal)`: `operation-cancelled`, the signal's reason as the
/// cause.
fn cancelled(signal: &AbortSignal) -> Error {
    plugin_error(
        code::OPERATION_CANCELLED,
        "signature",
        "signing cancelled",
        signal.reason_value(),
        None,
    )
}

/// `sealUnlessCancelled(signal)`'s document: the caller's signal checked
/// right before the seal. A signal that fired by then leaves nothing signed
/// (the signing gives the prepared candidate up); after it the seal is
/// done. Every other call goes through as it is.
struct SealUnlessCancelled {
    doc: Rc<dyn DocumentHandle>,
    signal: AbortSignal,
}

impl DocumentHandle for SealUnlessCancelled {
    fn is_local(&self) -> bool {
        self.doc.is_local()
    }
    fn held_object_numbers(&self) -> usize {
        self.doc.held_object_numbers()
    }
    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        self.doc.take_object_number()
    }
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        self.doc.reserve_object_numbers(count)
    }
    fn id(&self) -> String {
        self.doc.id()
    }
    fn capabilities(&self) -> Value {
        self.doc.capabilities()
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.doc.security()
    }
    fn has(&self, member: &str) -> bool {
        self.doc.has(member)
    }
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        if path == "signatures.complete" && self.signal.aborted() {
            return Promise::rejected(cancelled(&self.signal));
        }
        self.doc.call(path, pon, args)
    }
    fn call_with(
        &self,
        facts: &CallFacts,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
    ) -> EngineCall<Value> {
        if path == "signatures.complete" && self.signal.aborted() {
            return Promise::rejected(cancelled(&self.signal));
        }
        self.doc.call_with(facts, path, pon, args)
    }
    fn call_answer(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Answer> {
        if path == "signatures.complete" && self.signal.aborted() {
            return Promise::rejected(cancelled(&self.signal));
        }
        self.doc.call_answer(path, pon, args)
    }
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.doc.page_index(pon)
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.doc.subscribe(listener)
    }
    fn last_server_id(&self) -> Option<String> {
        self.doc.last_server_id()
    }
    fn set_working_set(&self, view: &str, pages: Value) {
        self.doc.set_working_set(view, pages)
    }
}

/// `{ pdf }`, the appearance the engine draws into the widget.
fn appearance_json(pdf: Option<Vec<u8>>) -> Value {
    json!({ "pdf": pdf.map_or(Value::Null, |pdf| Bytes(pdf).to_json()) })
}

impl SignatureCapability {
    /// The document as one-shot signing sees it.
    fn seal_unless_cancelled(
        &self,
        signal: Option<&AbortSignal>,
    ) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.require_signatures()?;
        Ok(match signal {
            None => doc,
            Some(signal) => Rc::new(SealUnlessCancelled { doc, signal: signal.clone() }),
        })
    }

    /// `assertMaySign(input, operation)`: refused before anything starts:
    /// signing the field (`doc.sign`, or `fields:sign` for its group), and
    /// `doc.sign.certify` for a certification.
    fn assert_may_sign(
        &self,
        field: &Value,
        certify: Option<&Value>,
        operation: &str,
    ) -> Result<(), Error> {
        self.assert_allowed_field("sign", field, operation)?;
        if certify.is_some_and(truthy) {
            self.cx().assert_allowed("doc.sign.certify", operation)?;
        }
        Ok(())
    }

    /// `appearanceOf(input)`: an `async` function. The explicit appearance,
    /// else the mark's bytes, else `null`.
    fn appearance_of(
        &self,
        mark: Option<&Mark>,
        appearance: Option<&Value>,
    ) -> Promise<Option<Vec<u8>>> {
        let pending = match (appearance.filter(|a| truthy(a)), mark) {
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

    /// `sign(input, options)`, unwrapped: the placement rule calls it as it
    /// is.
    pub(crate) fn sign_raw(
        &self,
        input: SignFieldInput,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_may_sign(&input.field, input.certify.as_ref(), "signature.sign")?;
            if let Some(signal) = options.signal.as_ref().filter(|s| s.aborted()) {
                return Err(cancelled(signal));
            }
            let busy = this.clone();
            adopt(this.with_busy(move || {
                Promise::spawn(async move {
                    let doc = busy.seal_unless_cancelled(options.signal.as_ref())?;
                    // The cryptography loads the first time someone signs.
                    import_module().await;
                    let key = busy.resolve_key(input.key.clone()).await?;
                    // The certificate's subject is the default /Name; the
                    // caller's facts win.
                    let subject = match &key {
                        SignerPort::Raw(raw) => match raw.certificate_chain.first() {
                            Some(first) => certificate_common_name(first)?,
                            None => None,
                        },
                        SignerPort::Cms(_) => None,
                    };
                    let mut signer = Map::new();
                    if let Some(subject) = subject.filter(|s| !s.is_empty()) {
                        signer.insert("name".into(), json!(subject));
                    }
                    if let Some(Value::Object(given)) = &input.signer {
                        for (k, v) in given {
                            signer.insert(k.clone(), v.clone());
                        }
                    }
                    // The appearance is the mark: its page is drawn into the
                    // widget by the engine.
                    let appearance =
                        busy.appearance_of(input.mark.as_ref(), input.appearance.as_ref()).await?;
                    let mut request = Map::new();
                    request.insert("field".into(), input.field.clone());
                    request.insert("signer".into(), Value::Object(signer));
                    if let Some(certify) = &input.certify {
                        request.insert("certify".into(), certify.clone());
                    }
                    if let Some(lock) = &input.lock {
                        request.insert("lock".into(), lock.clone());
                    }
                    request.insert("appearance".into(), appearance_json(appearance));
                    let result = rt::returned(epdf_signature_core::sign(doc, request, key)).await?;
                    busy.clear_if_target(&input.field);
                    Ok(result)
                })
            }))
            .await
        })
    }

    /// `sign(input, options)`: sign a field. The mark becomes what it
    /// shows, the key seals the document, and the signed document is a new
    /// version. A signal that fires before the seal leaves nothing signed.
    pub fn sign(&self, input: SignFieldInput, options: OperationOptions) -> Promise<Value> {
        verb(self.sign_raw(input, options))
    }

    /// `prepareSignature(input, options)`: signing in two steps, the first.
    /// Resolves the digest for your key to sign, and keeps the document from
    /// changing until `completeSignature()` or `cancelPending()`.
    pub fn prepare_signature(
        &self,
        input: PrepareSignatureInput,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            this.assert_may_sign(
                &input.field,
                input.certify.as_ref(),
                "signature.prepareSignature",
            )?;
            let busy = this.clone();
            adopt(this.with_busy(move || {
                Promise::spawn(async move {
                    let doc = busy.require_signatures()?;
                    let appearance =
                        busy.appearance_of(input.mark.as_ref(), input.appearance.as_ref()).await?;
                    let mut request = Map::new();
                    request.insert("field".into(), input.field.clone());
                    for (key, value) in [
                        ("signer", &input.signer),
                        ("certify", &input.certify),
                        ("lock", &input.lock),
                        ("subFilter", &input.sub_filter),
                        ("digest", &input.digest),
                    ] {
                        if let Some(value) = value {
                            request.insert(key.into(), value.clone());
                        }
                    }
                    if appearance.is_some() {
                        request.insert("appearance".into(), appearance_json(appearance));
                    }
                    let pending =
                        doc.call("signatures.prepare", None, vec![Value::Object(request)]);
                    let result = busy.cancellable(options.signal, pending).await?;
                    let signing_id = crate::js::template(result.get("signingId"));
                    busy.0.prepared.borrow_mut().insert(signing_id, result.clone());
                    Ok(result)
                })
            }))
            .await
        }))
    }

    /// `completeSignature(signingId, cms, options)`: signing in two steps,
    /// the second: the key's CMS seals the document. Rejects `not-found` for
    /// a signing this session didn't prepare.
    pub fn complete_signature(
        &self,
        signing_id: String,
        cms: Vec<u8>,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            // Its field was checked when it was prepared; the engine checks
            // it again.
            if let Some(signal) = options.signal.as_ref().filter(|s| s.aborted()) {
                return Err(cancelled(signal));
            }
            let busy = this.clone();
            adopt(this.with_busy(move || {
                Promise::spawn(async move {
                    let doc = busy.require_signatures()?;
                    let parked = busy.0.prepared.borrow().get(&signing_id).cloned();
                    let Some(parked) = parked else {
                        return Err(plugin_error(
                            code::NOT_FOUND,
                            "signature",
                            format!("no signing '{signing_id}' was prepared in this session"),
                            None,
                            None,
                        ));
                    };
                    let mut request = Map::new();
                    request.insert("signingId".into(), json!(signing_id));
                    request.insert("cms".into(), Bytes(cms).to_json());
                    if let Some(expected) = parked.get("expectedVersion") {
                        request.insert("expectedVersion".into(), expected.clone());
                    }
                    let result =
                        doc.call("signatures.complete", None, vec![Value::Object(request)]).await?;
                    busy.0.prepared.borrow_mut().remove(&signing_id);
                    let field = result
                        .get("signature")
                        .and_then(|s| s.get("field"))
                        .cloned()
                        .unwrap_or_else(epdf_kernel::undefined);
                    busy.clear_if_target(&field);
                    Ok(result)
                })
            }))
            .await
        }))
    }

    /// `cancelPending(options)`: give up a signature prepared in two steps,
    /// so the document can be changed again. A cancel or a completion clears
    /// the signing through its event; one the engine doesn't know has none,
    /// so the mirror reloads without it.
    pub fn cancel_pending(&self, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            let Some(pending) = this.get_pending() else { return Ok(()) };
            this.assert_allowed_field("sign", &pending.field, "signature.cancelPending")?;
            let doc = this.require_signatures()?;
            let call = doc.call("signatures.cancel", None, vec![json!(pending.signing_id)]);
            let answer = this.cancellable(options.signal, call).await?;
            this.0.prepared.borrow_mut().remove(&pending.signing_id);
            if answer.get("status").and_then(Value::as_str) == Some("unknown") {
                this.disown(&pending.signing_id).await?;
            }
            Ok(())
        }))
    }
}
