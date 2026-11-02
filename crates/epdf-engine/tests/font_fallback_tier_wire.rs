//! `fonts.addFallback`'s `tier` on the wire.
//!
//! The TypeScript worker passes `req.tier` to `FontRegistrar.addFallback`
//! unchecked, and the registrar defaults an absent one to `FALLBACK_TIERS.own`.
//! The order the tiers produce is covered in `font_registry.rs`; this covers
//! the field.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::host::Host;

fn roboto() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
    )
    .expect("read Roboto-Regular.ttf")
}

fn call(host: &mut Host, request: serde_json::Value, payloads: Vec<Vec<u8>>) -> serde_json::Value {
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).expect("every response is JSON")
}

#[test]
fn add_fallback_takes_a_tier_and_defaults_an_absent_one() {
    // PDFium's font registry is per thread, so the host gets a thread of its own.
    std::thread::spawn(|| {
        let mut host = Host::new();
        for key in ["own", "built-in"] {
            // `italic: null` infers. This host still decodes italic as an
            // optional bool, where the TypeScript sends the C API's -1.
            let json = call(
                &mut host,
                serde_json::json!({
                    "kind": "fonts.register", "fontKey": key, "familyName": "",
                    "weight": 0, "italic": null,
                }),
                vec![roboto()],
            );
            assert!(json.get("error").is_none(), "register {key}: {json}");
        }

        let json = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "built-in", "tier": 1 }),
            Vec::new(),
        );
        assert!(json.get("error").is_none(), "a tier was refused: {json}");

        let json = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "own" }),
            Vec::new(),
        );
        assert!(json.get("error").is_none(), "an absent tier was refused: {json}");

        let json = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "own", "tier": "own" }),
            Vec::new(),
        );
        assert_eq!(json["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat), "{json}");
    })
    .join()
    .expect("the test thread panicked");
}
