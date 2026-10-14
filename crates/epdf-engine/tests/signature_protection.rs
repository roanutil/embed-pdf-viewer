//! Replays `tooling/engine-conformance/vectors/signature/protection.json`,
//! generated from the TypeScript's `deriveProtection` and
//! `protectedCapabilities`, against the Rust port.

use epdf_engine::features::signature::protection::{derive_protection, protected_capabilities};
use epdf_engine::features::signature::types::SignatureDto;
use serde_json::Value;

#[test]
fn protection_agrees_with_typescript() {
    let path = test_support::workspace_root()
        .join("tooling/engine-conformance/vectors/signature/protection.json");
    let file: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(file["schemaVersion"], 1);
    assert_eq!(file["kind"], "signature.protection");
    let cases = file["cases"].as_array().unwrap();
    assert!(cases.len() >= 10, "the corpus shrank: {}", cases.len());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let signatures: Vec<SignatureDto> = serde_json::from_value(case["signatures"].clone())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let protection = derive_protection(&signatures);
        assert_eq!(serde_json::to_value(&protection).unwrap(), case["protection"], "{name}");
        let mut capabilities: Vec<&str> =
            protected_capabilities(Some(&protection)).iter().map(|c| c.as_str()).collect();
        capabilities.sort_unstable();
        let want: Vec<&str> = case["protectedCapabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(capabilities, want, "{name}");
        // The DTOs round-trip unchanged: the Rust shape is the wire's.
        assert_eq!(serde_json::to_value(&signatures).unwrap(), case["signatures"], "{name}");
    }
}
