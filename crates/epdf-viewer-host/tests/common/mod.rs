//! Helpers more than one host test file shares.
#![allow(dead_code)]

use epdf_ffi::ViewerError;
use epdf_viewer_host::{OpenDocumentOptions, ViewerDelegate};

/// A delegate that ignores every report.
pub struct NullDelegate;

impl ViewerDelegate for NullDelegate {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, _error: ViewerError) {}
}

/// A fixture's bytes, workspace-relative.
pub fn fixture(relative: &str) -> Vec<u8> {
    test_support::fixture(relative)
}

pub fn options() -> OpenDocumentOptions {
    OpenDocumentOptions { name: None, password: None, activate: true }
}

/// A `SignatureSigner` over the TEST-ONLY key and certificate (16
/// signature). `fail` makes `sign` throw the way a cancelled prompt does.
pub struct TestSigner {
    key: rsa::RsaPrivateKey,
    chain: Vec<Vec<u8>>,
    fail: bool,
}

const KEYS: &str = "packages/engine/main/test/fixtures/signature-compat/keys";

/// A PEM file's DER.
pub fn pem_der(relative: &str) -> Vec<u8> {
    use base64::Engine as _;
    let pem = String::from_utf8(test_support::fixture(&format!("{KEYS}/{relative}"))).unwrap();
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    base64::engine::general_purpose::STANDARD.decode(body).unwrap()
}

impl TestSigner {
    pub fn load(fail: bool) -> Self {
        use rsa::pkcs8::DecodePrivateKey as _;
        let key =
            rsa::RsaPrivateKey::from_pkcs8_der(&pem_der("TEST-ONLY-private-key.pem")).unwrap();
        TestSigner { key, chain: vec![pem_der("TEST-ONLY-signer.pem")], fail }
    }
}

impl epdf_plugin_signature::ffi::SignatureSigner for TestSigner {
    fn certificate_chain(&self) -> Result<Vec<Vec<u8>>, ViewerError> {
        Ok(self.chain.clone())
    }
    fn algorithm(&self) -> epdf_plugin_signature::ffi::SignatureKeyAlgorithm {
        epdf_plugin_signature::ffi::SignatureKeyAlgorithm::RsaPkcs1
    }
    fn digest(&self) -> epdf_plugin_signature::ffi::SignatureDigest {
        epdf_plugin_signature::ffi::SignatureDigest::Sha256
    }
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, ViewerError> {
        use sha2::Digest as _;
        if self.fail {
            return Err(ViewerError::Kernel {
                name: "UserCancelled".into(),
                code: Some("operation-cancelled".into()),
                detail: "the user cancelled".into(),
            });
        }
        Ok(self
            .key
            .sign(rsa::Pkcs1v15Sign::new::<sha2::Sha256>(), &sha2::Sha256::digest(&message))
            .unwrap())
    }
}

/// The TEST-ONLY certificate as a trust anchor.
pub struct TestAnchors;

impl epdf_plugin_signature::ffi::SignatureTrust for TestAnchors {
    fn anchors(&self) -> Result<Vec<Vec<u8>>, ViewerError> {
        Ok(vec![pem_der("TEST-ONLY-signer.pem")])
    }
}
