//! Hostile CMS bytes: inputs built to crash the reader or the verifier
//! rather than to fail verification. Each must come back as a verdict.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use epdf_signature_core::cert::AlgId;
use epdf_signature_core::{Cryptography, crypto, der, oids, parse, verify_cms_signature};

fn fixture(section: &str, name: &str) -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tooling/plugin-conformance/fixtures/signature/fixtures.json"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    STANDARD.decode(json[section][name].as_str().unwrap()).unwrap()
}

/// `depth` indefinite-length SEQUENCEs, each closed by end-of-contents.
fn nested_indefinite(depth: usize) -> Vec<u8> {
    let mut out = [0x30, 0x80].repeat(depth);
    out.extend([0, 0].repeat(depth));
    out
}

/// `depth` definite-length SEQUENCEs around a NULL.
fn nested_definite(depth: usize) -> Vec<u8> {
    let mut out = der::null();
    for _ in 0..depth {
        out = der::sequence(&[out]);
    }
    out
}

#[test]
fn deep_nesting_is_refused_not_a_stack_overflow() {
    let bytes = nested_indefinite(100_000);
    assert_eq!(der::from_ber(&bytes), None);
    assert_eq!(der::from_ber(&nested_definite(1_000)), None);
    assert_eq!(verify_cms_signature(&bytes), Cryptography::Invalid);
}

#[test]
fn modest_nesting_still_reads() {
    assert!(der::from_ber(&nested_definite(32)).is_some());
    assert!(der::from_ber(&nested_indefinite(32)).is_some());
}

/// The `psssha256` signer's own signature and key, checked under
/// RSASSA-PSS parameters whose saltLength INTEGER has `salt` as content.
fn verify_with_salt(salt: &[u8]) -> Result<bool, String> {
    let internal = parse::parse_cms_internal(&fixture("cms", "psssha256")).unwrap();
    let info = &internal.signer_info;
    let params = der::sequence(&[
        der::context(0, &[der::sequence(&[der::oid(oids::SHA256), der::null()])]),
        der::context(2, &[der::integer_bytes(salt)]),
    ]);
    let algorithm = AlgId { oid: oids::RSA_PSS.to_string(), params: der::from_ber(&params) };
    crypto::verify_with_public_key(
        &info.signed_attributes_der(),
        &info.signature,
        &internal.signer_certificate.spki,
        &algorithm,
        Some(internal.parsed.digest_algorithm),
    )
}

#[test]
fn pss_salt_length_that_wraps_is_not_verified() {
    // h_len + s_len + 2 wraps to 0 inside `rsa`, past its length check.
    assert_eq!(verify_with_salt(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xde]), Ok(false));
}

#[test]
fn pss_salt_length_wider_than_u64_is_not_verified() {
    assert_eq!(verify_with_salt(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0]), Ok(false));
}

#[test]
fn pss_salt_length_past_the_modulus_is_not_verified() {
    // 2048-bit key, SHA-256: 256 - 32 - 2 = 222 is the largest that fits.
    assert_eq!(verify_with_salt(&[0x00, 0xdf]), Ok(false));
    assert_eq!(verify_with_salt(&[0x01, 0x00, 0x00, 0x00, 0x14]), Ok(false));
}
