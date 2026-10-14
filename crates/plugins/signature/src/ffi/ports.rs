//! The host ports, implemented in Swift: a key that signs, and the anchors
//! validation trusts. Both are called on the kernel thread, synchronously.
//!
//! They throw `ViewerError` rather than the shared `HostError` so a signer
//! can say why it failed in the kernel's codes: a cancelled biometric
//! prompt is `operation-cancelled`, and `sign` fails with exactly that.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::ViewerError;
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
#[uniffi::export(with_foreign)]
pub trait SignatureSigner: Send + Sync {
    fn certificate_chain(&self) -> Result<Vec<Vec<u8>>, ViewerError>;
    fn algorithm(&self) -> SignatureKeyAlgorithm;
    fn digest(&self) -> SignatureDigest;
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, ViewerError>;
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
/// per signing, when the plugin resolves the port.
pub fn signer_port(signer: Arc<dyn SignatureSigner>) -> Result<SignerPort, Error> {
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
        sign: Rc::new(move |message| match signer.sign(message) {
            Ok(signature) => Promise::resolved(signature),
            Err(error) => Promise::rejected(kernel_error(error)),
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
