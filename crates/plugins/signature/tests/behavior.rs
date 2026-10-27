//! The behaviour Task 13.25 pins beyond the printed traces, against
//! `packages/plugin/signature/src/{write/*,sync/signatures,services/*,tools/armed-mark}.ts`.
//!
//! The first group replays vector cases through the Rust adapter and names
//! the facts each one carries: a free placement that falls back to the
//! stamp plugin creates annotation 60 and settles, an imported signatures
//! library's mark reaches a field as a sign request, a refused fill and a
//! failed signer clean up after themselves, and an annotation hit keeps its
//! precedence over the armed mark. The second drives the capability
//! directly for what no vector reaches: the settings, a signal, an engine
//! that disowns a parked signing, and the fold's handling of a cancellation
//! for another signing.

use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, OpenOptions, OpenSource, rt};
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::Form;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_signature::harness::Signature;
use epdf_plugin_signature::*;
use epdf_plugin_stamp::harness::Stamp;
use epdf_signature_core::{CmsSignRequest, CmsSigner, SignerPort};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::{Registry, compare_trace, load, scenario};
use serde_json::{Value, json};

const SUITE: &str = "plugins/signature.json";

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Annotation).with(Form).with(Stamp).with(Signature)
}

fn case(name: &str) -> Value {
    load(SUITE)["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"))
        .clone()
}

/// The Rust trace of a vector case, which must also match the TypeScript's.
fn replay(name: &str) -> Vec<Value> {
    let case = case(name);
    let run = scenario::run_case(&case, &registry());
    let expected = case["trace"].as_array().cloned().unwrap_or_default();
    assert_eq!(compare_trace(&expected, &run.trace), None, "{name} replays as the TypeScript does");
    run.trace
}

/// What the promise a step returned settled with, wherever it settled.
fn settled_of(trace: &[Value], step: usize) -> Value {
    let id = trace[step]["outcome"]["promise"].clone();
    trace
        .iter()
        .flat_map(|s| s["settled"].as_array().cloned().unwrap_or_default())
        .find(|s| s["promise"] == id)
        .unwrap_or_else(|| panic!("step {step}'s promise never settled"))
}

fn paths(step: &Value) -> Vec<String> {
    step["engine"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| e["path"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn callbacks(step: &Value) -> Vec<String> {
    step["callbacks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c["fn"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_free_asset_placement_falls_back_to_the_stamp_plugin_and_settles() {
    let trace = replay("place-by-mode-visual");
    // `placeMark({assetId}, at)`: the stamp plugin places it, annotation 60.
    let placed = settled_of(&trace, 5);
    assert_eq!(placed["resolved"]["kind"], "placed");
    assert_eq!(placed["resolved"]["annotation"]["objectNumber"], 60);
    assert_eq!(paths(&trace[5]), ["apply"]);
    // Bytes go through the annotation plugin, the next number.
    assert_eq!(settled_of(&trace, 6)["resolved"]["annotation"]["objectNumber"], 61);
}

#[test]
fn an_armed_mark_off_a_field_stays_a_stamp_creating_annotation_60() {
    let trace = replay("an-armed-mark-over-a-field");
    // The press over an empty page area: the annotation plugin's placement.
    let apply = &trace[12]["engine"][0];
    assert_eq!(apply["path"], "apply");
    let op = &apply["args"][0]["ops"][0];
    assert_eq!(
        (op["type"].as_str(), op["objectNumber"].as_u64()),
        (Some("annotations.create"), Some(60))
    );
    assert_eq!(op["data"]["name"], "Stamp1");
    assert!(callbacks(&trace[12]).is_empty(), "no sign request off a field");
}

#[test]
fn an_imported_mark_reaches_the_field_as_a_sign_request() {
    let trace = replay("an-armed-mark-over-a-field");
    assert_eq!(settled_of(&trace, 3)["resolved"]["library"]["kind"], "signatures");
    let request = &trace[17]["callbacks"][0];
    assert_eq!(request["fn"], "SignRequested");
    assert_eq!(request["args"][0]["mark"], json!({ "assetId": "stamp-lib-0-1:Stamp1" }));
    assert_eq!(request["args"][0]["field"], json!({ "kind": "objectNumber", "objectNumber": 2 }));
    assert!(paths(&trace[17]).is_empty(), "mode ask places nothing");
}

#[test]
fn an_armed_mark_over_a_widget_never_lands_as_a_stamp() {
    let trace = replay("an-armed-mark-over-a-field");
    // Over the signed field and the field with a value (steps 5 to 8) the
    // signature handler consumes the press. Over the text field (steps 9
    // and 10) it declines, and the press still places no stamp: a widget
    // hit outranks placement (`annotation-edit`, 100, over
    // `annotation-place`, 95). The vector does not record the selection the
    // edit handler makes, so this pins only that nothing is placed or asked.
    for (step, entry) in trace.iter().enumerate().take(11).skip(5) {
        assert!(paths(entry).is_empty(), "step {step} placed nothing");
        assert!(callbacks(entry).is_empty(), "step {step} asked nothing");
    }
    // The press over an empty area after a re-arm does place (step 12), so
    // the quiet presses above were not a dead tool.
    assert_eq!(paths(&trace[12]), ["apply"]);
}

#[test]
fn a_refused_fill_and_a_failed_signer_clean_up() {
    let fills = replay("fill-and-clear");
    for (step, code) in [(16, "conflict"), (17, "conflict"), (18, "not-found")] {
        let refused = settled_of(&fills, step);
        assert_eq!(refused["rejected"]["$error"]["code"], code, "step {step}");
        assert!(paths(&fills[step]).is_empty(), "step {step} wrote nothing");
    }
    // The fill after the refusals still lands: the busy bracket closed.
    assert_eq!(paths(&fills[19]), ["forms.setSignatureAppearance"]);

    let signing = replay("sign-with-a-raw-signer");
    for step in 3..=5 {
        assert_eq!(paths(&signing[step]), ["signatures.prepare", "signatures.cancel"]);
        assert!(settled_of(&signing, step)["rejected"].is_object(), "step {step}");
    }
    let landing = replay("a-mark-that-fails-to-land");
    assert_eq!(landing[4]["callbacks"][0]["fn"], "console.error");
    assert_eq!(landing[4]["callbacks"][0]["args"][1]["$error"]["code"], "unsupported");
}

// ── direct ────────────────────────────────────────────────────────────────────

/// A vector case's engine, for a kernel driven from the test.
fn engine_of(name: &str) -> Value {
    case(name)["engine"].clone()
}

fn plugins(config: SignatureConfig) -> Vec<AnyPlugin> {
    vec![
        epdf_plugin_interaction::interaction_plugin(Default::default()),
        epdf_plugin_form::form_plugin(None),
        signature_plugin(config),
    ]
}

fn cms_key(on_sign: impl Fn() + 'static) -> SignatureKey {
    // The CMS the vectors' signer answers, which passes the completion gate
    // against their prepared digest.
    let spec =
        case("sign-one-shot")["plugins"][2]["config"]["signer"]["answer"]["resolves"].clone();
    let cms = epdf_kernel::bytes::Bytes::from_json(&spec).expect("cms bytes").0;
    SignatureKey::port(SignerPort::Cms(CmsSigner {
        sign: Rc::new(move |_: CmsSignRequest| {
            on_sign();
            Promise::resolved(cms.clone())
        }),
    }))
}

fn field(n: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

fn mark() -> Mark {
    Mark::Source { source: case("sign-one-shot")["steps"][5]["args"][0]["mark"]["source"].clone() }
}

/// A kernel over the fake engine with document `a` open.
struct Rig {
    // Held for the instance's lifetime; dropped with the rig.
    _kernel: Kernel,
    fake: Rc<FakeEngine>,
    signature: SignatureCapability,
}

impl Rig {
    fn open(config: SignatureConfig, engine: Value) -> Rig {
        rt::reset();
        let fake = FakeEngine::new(engine);
        let kernel =
            Kernel::new(Rc::new(AsEngine(fake.clone())), plugins(config), KernelOptions::default())
                .expect("kernel");
        settle(&kernel.start()).expect("start");
        let opened = kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        let answer = settle(&opened).expect("open");
        let id = answer["document"]["id"].as_str().expect("document id").to_string();
        let signature = kernel.capability(&SIGNATURE, Some(&id)).expect("signature");
        Rig { _kernel: kernel, fake, signature }
    }

    fn mark(&self) -> usize {
        self.fake.log_len()
    }

    fn logged(&self, from: usize) -> Vec<String> {
        self.fake
            .log_since(from)
            .iter()
            .map(|e| e["path"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    fn emit(&self, event: &Value) {
        self.fake.emit("a", event).expect("emit");
        rt::run_until_stalled();
    }
}

/// Run the runtime, firing timers as they come due, until `promise` settles.
fn settle<T: Clone + 'static>(promise: &Promise<T>) -> Result<T, Error> {
    loop {
        rt::run_until_stalled();
        if let Some(result) = promise.peek() {
            return result;
        }
        match rt::next_deadline() {
            Some(deadline) => rt::advance((deadline - rt::now()).max(0.0)),
            None => panic!("the promise can never settle"),
        }
    }
}

fn failure<T: Clone + 'static>(promise: &Promise<T>) -> Error {
    match settle(promise) {
        Ok(_) => panic!("expected a rejection"),
        Err(error) => error,
    }
}

#[test]
fn a_settings_change_applies_at_once_and_replaces_the_key_whole() {
    let rig = Rig::open(SignatureConfig::default(), engine_of("nothing-signed-reads"));
    let signature = &rig.signature;
    assert_eq!(signature.get_mode(), SignatureMode::Visual);
    let key = cms_key(|| {});
    signature
        .update_settings(&SignatureConfig { key: Some(Some(key.clone())), ..Default::default() });
    assert_eq!(signature.get_mode(), SignatureMode::Sign, "a key makes sign the default");
    assert!(signature.get_settings().key.is_some_and(|k| k.same(&key)), "the same port reads back");
    signature.update_settings(&SignatureConfig {
        mode: Some(Some(SignatureMode::Ask)),
        ..Default::default()
    });
    assert_eq!(signature.get_mode(), SignatureMode::Ask);
    let other = cms_key(|| {});
    signature
        .update_settings(&SignatureConfig { key: Some(Some(other.clone())), ..Default::default() });
    assert!(signature.get_settings().key.is_some_and(|k| k.same(&other) && !k.same(&key)));
    // A port rebuilt from the same parts is another key here (identity, a
    // Rust-only divergence: the TypeScript's `sameValue` would call a
    // rebuilt CMS port no change). The setting changes and reads back as the
    // new port.
    let changes = Rc::new(std::cell::Cell::new(0));
    let heard = changes.clone();
    let off = signature.on_settings_changed(move |event| {
        assert_eq!(event.changed, ["key"]);
        heard.set(heard.get() + 1);
        Ok(())
    });
    let SignatureKey::Port(port) = &other else { panic!("a port") };
    let rebuilt = SignatureKey::port((**port).clone());
    signature.update_settings(&SignatureConfig {
        key: Some(Some(rebuilt.clone())),
        ..Default::default()
    });
    assert_eq!(changes.get(), 1);
    assert!(signature.get_settings().key.is_some_and(|k| k.same(&rebuilt) && !k.same(&other)));
    // The same key again changes nothing.
    signature.update_settings(&SignatureConfig {
        key: Some(Some(rebuilt.clone())),
        ..Default::default()
    });
    assert_eq!(changes.get(), 1);
    off.call();
    signature.reset_settings();
    assert!(signature.get_settings().key.is_none());
    assert_eq!(signature.get_mode(), SignatureMode::Visual);
}

#[test]
fn a_signal_that_fires_before_the_seal_leaves_nothing_signed() {
    let signal = AbortSignal::new();
    let aborting = signal.clone();
    let config = SignatureConfig {
        key: Some(Some(cms_key(move || aborting.abort(Error::new("stop"))))),
        ..Default::default()
    };
    let rig = Rig::open(config, engine_of("sign-one-shot"));
    let signature = &rig.signature;
    let from = rig.mark();
    let options = OperationOptions { signal: Some(signal.clone()) };
    let error = failure(&signature.sign(SignFieldInput::new(field(2), mark()), options));
    assert!(error.is_plugin_error(Some(code::OPERATION_CANCELLED)), "{error:?}");
    let calls = rig.logged(from);
    assert!(calls.contains(&"signatures.prepare".to_string()), "{calls:?}");
    assert!(
        calls.contains(&"signatures.cancel".to_string()),
        "the candidate is given up: {calls:?}"
    );
    assert!(!calls.contains(&"signatures.complete".to_string()), "nothing sealed: {calls:?}");
    assert!(!signature.is_busy());

    // Already aborted: refused before anything starts.
    let from = rig.mark();
    let options = OperationOptions { signal: Some(signal) };
    let error = failure(&signature.sign(SignFieldInput::new(field(2), mark()), options));
    assert!(error.is_plugin_error(Some(code::OPERATION_CANCELLED)));
    assert!(rig.logged(from).is_empty());
}

fn prepared_event(signing_id: &str) -> Value {
    json!({
        "type": "signatures.prepared",
        "signingId": signing_id,
        "field": field(2),
        "origin": { "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null },
    })
}

#[test]
fn a_parked_signing_the_engine_disowns_is_reloaded_away() {
    let mut engine = engine_of("a-parked-signing-from-elsewhere");
    engine["documents"]["a"]["responses"]
        .as_array_mut()
        .expect("responses")
        .insert(0, json!({ "path": "signatures.cancel", "value": { "status": "unknown" } }));
    let rig = Rig::open(SignatureConfig::default(), engine);
    let signature = &rig.signature;
    rig.emit(&prepared_event("remote-1"));
    assert_eq!(signature.get_pending().map(|p| p.signing_id).as_deref(), Some("remote-1"));
    let from = rig.mark();
    settle(&signature.cancel_pending(OperationOptions::default())).expect("cancel");
    // No event will clear it: the mirror reads the signatures again, and the
    // reload drops the signing the engine disowned (then, Sig1 being signed,
    // judges the snapshot it loaded).
    assert_eq!(rig.logged(from)[..2], ["signatures.cancel", "signatures.list"]);
    assert_eq!(signature.get_pending(), None);
}

#[test]
fn a_cancellation_clears_the_parked_signing_whatever_its_id() {
    // `foldSignatureEvent` clears `pending` on any `signatures.cancelled`,
    // without comparing `signingId` (model.ts:145-146): the Rust fold does
    // the same, as the TypeScript is the oracle.
    let rig = Rig::open(SignatureConfig::default(), engine_of("document-events"));
    let signature = &rig.signature;
    rig.emit(&prepared_event("s-1"));
    assert!(signature.get_pending().is_some());
    let mut cancelled = prepared_event("s-2");
    cancelled["type"] = json!("signatures.cancelled");
    rig.emit(&cancelled);
    assert_eq!(signature.get_pending(), None);
}

#[test]
fn revision_reads_need_doc_download() {
    let mut engine = engine_of("revisions-and-analysis");
    engine["documents"]["a"]["security"] = json!({ "allows": ["doc.sign", "doc.forms.fill"] });
    let rig = Rig::open(SignatureConfig::default(), engine);
    let signature = &rig.signature;
    assert!(!signature.can_read_revision());
    let from = rig.mark();
    let error = failure(
        &signature.read_revision(json!({ "revisionIndex": 0 }), OperationOptions::default()),
    );
    assert!(error.is_plugin_error(Some(code::PERMISSION_DENIED)), "{error:?}");
    assert!(rig.logged(from).is_empty());
}

#[test]
fn signer_rows_split_each_signatures_library_by_role() {
    use epdf_plugin_stamp::{StampAsset, StampLibrary};
    let library = |id: &str, kind: &str| {
        serde_json::from_value::<StampLibrary>(
            json!({ "id": id, "name": id.to_uppercase(), "kind": kind, "assetIds": [] }),
        )
        .expect("library")
    };
    let asset = |library: &str, name: &str| {
        serde_json::from_value::<StampAsset>(json!({
            "id": format!("{library}:{name}"),
            "libraryId": library,
            "kind": "stamp",
            "name": name,
            "label": name,
            "size": { "width": 10, "height": 5 },
            "page": { "kind": "objectNumber", "objectNumber": 1 },
        }))
        .expect("asset")
    };
    let libraries =
        vec![library("a", "signatures"), library("b", "stamps"), library("c", "signatures")];
    let assets = vec![
        asset("a", "signature"),
        asset("a", "initials"),
        asset("a", "second"),
        asset("b", "signature"),
    ];
    let rows = signer_rows_of(&libraries, &assets);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].library_id.as_str(), rows[0].name.as_str()), ("a", "A"));
    let names: Vec<&str> = rows[0].signatures.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["signature", "second"]);
    assert_eq!(rows[0].initials.as_ref().map(|a| a.name.as_str()), Some("initials"));
    assert!(rows[1].signatures.is_empty() && rows[1].initials.is_none());
}

/// M37: the registration's port table keeps only the ports the registered
/// and current settings name, as the TypeScript store holds them; a key a
/// change replaced is released.
#[test]
fn a_replaced_key_is_released_from_the_port_table() {
    let rig = Rig::open(SignatureConfig::default(), engine_of("nothing-signed-reads"));
    let signature = &rig.signature;
    for _ in 0..50 {
        let key = cms_key(|| {});
        signature.update_settings(&SignatureConfig { key: Some(Some(key)), ..Default::default() });
    }
    assert_eq!(signature.retained_ports(), 1, "only the current key");
    let kept = signature.get_settings().key.expect("a key");
    signature.update_settings(&SignatureConfig { key: Some(None), ..Default::default() });
    assert_eq!(signature.retained_ports(), 0);
    signature
        .update_settings(&SignatureConfig { key: Some(Some(kept.clone())), ..Default::default() });
    assert!(
        signature.get_settings().key.is_some_and(|k| k.same(&kept)),
        "set again, it reads back"
    );
    signature.reset_settings();
    assert!(signature.get_settings().key.is_none());
}

/// M37's count covers both ports: a trust port the settings name is held
/// beside the key.
#[test]
fn the_port_table_counts_the_trust_port_beside_the_key() {
    struct NoAnchors;
    impl epdf_signature_core::trust::TrustPort for NoAnchors {
        fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
            Promise::resolved(Vec::new())
        }
    }
    let rig = Rig::open(SignatureConfig::default(), engine_of("nothing-signed-reads"));
    rig.signature.update_settings(&SignatureConfig {
        key: Some(Some(cms_key(|| {}))),
        trust: Some(Some(Rc::new(NoAnchors))),
        ..Default::default()
    });
    assert_eq!(rig.signature.retained_ports(), 2, "the key and the trust port");
}

/// M37: a reset drops the current key the store no longer holds, as the
/// TypeScript store lets the replaced value go.
#[test]
fn a_reset_releases_the_key_it_replaced() {
    let rig = Rig::open(SignatureConfig::default(), engine_of("nothing-signed-reads"));
    let signature = &rig.signature;
    signature.update_settings(&SignatureConfig {
        key: Some(Some(cms_key(|| {}))),
        ..Default::default()
    });
    assert_eq!(signature.retained_ports(), 1);
    signature.reset_settings();
    assert_eq!(signature.retained_ports(), 0, "nothing registered, nothing kept");
}
