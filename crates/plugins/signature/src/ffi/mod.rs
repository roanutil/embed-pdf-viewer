//! The signature plugin through uniffi: typed records decoded from the
//! plugin's JSON ([`records`]), one handle, one event enum. Every method
//! hops to the kernel thread through `KernelBound`.
//!
//! Migrated by hand to the Task 13.25 plugin (settings, per-field authority,
//! `cancelPending`, verbs taking `OperationOptions`) and not compiled until
//! `epdf-ffi` builds: Task 13.33 brings it, `epdf-viewer-host` and Swift up
//! together.

pub mod ports;
pub mod records;

use std::sync::Arc;

use epdf_ffi::{
    CancelToken, FormFieldRef, KernelBound, KernelPost, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::mirror::ResourceStatus as CoreStatus;
use serde_json::{Value, json};

pub use ports::{
    SignatureDigest, SignatureKeyAlgorithm, SignatureSignCompletion, SignatureSigner,
    SignatureTrust,
};
pub use records::*;

use crate::SignatureCapability;

fn open(cap: &SignatureCapability) -> Result<&SignatureCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "signature",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn status(s: CoreStatus) -> ResourceStatus {
    match s {
        CoreStatus::Idle => ResourceStatus::Idle,
        CoreStatus::Loading => ResourceStatus::Loading,
        CoreStatus::Ready => ResourceStatus::Ready,
        CoreStatus::Forbidden => ResourceStatus::Forbidden,
        CoreStatus::Error => ResourceStatus::Error,
    }
}

/// `OperationOptions` without a signal: a `CancelToken` aborts the verb's
/// promise itself (`KernelBound::call_promise_cancellable`).
fn none() -> crate::OperationOptions {
    crate::OperationOptions::default()
}

/// One document's signature fields: reading, validating, signing and
/// filling them.
#[derive(uniffi::Object)]
pub struct SignatureHandle {
    pub(crate) cap: KernelBound<SignatureCapability>,
    pub(crate) post: KernelPost,
}

impl SignatureHandle {
    /// On the kernel thread.
    pub fn new(cap: SignatureCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(SignatureHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

/// The plugin's settings a host registers. The signer and the trust anchors
/// are set on the host (`ViewerHost::set_signature_signer`,
/// `set_signature_trust`).
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SignatureConfig {
    /// `None` is `Sign` while the `key` setting is set, else `Visual`. The
    /// host's key is a function over its signer slot, so with a host plugin
    /// the default is `Sign`; a host that wants `Visual` until a signer is
    /// set registers the mode, or updates it when the slot fills.
    #[uniffi(default = None)]
    pub mode: Option<SignatureMode>,
    /// Let the UI offer a certification. `None` is false.
    #[uniffi(default = None)]
    pub allow_certify: Option<bool>,
}

/// `signaturePlugin(config)` for a host: the `key` setting is a function
/// over `signer` (resolved per signing, `invalid-input` while empty) and the
/// `trust` setting a port over `trust` (no anchors while empty).
pub fn host_plugin(
    config: SignatureConfig,
    signer: ports::SignatureSignerSlot,
    trust: ports::SignatureTrustSlot,
    post: KernelPost,
) -> epdf_kernel::AnyPlugin {
    let slot = signer.clone();
    let resolve: std::rc::Rc<
        dyn Fn() -> epdf_kernel::rt::Promise<epdf_signature_core::SignerPort>,
    > = std::rc::Rc::new(move || {
        let current = slot.lock().unwrap().clone();
        match current {
            Some(signer) => match ports::signer_port(signer, post.clone()) {
                Ok(port) => epdf_kernel::rt::Promise::resolved(port),
                Err(error) => epdf_kernel::rt::Promise::rejected(error),
            },
            None => epdf_kernel::rt::Promise::rejected(plugin_error(
                code::INVALID_INPUT,
                "signature",
                "no signer: set one on the viewer first",
                None,
                None,
            )),
        }
    });
    crate::signature_plugin(crate::SignatureConfig {
        mode: config.mode.map(|mode| Some(mode.into())),
        key: Some(Some(crate::SignatureKey::Thunk(resolve))),
        trust: Some(Some(std::rc::Rc::new(ports::SlotTrust(trust)))),
        allow_certify: config.allow_certify,
    })
}

#[uniffi::export]
impl SignatureHandle {
    /// The last snapshot read, `None` before the first.
    pub async fn snapshot(&self) -> Result<Option<SignatureSnapshot>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.get_snapshot().as_ref().map(SignatureSnapshot::from_json).transpose()
            })
            .await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|c| Ok(status(open(c)?.get_status()))).await
    }

    /// The signed fields.
    pub async fn signatures(&self) -> Result<Vec<SignatureInfo>, ViewerError> {
        self.cap
            .call(|c| {
                SignatureInfo::list_from_json(&Value::Array((*open(c)?.list_signatures()).clone()))
            })
            .await
    }

    /// The signature fields nobody has signed.
    pub async fn unsigned_fields(&self) -> Result<Vec<SignatureInfo>, ViewerError> {
        self.cap
            .call(|c| {
                let cap = open(c)?;
                let fields = cap.list_unsigned_fields();
                let fields = fields.as_slice();
                fields
                    .iter()
                    .map(|f| {
                        cap.get_signature(f)
                            .ok_or_else(|| invalid_state("an unsigned field", f))
                            .and_then(|s| SignatureInfo::from_json(&s))
                    })
                    .collect()
            })
            .await
    }

    /// One signature field, by its ref.
    pub async fn signature(
        &self,
        field: FormFieldRef,
    ) -> Result<Option<SignatureInfo>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_signature(&field_json(&field))
                    .as_ref()
                    .map(SignatureInfo::from_json)
                    .transpose()
            })
            .await
    }

    /// The last verdicts, `None` before the first validation.
    pub async fn verdicts(&self) -> Result<Option<Vec<SignatureVerdict>>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?
                    .list_verdicts()
                    .as_deref()
                    .map(SignatureVerdict::list_from_json)
                    .transpose()
            })
            .await
    }

    pub async fn verdict(
        &self,
        field: FormFieldRef,
    ) -> Result<Option<SignatureVerdict>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_verdict(&field_json(&field))
                    .as_ref()
                    .map(SignatureVerdict::from_json)
                    .transpose()
            })
            .await
    }

    pub async fn protection(&self) -> Result<Option<DocumentProtection>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.get_protection().as_ref().map(DocumentProtection::from_json).transpose()
            })
            .await
    }

    /// A parked two-phase signing: the document is read-only until it
    /// completes or is aborted.
    pub async fn pending(&self) -> Result<Option<SignaturePending>, ViewerError> {
        self.cap.call(|c| open(c)?.get_pending().map(SignaturePending::try_from).transpose()).await
    }

    /// A sign or fill is in flight.
    pub async fn is_busy(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_busy())).await
    }

    /// What placing a mark on a field does.
    pub async fn mode(&self) -> Result<SignatureMode, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_mode().into())).await
    }

    /// The field the next picked mark goes to.
    pub async fn target(&self) -> Result<Option<FormFieldRef>, ViewerError> {
        self.cap.call(|c| open(c)?.get_target().as_ref().map(field_from_json).transpose()).await
    }

    /// Re-read the snapshot. `None` when the engine has no signatures.
    pub async fn refresh(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<SignatureSnapshot>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.refresh(none())),
                |s: Option<Value>| {
                    s.as_ref()
                        .map(SignatureSnapshot::from_json)
                        .transpose()
                        .map_err(ViewerError::from)
                },
            )
            .await?
    }

    /// Judge every signature. Without trust anchors a good one tops out at
    /// `ValidUntrusted`.
    pub async fn validate(
        &self,
        request: SignatureValidateRequest,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<SignatureVerdict>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.validate(request.into_core(None))),
                |v: std::rc::Rc<Value>| {
                    SignatureVerdict::list_from_json(&v).map_err(ViewerError::from)
                },
            )
            .await?
    }

    /// Judge one signature.
    pub async fn validate_field(
        &self,
        field: FormFieldRef,
        request: SignatureValidateRequest,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureVerdict, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.validate_field(field_json(&field), request.into_core(None))),
                |v: Value| SignatureVerdict::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// What changed after signature `since` (its index), up to the saved
    /// bytes or the working copy.
    pub async fn analyze_changes(
        &self,
        since: u32,
        until: SignatureModificationsBasis,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureChangeAnalysis, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input =
                        json!({ "since": { "signatureIndex": since }, "until": until.as_str() });
                    Ok(open(c)?.analyze_changes(input, none()))
                },
                |v: Value| SignatureChangeAnalysis::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// The bytes the signature on `field` seals: its revision, a complete
    /// PDF of its own.
    pub async fn read_revision(
        &self,
        field: FormFieldRef,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.read_revision(field_json(&field), none())),
                |v: Value| {
                    Bytes::from_json(&v)
                        .map(|b| b.0)
                        .ok_or_else(|| ViewerError::from(invalid_state("revision bytes", &v)))
                },
            )
            .await?
    }

    /// Name the field the next picked mark goes to; `None` clears.
    pub async fn set_target(&self, field: Option<FormFieldRef>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.set_target(field.as_ref().map(field_json));
                Ok(())
            })
            .await
    }

    /// Ask the app to show a signed field's facts (`InspectionRequested`).
    pub async fn request_inspection(&self, field: FormFieldRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.request_inspection(field_json(&field));
                Ok(())
            })
            .await
    }

    /// Seal `input.field` with `signer`, or the `key` setting (the signer
    /// the viewer has). Fails with the signer's own error as the cause of an
    /// `operation-failed` if it throws, and leaves nothing pending.
    pub async fn sign(
        &self,
        input: SignatureSignInput,
        signer: Option<Arc<dyn SignatureSigner>>,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureCompletion, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let key = signer
                        .map(|signer| ports::signer_port(signer, post))
                        .transpose()?
                        .map(crate::SignatureKey::port);
                    Ok(open(c)?.sign(input.into_core(key), none()))
                },
                |v: Value| SignatureCompletion::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Phase one of an external signing: the digest to sign elsewhere. The
    /// document is read-only until `complete_signature` or `cancel_pending`.
    pub async fn prepare_signature(
        &self,
        input: SignaturePrepareInput,
        cancel: Arc<CancelToken>,
    ) -> Result<SignaturePrepared, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.prepare_signature(input.into_core(), none())),
                |v: Value| SignaturePrepared::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Phase two: seal the parked signing with a detached CMS.
    pub async fn complete_signature(
        &self,
        signing_id: String,
        cms: Vec<u8>,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureCompletion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.complete_signature(signing_id, cms, none())),
                |v: Value| SignatureCompletion::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Give up the parked signing, if any (`cancelPending`).
    pub async fn cancel_pending(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.cancel_pending(none())), |()| ())
            .await
    }

    /// Draw `mark` into an unsigned field without sealing it.
    pub async fn fill_field(
        &self,
        field: FormFieldRef,
        mark: SignatureMark,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.fill_field(field_json(&field), mark.into(), none())),
                |()| (),
            )
            .await
    }

    /// Undo a visual fill.
    pub async fn clear_field(
        &self,
        field: FormFieldRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.clear_field(field_json(&field), none())),
                |()| (),
            )
            .await
    }

    /// What placing `mark` does follows `mode`: sign, fill, or ask. A free
    /// placement is a stamp.
    pub async fn place_mark(
        &self,
        mark: SignatureMark,
        target: SignaturePlaceTarget,
        cancel: Arc<CancelToken>,
    ) -> Result<SignaturePlaceResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.place_mark(mark.into(), target.try_into()?, none())),
                |r: crate::PlaceMarkResult| {
                    SignaturePlaceResult::try_from(r).map_err(ViewerError::from)
                },
            )
            .await?
    }

    /// Whether signing `field` is allowed: `doc.sign`, or `fields:sign`
    /// for its group. `false` for a field the form doesn't have.
    pub async fn can_sign(&self, field: FormFieldRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_sign(&field_json(&field)))).await
    }

    /// Whether drawing a mark into `field` is allowed: `doc.forms.fill`, or
    /// `fields:fill` for its group.
    pub async fn can_fill(&self, field: FormFieldRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_fill(&field_json(&field)))).await
    }

    /// Certification is allowed by the `allowCertify` setting and
    /// `doc.sign.certify`.
    pub async fn can_certify(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_certify())).await
    }

    /// The signed bytes may be read: `doc.download`.
    pub async fn can_read_revision(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read_revision())).await
    }

    /// The nine change hooks as one stream, ended when the document closes.
    pub fn subscribe(&self, listener: Arc<dyn SignatureEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || register_hooks(c, listener),
                move || ending.on_end(),
            );
        });
        subscription
    }
}
