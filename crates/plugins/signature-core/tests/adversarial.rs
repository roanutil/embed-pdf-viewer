//! Hostile CMS bytes: inputs built to crash the reader or the verifier
//! rather than to fail verification. Each must come back as a verdict.

use epdf_signature_core::{Cryptography, der, verify_cms_signature};

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
