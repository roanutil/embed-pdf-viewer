//! `vectors/core/signature-*.json`, generated from `packages/core/signature`.

use epdf_signature_core::harness::{SignatureHarness, functions};
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new().with(SignatureHarness), &functions());
}

#[test]
fn parse_matches_the_typescript() {
    replay("core/signature-parse.json");
}

#[test]
fn verify_build_and_gate_match_the_typescript() {
    replay("core/signature-verify.json");
}

#[test]
fn trust_matches_the_typescript() {
    replay("core/signature-trust.json");
}

#[test]
fn sign_and_validate_match_the_typescript() {
    replay("core/signature-document.json");
}

#[test]
fn signers_match_the_typescript() {
    replay("core/signature-signers.json");
}
