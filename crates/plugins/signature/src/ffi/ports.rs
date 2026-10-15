//! The host ports, implemented in Swift: a key that signs, and the anchors
//! validation trusts. Both are called on the kernel thread. `sign` answers
//! later through a completion, from any thread, because a Keychain key can
//! raise a Face ID or passcode prompt and the kernel thread must not wait on
//! it (`epdf_ffi::foreign`); the rest answer at once.
//!
//! The synchronous members throw `ViewerError` rather than the shared
//! `HostError`, and the completion fails with a `HostFailure`, so a signer
//! can say why it failed in the kernel's codes: a cancelled biometric prompt
//! is `operation-cancelled`, and `sign` fails with exactly that.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{ForeignSettle, HostFailure, KernelPost, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_signature_core::cms::{RawAlgorithm, RawSigner};
use epdf_signature_core::oids::Digest;
use epdf_signature_core::{SignerPort, TrustPort};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureKeyAlgorithm {
    RsaPkcs1,
    RsaPss,
    Ecdsa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureDigest {
    Sha256,
    Sha384,
    Sha512,
}

impl SignatureDigest {
    pub(crate) fn name(self) -> &'static str {
        match self {
            SignatureDigest::Sha256 => "sha256",
            SignatureDigest::Sha384 => "sha384",
            SignatureDigest::Sha512 => "sha512",
        }
    }
}

/// A private key that never leaves the host, and its certificate chain
/// (leaf first, DER). `sign` gets the DER of the CMS signed attributes and
/// hashes it itself with `digest`. An ECDSA key answers `r || s`, each
/// padded to the curve's size, not X9.62 DER.
///
/// Every member is called on the kernel thread. `sign` must return at once
/// and answer through `completion`, from any thread: the kernel keeps
/// serving other calls while a signature is pending.
#[uniffi::export(with_foreign)]
pub trait SignatureSigner: Send + Sync {
    fn certificate_chain(&self) -> Result<Vec<Vec<u8>>, ViewerError>;
    fn algorithm(&self) -> SignatureKeyAlgorithm;
    fn digest(&self) -> SignatureDigest;
    fn sign(&self, message: Vec<u8>, completion: Arc<SignatureSignCompletion>);
}

/// The answer to one `SignatureSigner::sign`. The first call wins; dropping
/// it unanswered fails the signing with `operation-failed`, and the signing
/// leaves nothing pending either way.
#[derive(uniffi::Object)]
pub struct SignatureSignCompletion {
    settle: ForeignSettle<Vec<u8>>,
}

#[uniffi::export]
impl SignatureSignCompletion {
    /// The raw signature: PKCS#1 v1.5 or PSS bytes, or ECDSA `r || s`.
    pub fn finish(&self, signature: Vec<u8>) {
        self.settle.settle(move || Ok(signature));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || {
            Err(Error::named("SigningError", detail)
                .with("code", epdf_kernel::error::code::OPERATION_FAILED))
        });
    }

    /// Fail with the thrown error's name and code: a cancelled prompt is
    /// `operation-cancelled`, and `sign` fails with exactly that.
    pub fn fail_with(&self, failure: HostFailure) {
        self.settle.fail(failure, "signature");
    }
}

/// Trust anchors, DER. An empty list means every verdict tops out at
/// `ValidUntrusted`.
#[uniffi::export(with_foreign)]
pub trait SignatureTrust: Send + Sync {
    fn anchors(&self) -> Result<Vec<Vec<u8>>, ViewerError>;
}

/// The signer a host has set, shared with every document's handle.
pub type SignatureSignerSlot = Arc<Mutex<Option<Arc<dyn SignatureSigner>>>>;

/// The anchors a host has set.
pub type SignatureTrustSlot = Arc<Mutex<Option<Arc<dyn SignatureTrust>>>>;

/// A foreign port's error as the kernel's, its name and code kept.
pub fn kernel_error(error: ViewerError) -> Error {
    match error {
        ViewerError::Kernel { name, code, detail } => {
            let error = Error::named(name, detail);
            match code {
                Some(code) => error.with("code", code),
                None => error,
            }
        }
        ViewerError::Closed => Error::new("[signature] the host port is closed"),
    }
}

/// The plugin's `SignerPort` over a Swift signer. The chain is read once
/// per signing, when the plugin resolves the port. On the kernel thread;
/// `post` carries the signer's answer back there.
pub fn signer_port(
    signer: Arc<dyn SignatureSigner>,
    post: KernelPost,
) -> Result<SignerPort, Error> {
    let certificate_chain = signer.certificate_chain().map_err(kernel_error)?;
    let algorithm = match signer.algorithm() {
        SignatureKeyAlgorithm::RsaPkcs1 => RawAlgorithm::RsaPkcs1,
        SignatureKeyAlgorithm::RsaPss => RawAlgorithm::RsaPss,
        SignatureKeyAlgorithm::Ecdsa => RawAlgorithm::Ecdsa,
    };
    let hash = match signer.digest() {
        SignatureDigest::Sha256 => Digest::Sha256,
        SignatureDigest::Sha384 => Digest::Sha384,
        SignatureDigest::Sha512 => Digest::Sha512,
    };
    Ok(SignerPort::Raw(RawSigner {
        certificate_chain,
        algorithm,
        hash,
        sign: Rc::new(move |message| {
            let (promise, resolver) = Promise::pending();
            let completion = Arc::new(SignatureSignCompletion {
                settle: ForeignSettle::new(resolver, post.clone(), "a SignatureSigner completion"),
            });
            signer.sign(message, completion);
            promise
        }),
    }))
}

struct ForeignTrust(Arc<dyn SignatureTrust>);

impl TrustPort for ForeignTrust {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
        match self.0.anchors() {
            Ok(anchors) => Promise::resolved(anchors),
            Err(error) => Promise::rejected(kernel_error(error)),
        }
    }
}

/// The plugin's `TrustPort` over Swift anchors.
pub fn trust_port(trust: Arc<dyn SignatureTrust>) -> Rc<dyn TrustPort> {
    Rc::new(ForeignTrust(trust))
}

/// A `TrustPort` that reads a host's slot per validation: no anchors when
/// it is empty.
pub struct SlotTrust(pub SignatureTrustSlot);

impl TrustPort for SlotTrust {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
        let trust = self.0.lock().unwrap().clone();
        match trust {
            Some(trust) => ForeignTrust(trust).anchors(),
            None => Promise::resolved(Vec::new()),
        }
    }
}
