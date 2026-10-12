//! Hostile CMS bytes: inputs built to crash the reader or the verifier
//! rather than to fail verification. Each must come back as a verdict.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use epdf_signature_core::cert::{AlgId, Spki};
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

/// An RSA public key with a `bits`-bit odd modulus (not a product of two
/// primes: nothing here needs the private half) and exponent `e`.
fn rsa_spki(bits: usize, e: u64) -> Spki {
    let mut n = vec![0x5au8; bits / 8];
    n[0] = 0x80 | 0x5a;
    *n.last_mut().unwrap() |= 1;
    let mut n_der = vec![0];
    n_der.extend(n);
    let key = der::sequence(&[der::integer_bytes(&n_der), der::integer(e)]);
    let algorithm =
        AlgId { oid: oids::RSA_ENCRYPTION.to_string(), params: der::from_ber(&der::null()) };
    Spki { raw: Vec::new(), algorithm, key }
}

fn verify_sha256_with_rsa(spki: &Spki) -> Result<bool, String> {
    let algorithm =
        AlgId { oid: oids::SHA256_WITH_RSA.to_string(), params: der::from_ber(&der::null()) };
    let signature = vec![0x11; (spki.key.len() - 8).max(1)];
    crypto::verify_with_public_key(b"data", &signature, spki, &algorithm, None)
}

#[test]
fn rsa_keys_up_to_webcrypto_s_limit_import() {
    // `vectors/core/signature-verify.json` (`verify-rsa8192`) verifies a real
    // 8192-bit signature; here a 16384-bit key imports and a junk signature
    // is simply false.
    for bits in [8192, 16384] {
        assert_eq!(verify_sha256_with_rsa(&rsa_spki(bits, 65537)), Ok(false), "{bits} bits");
    }
}

#[test]
fn rsa_keys_past_webcrypto_s_limit_never_verify() {
    // Node's WebCrypto imports a 16400-bit key and answers false to every
    // verify with it (OpenSSL's modulus limit), whatever its exponent.
    assert_eq!(verify_sha256_with_rsa(&rsa_spki(16400, 65537)), Ok(false));
    assert_eq!(verify_sha256_with_rsa(&rsa_spki(16400, 2)), Ok(false));
}

#[test]
fn lengths_past_the_input_are_refused_not_allocated() {
    // A definite length near u32::MAX, and one near u64::MAX, over a few bytes.
    for bytes in [
        vec![0x30, 0x84, 0xff, 0xff, 0xff, 0xff, 0x05, 0x00],
        vec![0x30, 0x88, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x05, 0x00],
        // Nine length octets: wider than any length the reader can hold.
        vec![0x30, 0x89, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0x00],
        // The reserved length octet 0xff.
        vec![0x30, 0xff, 0x05, 0x00],
    ] {
        assert_eq!(der::from_ber(&bytes), None, "{bytes:02x?}");
        assert_eq!(verify_cms_signature(&bytes), Cryptography::Invalid, "{bytes:02x?}");
    }
}

#[test]
fn truncated_and_unterminated_encodings_are_refused() {
    let unterminated = [0x30, 0x80, 0x05, 0x00];
    let cut_tag = [0x1f, 0xff, 0xff];
    let huge_tag = [0x1f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 0x00];
    for bytes in [&unterminated[..], &cut_tag[..], &huge_tag[..], &[0x30][..], &[][..]] {
        assert_eq!(verify_cms_signature(bytes), Cryptography::Invalid, "{bytes:02x?}");
    }
    assert_eq!(der::from_ber(&unterminated), None);
}

#[test]
fn every_prefix_of_a_real_cms_is_a_verdict() {
    // No cut of a valid signature panics or verifies.
    let whole = fixture("cms", "psssha256");
    assert_eq!(verify_cms_signature(&whole), Cryptography::Valid);
    for end in (0..whole.len()).step_by(7) {
        assert_ne!(verify_cms_signature(&whole[..end]), Cryptography::Valid, "cut at {end}");
    }
}
