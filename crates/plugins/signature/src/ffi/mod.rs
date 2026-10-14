//! The signature plugin through uniffi: typed records decoded from the
//! plugin's JSON ([`records`]), one handle, one event enum. Every method
//! hops to the kernel thread through `KernelBound`.

pub mod records;

use std::sync::Arc;

use epdf_ffi::{
    CancelToken, FormFieldRef, KernelBound, KernelPost, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Value, json};

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

fn status(s: crate::ResourceStatus) -> ResourceStatus {
    match s {
        crate::ResourceStatus::Idle => ResourceStatus::Idle,
        crate::ResourceStatus::Loading => ResourceStatus::Loading,
        crate::ResourceStatus::Ready => ResourceStatus::Ready,
        crate::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
        crate::ResourceStatus::Error => ResourceStatus::Error,
    }
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
        self.cap.call(|c| SignatureInfo::list_from_json(&open(c)?.list_signatures())).await
    }

    /// The signature fields nobody has signed.
    pub async fn unsigned_fields(&self) -> Result<Vec<SignatureInfo>, ViewerError> {
        self.cap
            .call(|c| {
                let cap = open(c)?;
                let fields = cap.list_unsigned_fields();
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
                open(c)?.list_verdicts().as_ref().map(SignatureVerdict::list_from_json).transpose()
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
                |c| Ok(open(c)?.refresh()),
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
                move |c| Ok(open(c)?.validate(request.into())),
                |v: Value| SignatureVerdict::list_from_json(&v).map_err(ViewerError::from),
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
                move |c| Ok(open(c)?.validate_field(field_json(&field), request.into())),
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
                    open(c)?.analyze_changes(input)
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
                move |c| open(c)?.read_revision(&field_json(&field)),
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

    /// `doc.sign` is granted and a signer is configured.
    pub async fn can_sign(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_sign())).await
    }

    /// `doc.forms.fill` is granted and the engine draws into signature fields.
    pub async fn can_fill(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_fill())).await
    }

    /// Certification is allowed by the config and `doc.sign.certify`.
    pub async fn can_certify(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_certify())).await
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
