//! `fonts.addFallback`'s `tier` on the wire.
//!
//! The TypeScript worker passes `req.tier` to `FontRegistrar.addFallback`
//! unchecked. The registrar defaults an absent one to `FALLBACK_TIERS.own`
//! and hands any other value to `EPDFFont_AddFallbackFont`, whose WebAssembly
//! `int` parameter takes `ToInt32(ToNumber(v))`. These tests read the chain
//! PDFium actually tries, through `EPDFDoc_ResolveFonts` on the host's own
//! thread, so they check the tier PDFium got rather than what was reported.

use epdf_engine::host::Host;

fn roboto() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
    )
    .expect("read Roboto-Regular.ttf")
}

fn call(
    host: &mut Host,
    mut request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> serde_json::Value {
    host.fill_envelope(&mut request);
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).expect("every response is JSON")
}

/// Register `keys` in order, add each as a fallback with the tier given (or
/// none), and answer the keys in the order PDFium tries them.
///
/// Font ids are a per-thread counter, so on a fresh thread the n-th key
/// registered has the n-th smallest id, which is how a resolved id maps
/// back to a key.
fn chain(fallbacks: Vec<(&'static str, Option<serde_json::Value>)>) -> Vec<&'static str> {
    std::thread::spawn(move || {
        let mut host = Host::new();
        for (key, _) in &fallbacks {
            let json = call(
                &mut host,
                // `italic: -1` is the C API's infer value, as the TypeScript sends it.
                serde_json::json!({
                    "kind": "fonts.register", "fontKey": key, "familyName": "",
                    "weight": 0, "italic": -1,
                }),
                vec![roboto()],
            );
            assert!(json.get("error").is_none(), "register {key}: {json}");
        }
        for (key, tier) in &fallbacks {
            let mut request = serde_json::json!({ "kind": "fonts.addFallback", "fontKey": key });
            if let Some(tier) = tier {
                request["tier"] = tier.clone();
            }
            let json = call(&mut host, request, Vec::new());
            assert!(json.get("error").is_none(), "{key} with {tier:?} was refused: {json}");
        }

        let document = pdfium::Document::from_bytes(
            pdfium::Library::acquire(),
            &test_support::fixture("tooling/engine-conformance/fixtures/annotation_kinds.pdf"),
            None,
        )
        .expect("open");
        let run = pdfium::FontResolveRun {
            text: "Hi".to_string(),
            family: "Helvetica".to_string(),
            weight: 400,
            italic: false,
            program: 0,
        };
        let resolution = document.resolve_fonts(None, &[run]).expect("read").expect("a snapshot");
        let tried = &resolution.runs[0].fallbacks;
        let mut by_id: Vec<_> = tried.clone();
        by_id.sort();
        tried
            .iter()
            .map(|id| fallbacks[by_id.iter().position(|x| x == id).expect("a known id")].0)
            .collect()
    })
    .join()
    .expect("the test thread panicked")
}

#[test]
fn a_fraction_truncates() {
    // 1.7 and -0.5 are tiers 1 and 0, so the second comes first and the
    // first ties with a plain 1 in the order added.
    use serde_json::json;
    assert_eq!(
        chain(vec![
            ("fraction", Some(json!(1.7))),
            ("one", Some(json!(1))),
            ("negative-half", Some(json!(-0.5))),
        ]),
        ["negative-half", "fraction", "one"],
    );
}

#[test]
fn absent_null_and_words_are_tier_zero() {
    use serde_json::json;
    assert_eq!(
        chain(vec![
            ("one", Some(json!(1))),
            ("absent", None),
            ("null", Some(json!(null))),
            ("word", Some(json!("own"))),
            ("object", Some(json!({ "tier": 1 }))),
        ]),
        ["absent", "null", "word", "object", "one"],
    );
}

#[test]
fn a_numeric_string_is_its_number() {
    use serde_json::json;
    assert_eq!(
        chain(vec![("string-one", Some(json!("1"))), ("zero", Some(json!(0)))]),
        ["zero", "string-one"],
    );
}

#[test]
fn large_tiers_wrap_modulo_two_to_the_thirty_second() {
    // 2^32 + 1 is tier 1; 2^31 wraps to the most negative int and is tried
    // before everything.
    use serde_json::json;
    assert_eq!(
        chain(vec![
            ("zero", Some(json!(0))),
            ("wraps-to-one", Some(json!(4_294_967_297u64))),
            ("wraps-negative", Some(json!(2_147_483_648u64))),
        ]),
        ["wraps-negative", "zero", "wraps-to-one"],
    );
}

#[test]
fn the_answer_carries_no_payload() {
    // `WorkerHost.handleFontsAddFallback` returns
    // `wirePack({ tag: 'fonts.addFallback' })`: no fallback list.
    std::thread::spawn(|| {
        let mut host = Host::new();
        let registered = call(
            &mut host,
            serde_json::json!({
                "kind": "fonts.register", "fontKey": "roboto", "familyName": "",
                "weight": 0, "italic": -1,
            }),
            vec![roboto()],
        );
        assert!(registered.get("error").is_none(), "register: {registered}");
        let json = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "roboto", "tier": 1 }),
            Vec::new(),
        );
        assert_eq!(json, serde_json::json!({ "result": null }));
    })
    .join()
    .expect("the test thread panicked");
}

/// Register Roboto as a fallback, send `kind`, and answer the envelope.
fn answer_after_a_fallback(kind: &'static str) -> serde_json::Value {
    std::thread::spawn(move || {
        let mut host = Host::new();
        let registered = call(
            &mut host,
            serde_json::json!({
                "kind": "fonts.register", "fontKey": "roboto", "familyName": "",
                "weight": 0, "italic": -1,
            }),
            vec![roboto()],
        );
        assert!(registered.get("error").is_none(), "register: {registered}");
        let added = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "roboto" }),
            Vec::new(),
        );
        assert!(added.get("error").is_none(), "addFallback: {added}");
        call(&mut host, serde_json::json!({ "kind": kind }), Vec::new())
    })
    .join()
    .expect("the test thread panicked")
}

#[test]
fn clearing_the_fallbacks_answers_no_payload() {
    // `WorkerHost.handleFontsClearFallbacks` returns
    // `wirePack({ tag: 'fonts.clearFallbacks' })`: no fallback list.
    assert_eq!(
        answer_after_a_fallback("fonts.clearFallbacks"),
        serde_json::json!({ "result": null })
    );
}

#[test]
fn clearing_the_fonts_answers_no_payload() {
    // `WorkerHost.handleFontsClear` returns `wirePack({ tag: 'fonts.clear' })`:
    // no count.
    assert_eq!(answer_after_a_fallback("fonts.clear"), serde_json::json!({ "result": null }));
}
