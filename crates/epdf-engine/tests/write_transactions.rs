//! Every write runs as one layer transaction
//! (`packages/engine/main/test/write-transactions.test.ts`). A write that
//! fails after all of its work, before its commit, leaves the document as it
//! was: the same saved objects (only the trailer's `/Size` may grow,
//! counting the object numbers the failed write used), the same annotations,
//! fields and pages, and a session that takes the same write again. A commit
//! that fails makes the session unusable.
//!
//! Upstream forces the failure by making a native call throw; this engine
//! has no proxy over its runtime, so a test arms the same two points on the
//! session ([`DocumentSession::force_commit_failure`]): the
//! `EPDFLayer_IsInTransaction` check a commit makes before it keeps
//! anything, and `EPDFLayer_CommitTransaction` itself. The batch case
//! upstream fails inside `EPDFPage_Flatten` and `EPDFPage_ApplyRedactions` on
//! the second page; here the second page fails by a cancel at its check,
//! after the first page was written (see `redaction_apply.rs`), and by a
//! page the scope names that is not there.

use epdf_engine::cancel::CancelToken;
use epdf_engine::host::Host;
use epdf_engine::session::document::{CommitFailure, DocumentSession};
use epdf_engine::session::registry::SessionKey;
use serde_json::{Value, json};

const HELLO: &str = "packages/engine/main/test/fixtures/hello_world.pdf";
const ARTWORK: &str = "packages/engine/main/test/fixtures/signature_artwork.pdf";
const DOC: &str = "tx";

fn send(host: &mut Host, mut request: Value, payloads: Vec<Vec<u8>>) -> Result<Value, Value> {
    request["jobId"] = json!(1);
    request["docId"] = json!(DOC);
    let response = host.call(&request.to_string(), payloads);
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope["result"].take()),
    }
}

fn call(host: &mut Host, request: Value) -> Value {
    send(host, request.clone(), Vec::new()).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn session(host: &Host) -> &DocumentSession {
    host.sessions().session(&SessionKey::document(DOC)).expect("the session")
}

fn open() -> (Host, Value) {
    let mut host = Host::new();
    send(&mut host, json!({ "kind": "open.fatMem" }), vec![test_support::fixture(HELLO)])
        .expect("open");
    let pages = call(&mut host, json!({ "kind": "pages.list" }));
    let first = pages["pages"][0]["ref"].clone();
    (host, first)
}

/// A save with what may differ between equal documents blanked: the
/// trailer's whole `/ID` array, both ids, as upstream's `comparable` blanks
/// it to `/ID[]`, and `/Size`.
fn comparable(bytes: &[u8]) -> String {
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let ids = blank_after(&text, "/ID", |rest| rest.find(']').map_or(rest.len(), |at| at + 1));
    blank_after(&ids, "/Size", |rest| {
        let digits = rest.trim_start();
        let spaces = rest.len() - digits.len();
        spaces + digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len())
    })
}

/// `text` with what `skip` measures after each `key` replaced by ` _`.
fn blank_after(text: &str, key: &str, skip: impl Fn(&str) -> usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        let tail = &rest[at + key.len()..];
        out.push_str(&rest[..at + key.len()]);
        out.push_str(" _");
        rest = &tail[skip(tail)..];
    }
    out.push_str(rest);
    out
}

/// What a refused write must leave as it was.
#[derive(Debug, PartialEq)]
struct State {
    save: String,
    annotations: Value,
    fields: Value,
    pages: Value,
}

fn state(host: &mut Host, page: &Value) -> State {
    let response = host.call(
        &json!({ "kind": "document.saveBuffer", "jobId": 1, "docId": DOC, "mode": "incremental" })
            .to_string(),
        Vec::new(),
    );
    assert_eq!(response.payloads.len(), 1, "{}", response.json);
    State {
        save: comparable(&response.payloads[0]),
        annotations: call(host, json!({ "kind": "annotations.list", "page": page })),
        fields: call(host, json!({ "kind": "forms.list" })),
        pages: call(host, json!({ "kind": "pages.list" })),
    }
}

fn rect(x: f64) -> Value {
    json!({ "x": x, "y": 40.0, "width": 40.0, "height": 30.0 })
}

fn fqn(name: &str) -> Value {
    json!({ "kind": "fqn", "name": name })
}

fn apply(op: Value) -> Value {
    json!({ "kind": "document.apply", "change": { "ops": [op] } })
}

/// Fail `request` at its commit check, check it left nothing behind, then
/// run it again and answer what it answered.
fn fails_then_applies(
    host: &mut Host,
    page: &Value,
    problems: &mut Vec<String>,
    what: &str,
    request: Value,
    payloads: Vec<Vec<u8>>,
) -> Value {
    let before = state(host, page);
    session(host).force_commit_failure(CommitFailure::InTransactionCheck);
    let failed = send(host, request.clone(), payloads.clone());
    if session(host).pending_commit_failure().is_some() {
        problems.push(format!("{what}: never reached its commit"));
        // Disarm it, so the next write is not the one that fails.
        let _ =
            send(host, json!({ "kind": "metadata.update", "patch": { "subject": "x" } }), vec![]);
    }
    if failed.is_ok() {
        problems.push(format!("{what}: was not refused"));
    }
    let after = state(host, page);
    if after.save != before.save {
        problems.push(format!("{what}: left save changed"));
    }
    if after.annotations != before.annotations {
        problems.push(format!("{what}: left annotations changed"));
    }
    if after.fields != before.fields {
        problems.push(format!("{what}: left fields changed"));
    }
    if after.pages != before.pages {
        problems.push(format!("{what}: left pages changed"));
    }
    send(host, request.clone(), payloads).unwrap_or_else(|e| panic!("{what} again: {e}"))
}

#[test]
fn a_write_that_fails_before_its_commit_leaves_nothing_behind_and_applies_afterwards() {
    let (mut host, first) = open();
    let artwork = test_support::fixture(ARTWORK);
    let hello = test_support::fixture(HELLO);
    let mut problems = Vec::new();
    let host = &mut host;
    let p = &mut problems;
    let circle = |host: &mut Host, x: f64| {
        call(
            host,
            json!({ "kind": "annotations.create", "page": first,
                "draft": { "subtype": "circle", "box": rect(x) } }),
        )["result"]["annotation"]["ref"]
            .clone()
    };

    fails_then_applies(
        host,
        &first,
        p,
        "metadata.update",
        json!({ "kind": "metadata.update", "patch": { "title": "title 1" } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "metadata.updateCustom",
        json!({ "kind": "metadata.updateCustom", "patch": { "Probe": "value 1" } }),
        vec![],
    );
    let square = fails_then_applies(
        host,
        &first,
        p,
        "annotations.create",
        json!({ "kind": "annotations.create", "page": first,
            "draft": { "subtype": "square", "box": rect(20.0) } }),
        vec![],
    )["result"]["annotation"]["ref"]
        .clone();
    let square = fails_then_applies(
        host,
        &first,
        p,
        "annotations.update",
        json!({ "kind": "annotations.update", "ref": square,
            "patch": { "subtype": "square", "box": rect(21.0) } }),
        vec![],
    )["result"]["annotation"]["ref"]
        .clone();
    circle(host, 70.0);
    fails_then_applies(
        host,
        &first,
        p,
        "annotations.reorder",
        json!({ "kind": "annotations.reorder", "page": first, "refs": [square],
            "position": "end" }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "annotations.flatten",
        json!({ "kind": "annotations.flatten", "page": first, "refs": [square],
            "usage": "display" }),
        vec![],
    );
    let victim = circle(host, 120.0);
    fails_then_applies(
        host,
        &first,
        p,
        "annotations.delete",
        json!({ "kind": "annotations.delete", "ref": victim }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.createField",
        json!({ "kind": "forms.createField", "draft": { "family": "text", "name": "probe",
            "widgets": [{ "page": first, "rect": rect(160.0) }] } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.setValue",
        json!({ "kind": "forms.setValue", "ref": fqn("probe"), "value": { "value": "value 2" } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "document.apply",
        json!({ "kind": "document.apply", "change": { "ops": [
            { "type": "forms.setValue", "field": fqn("probe"), "value": { "value": "apply 3" } },
            { "type": "forms.setDisplay", "field": fqn("probe"), "display": "noPrint" },
        ] } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.reset",
        json!({ "kind": "forms.reset", "refs": [fqn("probe")] }),
        vec![],
    );
    let probe = call(
        host,
        json!({ "kind": "forms.export", "effect": "snapshot",
            "selection": { "fields": [fqn("probe")] } }),
    )["bundle"]
        .clone();
    let with_fields = |change: Value| {
        let mut bundle = probe.clone();
        for field in bundle["fields"].as_array_mut().expect("fields") {
            for (key, value) in change.as_object().expect("an object") {
                field["data"][key] = value.clone();
            }
        }
        bundle
    };
    fails_then_applies(
        host,
        &first,
        p,
        "forms.import",
        json!({ "kind": "forms.import", "bundle": with_fields(json!({ "name": "copy4" })),
            "attribution": "restore", "values": true, "mayScript": false }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.importValues",
        json!({ "kind": "forms.importValues", "attribution": "restore",
            "bundle": with_fields(json!({ "value": "imported 5",
                "valueEntry": { "kind": "scalar", "value": "imported 5" } })) }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.updateField",
        json!({ "kind": "forms.updateField", "ref": fqn("probe"),
            "patch": { "family": "text", "required": true } }),
        vec![],
    );
    let added = fails_then_applies(
        host,
        &first,
        p,
        "forms.addWidget",
        json!({ "kind": "forms.addWidget", "ref": fqn("probe"),
            "placement": { "page": first, "rect": rect(210.0) } }),
        vec![],
    );
    let widget = added["result"]["meta"]["changedWidgets"][0]["ref"].clone();
    fails_then_applies(
        host,
        &first,
        p,
        "forms.detachWidget",
        json!({ "kind": "forms.detachWidget", "ref": fqn("probe"), "widget": widget }),
        vec![],
    );
    call(
        host,
        json!({ "kind": "forms.createField", "draft": { "family": "signature", "name": "sig",
            "widgets": [{ "page": first, "rect": rect(260.0) }] } }),
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.setSignatureAppearance",
        json!({ "kind": "forms.setSignatureAppearance", "ref": fqn("sig") }),
        vec![artwork],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "forms.deleteField",
        json!({ "kind": "forms.deleteField", "ref": fqn("probe") }),
        vec![],
    );
    fails_then_applies(host, &first, p, "forms.repair", json!({ "kind": "forms.repair" }), vec![]);
    fails_then_applies(
        host,
        &first,
        p,
        "pages.insertBlank",
        apply(json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 200, "height": 200 } }, "position": "end" })),
        vec![],
    );
    let blank = call(host, json!({ "kind": "pages.list" }))["pages"]
        .as_array()
        .and_then(|pages| pages.last())
        .map(|page| page["ref"].clone())
        .expect("the blank page");
    fails_then_applies(
        host,
        &first,
        p,
        "pages.rotate",
        apply(json!({ "type": "pages.rotate", "pages": [blank], "rotation": 90 })),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.reorder",
        apply(json!({ "type": "pages.reorder", "pages": [blank], "position": "start" })),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.setName",
        json!({ "kind": "pages.setName", "name": "probe", "page": first }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.removeName",
        json!({ "kind": "pages.removeName", "name": "probe" }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.flatten",
        json!({ "kind": "pages.flatten", "pages": [first], "usage": "display" }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.delete",
        apply(json!({ "type": "pages.delete", "pages": [blank] })),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pages.insert",
        apply(json!({ "type": "pages.insert", "source": { "payload": 0 } })),
        vec![hello],
    );
    let redact = call(
        host,
        json!({ "kind": "annotations.create", "page": first,
            "draft": { "subtype": "redact", "rect": rect(300.0) } }),
    )["result"]["annotation"]["ref"]
        .clone();
    fails_then_applies(
        host,
        &first,
        p,
        "redaction.apply",
        json!({ "kind": "redaction.apply", "scope": { "annotations": [redact] } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "attachments.create",
        apply(json!({ "type": "attachments.create",
            "file": { "name": "probe.csv", "mimeType": "text/csv" },
            "resources": { "file": { "payload": 0 } } })),
        vec![vec![1, 2, 3]],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "attachments.delete",
        apply(json!({ "type": "attachments.delete",
            "ref": { "kind": "key", "key": "probe.csv" } })),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "measure.setScale",
        json!({ "kind": "measure.setScale", "page": first, "measure": {
            "subtype": "rectilinear", "ratio": "1 in = 3 m",
            "x": [{ "unit": "m", "conversion": 0.041666666666666664 }],
            "distance": [{ "unit": "m", "conversion": 1 }],
            "area": [{ "unit": "sq m", "conversion": 1 }] } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pieceInfo.update",
        json!({ "kind": "pieceInfo.update", "application": "EMBD_Probe",
            "patch": { "name": "piece 6" } }),
        vec![],
    );
    fails_then_applies(
        host,
        &first,
        p,
        "pieceInfo.delete",
        json!({ "kind": "pieceInfo.delete", "application": "EMBD_Probe" }),
        vec![],
    );

    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn a_commit_that_fails_makes_the_session_unusable() {
    let (mut host, first) = open();
    session(&host).force_commit_failure(CommitFailure::Commit);
    let refused = send(
        &mut host,
        json!({ "kind": "annotations.create", "page": first,
            "draft": { "subtype": "square", "box": rect(20.0) } }),
        vec![],
    )
    .expect_err("the commit failed");
    assert_eq!(refused["code"], "DocNotOpen", "{refused}");
    assert!(session_state(&host).is_none(), "the session still serves reads");
    let after = send(&mut host, json!({ "kind": "pages.list" }), vec![]).expect_err("unusable");
    assert_eq!(after["code"], "DocNotOpen", "{after}");
    assert!(
        after["message"].as_str().is_some_and(|m| m.contains("open the document again")),
        "{after}"
    );
}

fn session_state(host: &Host) -> Option<()> {
    host.sessions().session(&SessionKey::document(DOC)).ok().map(drop)
}

#[test]
fn a_batch_that_fails_on_a_later_page_leaves_the_earlier_pages_as_they_were() {
    let (mut host, first) = open();
    let inserted = call(
        &mut host,
        apply(json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 200, "height": 200 } }, "position": "end" })),
    );
    assert!(inserted["result"]["items"].is_array(), "{inserted}");
    let second = call(&mut host, json!({ "kind": "pages.list" }))["pages"][1]["ref"].clone();
    for page in [&first, &second] {
        call(
            &mut host,
            json!({ "kind": "annotations.create", "page": page,
                "draft": { "subtype": "square", "box": rect(20.0) } }),
        );
        call(
            &mut host,
            json!({ "kind": "annotations.create", "page": page,
                "draft": { "subtype": "redact", "rect": rect(80.0) } }),
        );
    }
    let before = state(&mut host, &first);
    let before_second = call(&mut host, json!({ "kind": "annotations.list", "page": second }));

    // The second page fails after the first was written: a cancel at its
    // check (the call's, then the first page's, then the second's).
    let key = SessionKey::document(DOC);
    let pages: Vec<epdf_engine::identity::PageObjectNumber> = [&first, &second]
        .iter()
        .map(|page| {
            epdf_engine::identity::PageObjectNumber::new(
                page["objectNumber"].as_u64().expect("a number") as u32,
            )
            .expect("a page")
        })
        .collect();
    for batch in ["pages.flatten", "redaction.apply"] {
        let session = host.sessions_mut().session_mut(&key).expect("the session");
        let cancel = CancelToken::cancelled_at_check(3);
        let refused = epdf_engine::features::changes::transaction::in_transaction(
            session,
            batch,
            |session| match batch {
                "pages.flatten" => epdf_engine::features::pages_write::flatten_pages(
                    session,
                    &pages,
                    epdf_engine::features::pages_write::FlattenUsage::Display,
                    &cancel,
                )
                .map(drop),
                _ => epdf_engine::features::redaction::apply(
                    session,
                    &epdf_engine::features::redaction::RedactionScope::Pages {
                        pages: pages.clone(),
                    },
                    &cancel,
                )
                .map(drop),
            },
        );
        assert!(refused.is_err(), "{batch} was not refused");
        assert_eq!(state(&mut host, &first), before, "{batch} left the first page changed");
        assert_eq!(
            call(&mut host, json!({ "kind": "annotations.list", "page": second })),
            before_second,
            "{batch} left the second page changed",
        );
    }

    // And over the wire: a page the scope names that is not there refuses
    // the batch before it writes anything.
    let missing = json!({ "kind": "objectNumber", "objectNumber": 999 });
    let refused = send(
        &mut host,
        json!({ "kind": "pages.flatten", "pages": [first, second, missing], "usage": "display" }),
        vec![],
    )
    .expect_err("no page 999");
    assert_eq!(refused["code"], "NotFound", "{refused}");
    assert_eq!(state(&mut host, &first), before);

    let flattened = call(
        &mut host,
        json!({ "kind": "pages.flatten", "pages": [first, second], "usage": "display" }),
    );
    let statuses: Vec<&Value> =
        flattened["result"]["results"].as_array().unwrap().iter().map(|r| &r["status"]).collect();
    assert_eq!(statuses, [&json!("applied"), &json!("applied")], "{flattened}");
}

#[test]
fn a_write_names_its_layer_artifact_with_the_layers_last_object_number() {
    // `finishMutation` saves the artifact after the commit, and the answer
    // names it (`LayerArtifactWorkerPayload`): its bytes the payload, its
    // size, and the layer's last object number as saved.
    let mut host = Host::new();
    let opened = send(
        &mut host,
        json!({ "kind": "open.layerMemBase", "baseKey": "base", "layer": { "kind": "fresh" } }),
        vec![test_support::fixture(HELLO)],
    )
    .expect("open");
    let pages = call(&mut host, json!({ "kind": "pages.list" }));
    let first = pages["pages"][0]["ref"].clone();
    let request = json!({ "kind": "annotations.create", "jobId": 1, "docId": DOC, "page": first,
        "draft": { "subtype": "square", "box": rect(20.0) } });
    let response = host.call(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    let artifact = &envelope["result"]["artifact"];
    assert_eq!(artifact["payload"], json!(0), "{envelope}");
    assert_eq!(artifact["size"], json!(response.payloads[0].len()), "{envelope}");
    let last = session(&host).last_object_number().unwrap().expect("a layer");
    assert_eq!(artifact["lastObjectNumber"], json!(last), "{envelope}");
    assert!(
        last > opened["lastObjectNumber"].as_u64().unwrap() as u32,
        "the create made no object past the layer's last: {envelope}"
    );

    // With an `artifactPath`, the artifact goes to the file instead
    // (`LayerArtifactFileWorkerPayload`), and no payload carries it.
    let path = std::env::temp_dir().join(format!("write-tx-{}.layer", std::process::id()));
    let mut request = request.clone();
    request["artifactPath"] = json!(path.to_string_lossy());
    let response = host.call(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(response.payloads.is_empty(), "{envelope}");
    assert_eq!(
        envelope["result"]["artifactFile"],
        json!({ "path": path.to_string_lossy(),
            "lastObjectNumber": session(&host).last_object_number().unwrap() }),
        "{envelope}"
    );
    assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 0), "no artifact was written");
    let _ = std::fs::remove_file(&path);

    // A session opened from bytes persists no artifact: its caller holds
    // the document.
    let (mut bytes_host, first) = open();
    let response = bytes_host.call(
        &json!({ "kind": "annotations.create", "jobId": 1, "docId": DOC, "page": first,
            "draft": { "subtype": "square", "box": rect(20.0) } })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope["result"].get("artifact").is_none(), "{envelope}");
    assert!(response.payloads.is_empty(), "{envelope}");
}

#[test]
fn a_commit_past_the_ceiling_is_layer_full_and_leaves_the_document_as_it_was() {
    // `commitTransaction` refuses a document past `OBJECT_NUMBER_CEILING`
    // with `LayerFull` and the layer's last object number; the abort takes
    // the write back. The floor is raised inside the transaction, and a
    // raise outlives an abort, so the layer's last object number stays.
    let (mut host, first) = open();
    let before = state(&mut host, &first);
    let ceiling = epdf_engine::identity::OBJECT_NUMBER_CEILING;
    let refused = send(
        &mut host,
        json!({ "kind": "annotations.create", "page": first, "objectNumberFloor": ceiling,
            "draft": { "subtype": "square", "box": rect(20.0) } }),
        vec![],
    )
    .expect_err("past the ceiling");
    assert_eq!(refused["code"], "LayerFull", "{refused}");
    let last = refused["details"]["lastObjectNumber"].as_u64().expect("a last object number");
    assert!(last > u64::from(ceiling), "{refused}");
    assert_eq!(state(&mut host, &first), before, "the refused create left something behind");
}

#[test]
fn a_parked_signing_refuses_a_write_before_its_floor_is_raised() {
    // `WorkerHost.run`: `assertNoPendingSigning` before the transaction
    // opens and before the floor moves.
    let mut host = Host::new();
    let doc = common_open_signing(&mut host);
    let last = |host: &Host| {
        host.sessions()
            .session(&SessionKey::document(doc.as_str()))
            .unwrap()
            .last_object_number()
            .unwrap()
            .unwrap()
    };
    let before = last(&host);
    let request = json!({ "kind": "metadata.update", "jobId": 1, "docId": doc,
        "objectNumberFloor": before + 1000, "patch": { "title": "frozen" } });
    let response = host.call(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], "SigningPending", "{envelope}");
    assert_eq!(last(&host), before, "the floor moved for a refused write");
}

/// Open the unsigned signature field fixture and park a signing on it.
fn common_open_signing(host: &mut Host) -> String {
    let doc = "signing".to_string();
    let bytes = test_support::fixture("packages/engine/main/test/fixtures/unsigned_sigfield.pdf");
    let response = host
        .call(&json!({ "kind": "open.fatMem", "jobId": 1, "docId": doc }).to_string(), vec![bytes]);
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{envelope}");
    let response = host.call(
        &json!({ "kind": "signatures.list", "jobId": 2, "docId": doc }).to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    let field = envelope["result"]["snapshot"]["signatures"][0]["field"].clone();
    let response = host.call(
        &json!({ "kind": "signatures.prepare", "jobId": 3, "docId": doc,
            "input": { "field": field, "signer": { "name": "Test Signer" } } })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{envelope}");
    doc
}

#[test]
fn the_save_comparison_blanks_the_whole_id_array_and_the_size() {
    let trailer = b"<</Root 1 0 R /Size 25/ID[<AB><CD>]>>\nstartxref\n12";
    assert_eq!(comparable(trailer), "<</Root 1 0 R /Size _/ID _>>\nstartxref\n12");
    assert_eq!(comparable(b"/Size 7 >>"), "/Size _ >>");
}

#[test]
fn a_repair_whose_commit_is_refused_leaves_the_sequence_as_it_was() {
    // `finishMutation` counts a write once, after its commit (`noteEdit`):
    // a repair refused at its commit moved nothing.
    let (mut host, _) = open();
    let before = session(&host).mutation_sequence();
    session(&host).force_commit_failure(CommitFailure::InTransactionCheck);
    send(&mut host, json!({ "kind": "forms.repair" }), vec![]).expect_err("refused at its commit");
    assert_eq!(session(&host).mutation_sequence(), before);

    let repaired = call(&mut host, json!({ "kind": "forms.repair" }));
    assert_eq!(repaired["mutationSequence"], json!(before + 1), "{repaired}");
}

#[test]
fn a_refused_numbered_create_leaves_its_number_held() {
    // `abortTransaction`: the numbers a create claimed are free again, and
    // still the session's.
    let mut host = Host::new();
    let opened = send(
        &mut host,
        json!({ "kind": "open.fatMem", "reserveObjectNumbers": 4 }),
        vec![test_support::fixture(HELLO)],
    )
    .expect("open");
    let number = opened["objectNumbers"]["first"].as_u64().expect("a reserved range") as u32;
    let first = call(&mut host, json!({ "kind": "pages.list" }))["pages"][0]["ref"].clone();
    let create = json!({ "kind": "annotations.create", "page": first, "objectNumber": number,
        "draft": { "subtype": "square", "box": rect(20.0) } });

    session(&host).force_commit_failure(CommitFailure::InTransactionCheck);
    send(&mut host, create.clone(), vec![]).expect_err("refused at its commit");
    assert!(session(&host).holds_object_number(number), "the refused create spent its number");

    let created = call(&mut host, create);
    assert_eq!(created["result"]["annotation"]["ref"]["objectNumber"], json!(number));
    assert!(!session(&host).holds_object_number(number), "the commit did not spend it");
}
