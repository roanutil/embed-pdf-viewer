#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, FormFieldRef, KernelThread, ResourceStatus, ViewerError};
use epdf_plugin_form::{FormConfig, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_signature::ffi::{
    SignatureCoverage, SignatureEvent, SignatureEventListener, SignatureHandle, SignatureMode,
    SignatureModificationLevel, SignatureModificationsBasis, SignatureSummary,
    SignatureValidateRequest,
};
use epdf_plugin_signature::{SIGNATURE, SignatureConfig, signature_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<SignatureEvent>>, AtomicUsize);

impl SignatureEventListener for Events {
    fn on_event(&self, event: SignatureEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn field(n: u32) -> Value {
    json!({ "kind": "objectNumber", "fieldObjectNumber": n })
}

fn page() -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": 1 })
}

/// One signed field (12) over revision 0 of two, one unsigned (14).
fn snapshot() -> Value {
    let signer = json!({ "name": "Ana", "reason": null, "location": null, "contactInfo": null,
                         "claimedTime": "D:20260901120000Z" });
    let base = |n: u32, signed: bool| {
        json!({
            "index": if n == 12 { 0 } else { 1 }, "field": field(n), "fieldName": format!("Sig{n}"),
            "widget": { "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": n + 1 },
                        "annotObjectNumber": n + 1, "page": page() },
            "signed": signed, "kind": "signature",
            "filter": if signed { json!("Adobe.PPKLite") } else { Value::Null },
            "subFilter": if signed { json!("ETSI.CAdES.detached") } else { Value::Null },
            "byteRange": if signed { json!([0, 100, 200, 50]) } else { Value::Null },
            "contentsSize": if signed { 8192 } else { 0 },
            "coverage": if signed { json!("whole-revision") } else { Value::Null },
            "revisionIndex": if signed { json!(0) } else { Value::Null },
            "signer": signer, "docMdp": null, "catalogCertification": false,
            "fieldMdp": null, "lock": null, "seedValue": null
        })
    };
    json!({
        "chainValid": true,
        "revisions": [
            { "index": 0, "end": 250, "xrefOffset": 180, "signatureIndex": 0 },
            { "index": 1, "end": 400, "xrefOffset": 330, "signatureIndex": null }
        ],
        "signatures": [base(12, true), base(14, false)],
        "protection": { "enforced": null, "judged": "annotate", "certification": null,
                        "fieldLocks": [], "policyVersion": 4 }
    })
}

fn analysis() -> Value {
    json!({ "mode": "authoritative", "policyVersion": 4,
            "basis": { "version": { "sha256": "x", "byteLength": 400 }, "editsVersion": 0, "source": "persisted" },
            "since": { "revisionIndex": 0, "signatureIndex": 0 }, "until": { "revisionIndex": 1 },
            "restrictions": [],
            "current": { "verdict": "permitted", "complete": true, "findings": [], "method": "net-state",
                         "primary": { "rule": "annotation", "verdict": "permitted", "objectNumber": 9 } },
            "later": { "revisionCount": 1, "undoneObjectNumbers": [] },
            "verdict": "permitted", "steps": [] })
}

fn spec() -> TestSpec {
    spec_listing(snapshot())
}

/// `spec()`, with `signatures.list` answering `listed`.
fn spec_listing(listed: Value) -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    TestSpec::new(json!({
        "documents": { "a": {
            "id": "a",
            "pages": [{ "index": 0, "ref": page(), "label": null,
                        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                        "boxes": { "media": bx, "crop": bx } }],
            "security": { "allows": "*" },
            "services": ["signatures"]
        } }
    }))
    .respond("signatures.list", move |_| Ok(Reply::value(listed.clone())))
    .respond("signatures.contents", |_| Ok(Reply::value(json!({ "$bytes": "MAA=" }))))
    .respond("signatures.digest", |_| Ok(Reply::value(json!({ "$bytes": "AA==" }))))
    .respond("signatures.analyze", |_| Ok(Reply::value(analysis())))
    .respond("signatures.revisionBytes", |_| Ok(Reply::value(json!({ "$bytes": "JVBERi0=" }))))
    .respond("forms.list", |_| {
        Ok(Reply::value(json!({ "formKind": "acroform", "needsAppearances": false,
                                "calculationOrder": [], "fields": [] })))
    })
}

/// A kernel with interaction, form and signature, built on the kernel
/// thread (the plugin's config and the spec hold `Rc`s).
fn handle_with(
    config: impl FnOnce() -> SignatureConfig + Send + 'static,
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<SignatureHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                form_plugin(FormConfig::default()),
                signature_plugin(config()),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post)),
    )
    .unwrap();
    (thread, handle)
}

fn handle() -> (KernelThread<TestKernel>, Arc<SignatureHandle>) {
    handle_with(SignatureConfig::default, spec)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn refresh_decodes_the_snapshot() {
    let (_thread, s) = handle();
    let snapshot = block_on(s.refresh(CancelToken::new())).unwrap().unwrap();
    assert!(snapshot.chain_valid);
    assert_eq!(snapshot.revisions.len(), 2);
    assert_eq!(snapshot.protection.judged, Some(SignatureModificationLevel::Annotate));
    assert_eq!(block_on(s.status()).unwrap(), ResourceStatus::Ready);
    let signed = block_on(s.signatures()).unwrap();
    assert_eq!(signed.len(), 1);
    assert_eq!(signed[0].coverage, Some(SignatureCoverage::WholeRevision));
    assert_eq!(signed[0].signer.name.as_deref(), Some("Ana"));
    assert_eq!(signed[0].byte_range, Some(vec![0, 100, 200, 50]));
    let widget = signed[0].widget.clone().unwrap();
    assert_eq!(widget.annot_object_number, 13);
    assert!(widget.annotation.is_some());
    assert_eq!(block_on(s.unsigned_fields()).unwrap().len(), 1);
    let one =
        block_on(s.signature(FormFieldRef::ObjectNumber { field_object_number: 14 })).unwrap();
    assert_eq!(one.map(|s| s.signed), Some(false));
    assert!(block_on(s.snapshot()).unwrap().is_some());
}

#[test]
fn a_snapshot_the_record_cannot_hold_is_invalid_state() {
    let bad = || spec_listing(json!({ "chainValid": "yes" }));
    let (_thread, s) = handle_with(SignatureConfig::default, bad);
    let error = block_on(s.refresh(CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn without_anchors_a_good_signature_is_at_best_valid_untrusted() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let verdicts =
        block_on(s.validate(SignatureValidateRequest::default(), CancelToken::new())).unwrap();
    assert_eq!(verdicts.len(), 1);
    // The fake CMS is not a real signature, so this one is indeterminate or
    // invalid; it is never `Valid` without anchors.
    assert_ne!(verdicts[0].summary, SignatureSummary::Valid);
    assert_eq!(verdicts[0].modifications.basis, SignatureModificationsBasis::Persisted);
    assert_eq!(block_on(s.verdicts()).unwrap().map(|v| v.len()), Some(1));
    let field = FormFieldRef::ObjectNumber { field_object_number: 12 };
    assert!(block_on(s.verdict(field)).unwrap().is_some());
}

#[test]
fn analyze_changes_decodes_the_analysis() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let analysis =
        block_on(s.analyze_changes(0, SignatureModificationsBasis::Persisted, CancelToken::new()))
            .unwrap();
    assert_eq!(analysis.later_revisions, 1);
    assert_eq!(analysis.primary.unwrap().object_number, 9);
}

#[test]
fn mode_defaults_to_visual_without_a_signer() {
    let (_thread, s) = handle();
    assert_eq!(block_on(s.mode()).unwrap(), SignatureMode::Visual);
    assert!(!block_on(s.can_sign()).unwrap());
    assert!(block_on(s.can_fill()).unwrap());
    assert!(!block_on(s.can_certify()).unwrap());
    assert!(!block_on(s.is_busy()).unwrap());
    assert_eq!(block_on(s.pending()).unwrap(), None);
}

#[test]
fn set_target_is_observed() {
    let (_thread, s) = handle();
    let events = Arc::new(Events::default());
    let _sub = s.subscribe(events.clone());
    block_on(s.refresh(CancelToken::new())).unwrap();
    let unsigned = block_on(s.unsigned_fields()).unwrap();
    block_on(s.set_target(Some(unsigned[0].field.clone()))).unwrap();
    assert_eq!(block_on(s.target()).unwrap(), Some(unsigned[0].field.clone()));
    block_on(s.request_inspection(unsigned[0].field.clone())).unwrap();
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, SignatureEvent::TargetChanged { field: Some(_) })),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|e| matches!(e, SignatureEvent::InspectionRequested { .. })),
        "{seen:?}"
    );
}

#[test]
fn read_revision_answers_bytes() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.signatures()).unwrap()[0].field.clone();
    let bytes = block_on(s.read_revision(field, CancelToken::new())).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, s) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(s.snapshot()).map(|_| ()).unwrap_err(),
        block_on(s.status()).map(|_| ()).unwrap_err(),
        block_on(s.refresh(CancelToken::new())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, s) = handle();
    let events = Arc::new(Events::default());
    let _sub = s.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
