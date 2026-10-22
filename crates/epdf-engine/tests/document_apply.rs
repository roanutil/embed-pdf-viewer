//! The change queue, `document.apply` and `document.applyChanges`, on the
//! metadata ops.
//!
//! Ports the cases of upstream's change conformance
//! (`packages/engine/core/src/conformance/runChangeConformance.ts`, run by
//! `packages/engine/main/test/changes.test.ts`) that need only the ops this
//! engine runs, with metadata ops in place of the annotation and form ops
//! upstream's spell a case with where the case is about the queue rather
//! than the op.

mod common;

use epdf_engine::host::Host;
use serde_json::{Value, json};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/rich_metadata.pdf";
const DOC: &str = "d";

fn open() -> Host {
    let mut host = Host::new();
    common::open_fixture(&mut host, FIXTURE);
    host
}

/// Send `request` to the document and answer its `result` or its `error`.
fn send(host: &mut Host, mut request: Value) -> Result<Value, Value> {
    request["jobId"] = json!(7);
    request["docId"] = json!(DOC);
    let response = host.call_filled(&request.to_string(), Vec::new());
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope["result"].take()),
    }
}

/// `doc.apply(change, { opId })`: the whole answer (`result`, `replayed`,
/// `mutationSequence`, `events`), or the refusal.
fn apply(host: &mut Host, change: Value, op_id: Option<&str>) -> Result<Value, Value> {
    let mut request = json!({ "kind": "document.apply", "effect": "write", "change": change });
    if let Some(op_id) = op_id {
        request["opId"] = json!(op_id);
    }
    send(host, request)
}

/// [`apply`], which must apply: answers the `ChangeResult`.
fn applied(host: &mut Host, change: Value) -> Value {
    apply(host, change, None).unwrap_or_else(|e| panic!("the change was refused: {e}"))["result"]
        .take()
}

/// Everything a metadata change can write, as the document reads it
/// (`factsOf`).
fn facts(host: &mut Host) -> Value {
    json!({
        "metadata": send(host, json!({ "kind": "metadata.read" })).expect("read")["metadata"],
        "custom": send(host, json!({ "kind": "metadata.readCustom" })).expect("read custom")
            ["custom"],
    })
}

fn update(patch: Value) -> Value {
    json!({ "type": "metadata.update", "patch": patch })
}

fn update_custom(patch: Value) -> Value {
    json!({ "type": "metadata.updateCustom", "patch": patch })
}

fn item_types(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["type"].as_str().expect("a type").to_owned())
        .collect()
}

/// `${type}: undo gives the state before, the undo's undo the state after`,
/// for both metadata ops.
#[test]
fn each_metadata_op_undoes_to_the_state_before_and_redoes_to_the_state_after() {
    for (kind, op) in [
        ("metadata.update", update(json!({ "title": "Changed", "subject": "Undo" }))),
        ("metadata.updateCustom", update_custom(json!({ "project": "apply" }))),
    ] {
        let mut host = open();
        let before = facts(&mut host);

        let done = applied(&mut host, json!({ "ops": [op] }));
        assert_eq!(done["meta"]["undoable"], true, "{kind}: {done}");
        assert_eq!(item_types(&done), [kind]);
        let after = facts(&mut host);
        assert_ne!(after, before, "{kind} changed nothing");

        let undo = applied(&mut host, json!({ "undoOf": done["meta"]["opId"] }));
        assert_eq!(undo["meta"]["undoable"], true, "{kind}: {undo}");
        assert!(
            undo["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "skipped" && item.get("skipped").is_none()),
            "{kind}: {undo}"
        );
        assert_eq!(facts(&mut host), before, "{kind}: the undo left another state");

        applied(&mut host, json!({ "undoOf": undo["meta"]["opId"] }));
        assert_eq!(facts(&mut host), after, "{kind}: the redo left another state");
    }
}

#[test]
fn ops_apply_in_order_as_one_write_and_publish_one_burst() {
    let mut host = open();
    let answer = apply(
        &mut host,
        json!({ "ops": [
            update(json!({ "title": "First" })),
            update_custom(json!({ "Project": "Second" })),
            update(json!({ "title": "Third" })),
        ] }),
        Some("change-conformance-order"),
    )
    .expect("the change applies");
    let result = &answer["result"];
    assert_eq!(result["meta"]["opId"], "change-conformance-order");
    assert_eq!(result["meta"]["undoable"], true);
    assert_eq!(result["meta"]["affectedPages"], json!([]));
    assert_eq!(item_types(result), ["metadata.update", "metadata.updateCustom", "metadata.update"]);
    // A later op meets the document as the earlier ones left it.
    assert_eq!(result["items"][2]["metadata"]["title"], "Third");
    assert_eq!(result["items"][1]["custom"]["Project"], "Second");
    for item in result["items"].as_array().unwrap() {
        assert_eq!(item["meta"]["opId"], "change-conformance-order", "{item}");
        assert_eq!(item["meta"]["undoable"], true, "{item}");
    }

    // One burst: each op's event in op order, all carrying the change's opId.
    let events = answer["events"].as_array().expect("events");
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["metadata.updated", "metadata.customUpdated", "metadata.updated"]);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            event["origin"]["tx"],
            json!({ "id": "change-conformance-order", "index": index, "count": 3 })
        );
        assert!(event["origin"].get("undoOf").is_none(), "{event}");
    }
    assert_eq!(answer["replayed"], false);
}

#[test]
fn a_failing_op_leaves_nothing() {
    // The first op writes; the second is refused; the first is taken back.
    let mut host = open();
    let before = facts(&mut host);
    let refused = apply(
        &mut host,
        json!({ "ops": [
            update(json!({ "title": "Changed", "createdAt": null })),
            update_custom(json!({ "Project": "x" })),
            update_custom(json!({ "Title": "shadow" })),
        ] }),
        None,
    )
    .expect_err("a standard key as a custom one is refused");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(facts(&mut host), before);
}

#[test]
fn an_empty_change_writes_nothing_and_is_not_undoable() {
    let mut host = open();
    let answer = apply(&mut host, json!({ "ops": [] }), None).expect("an empty change applies");
    assert_eq!(answer["result"]["items"], json!([]));
    assert_eq!(answer["result"]["meta"]["undoable"], false);
    assert_eq!(answer["events"], json!([]));
    assert_eq!(answer["mutationSequence"], 0, "nothing was written: {answer}");
}

#[test]
fn expect_refuses_the_whole_change_with_change_conflict() {
    let mut host = open();
    let before = facts(&mut host);
    let refused = apply(
        &mut host,
        json!({ "ops": [
            update_custom(json!({ "Project": "x" })),
            { "type": "metadata.update", "patch": { "title": "Changed" },
              "expect": { "title": "Not the title" } },
        ] }),
        None,
    )
    .expect_err("the expectation fails");
    assert_eq!(refused["code"], "ChangeConflict", "{refused}");
    assert_eq!(refused["details"], json!({ "opIndex": 1, "fields": ["title"] }));
    assert_eq!(facts(&mut host), before);

    // An expectation the document meets lets the change through.
    let title = before["metadata"]["title"].clone();
    applied(
        &mut host,
        json!({ "ops": [{ "type": "metadata.update", "patch": { "title": "Changed" },
            "expect": { "title": title, "keywords": null } }] }),
    );
}

#[test]
fn undo_leaves_a_colleagues_later_change_alone_and_reports_it() {
    let mut host = open();
    let before = facts(&mut host);
    let mine = applied(
        &mut host,
        json!({ "ops": [update(json!({ "title": "Mine", "subject": "Mine" }))] }),
    );
    applied(&mut host, json!({ "ops": [update(json!({ "title": "Theirs" }))] }));

    let undo = applied(&mut host, json!({ "undoOf": mine["meta"]["opId"] }));
    assert_eq!(item_types(&undo), ["metadata.update"]);
    assert_eq!(undo["items"][0]["skipped"], json!(["title"]), "{undo}");
    let after = facts(&mut host);
    assert_eq!(after["metadata"]["title"], "Theirs");
    assert_eq!(after["metadata"]["subject"], before["metadata"]["subject"]);
}

#[test]
fn undo_goes_by_value() {
    // A field set back to what the change set is undone.
    let mut host = open();
    let before = facts(&mut host);
    let mine = applied(&mut host, json!({ "ops": [update(json!({ "title": "Mine" }))] }));
    applied(&mut host, json!({ "ops": [update(json!({ "title": "Other" }))] }));
    applied(&mut host, json!({ "ops": [update(json!({ "title": "Mine" }))] }));

    let undo = applied(&mut host, json!({ "undoOf": mine["meta"]["opId"] }));
    assert!(undo["items"][0].get("skipped").is_none(), "{undo}");
    assert_eq!(facts(&mut host), before);
}

#[test]
fn undoing_a_change_twice_is_harmless() {
    let mut host = open();
    let before = facts(&mut host);
    let mine = applied(&mut host, json!({ "ops": [update_custom(json!({ "Project": "x" }))] }));
    applied(&mut host, json!({ "undoOf": mine["meta"]["opId"] }));
    let again = apply(&mut host, json!({ "undoOf": mine["meta"]["opId"] }), None).expect("again");
    let items = again["result"]["items"].as_array().unwrap();
    assert!(!items.is_empty() && items.iter().all(|item| item["type"] == "skipped"), "{again}");
    assert_eq!(items[0]["op"], "metadata.updateCustom");
    assert_eq!(again["events"], json!([]), "an undo that wrote nothing publishes nothing");
    assert_eq!(facts(&mut host), before);
}

#[test]
fn an_undo_carries_the_op_id_it_undid() {
    let mut host = open();
    let mine = applied(&mut host, json!({ "ops": [update(json!({ "title": "Mine" }))] }));
    let undo = apply(&mut host, json!({ "undoOf": mine["meta"]["opId"] }), Some("undo-1"))
        .expect("the undo applies");
    assert_eq!(undo["events"][0]["origin"]["undoOf"], mine["meta"]["opId"]);
    assert_eq!(undo["events"][0]["origin"]["tx"]["id"], "undo-1");
}

#[test]
fn a_refused_change_asked_again_answers_its_refusal_even_when_it_would_now_apply() {
    let mut host = open();
    let refused = json!({ "ops": [{ "type": "metadata.update", "patch": { "title": "Again" },
        "expect": { "title": "Mine" } }] });
    let first = apply(&mut host, refused.clone(), Some("change-conformance-refused"))
        .expect_err("the expectation fails");
    assert_eq!(first["code"], "ChangeConflict");
    applied(&mut host, json!({ "ops": [update(json!({ "title": "Mine" }))] }));
    let second = apply(&mut host, refused, Some("change-conformance-refused"))
        .expect_err("the refusal is kept");
    assert_eq!(second["code"], "ChangeConflict");
    assert_eq!(facts(&mut host)["metadata"]["title"], "Mine");
}

#[test]
fn asking_again_answers_the_first_result_and_publishes_nothing_new() {
    let mut host = open();
    let change = json!({ "ops": [update(json!({ "title": "Once" }))] });
    let first = apply(&mut host, change.clone(), Some("change-conformance-retry")).unwrap();
    let second = apply(&mut host, change, Some("change-conformance-retry")).unwrap();
    assert_eq!(second["result"], first["result"]);
    assert_eq!(first["replayed"], false);
    assert_eq!(second["replayed"], true);
    assert_eq!(first["events"].as_array().unwrap().len(), 1);
    assert_eq!(second["events"], json!([]));
    assert_eq!(second["mutationSequence"], first["mutationSequence"], "a retry wrote again");
}

#[test]
fn a_different_change_under_a_used_op_id_is_idempotency_key_reused() {
    let mut host = open();
    apply(&mut host, json!({ "ops": [update(json!({ "title": "Bea" }))] }), Some("reused"))
        .unwrap();
    let refused =
        apply(&mut host, json!({ "ops": [update(json!({ "title": "Cy" }))] }), Some("reused"))
            .expect_err("another change under the same opId");
    assert_eq!(refused["code"], "IdempotencyKeyReused", "{refused}");
    // The same change with its keys in another order is the same change.
    let same = json!({ "ops": [{ "patch": { "title": "Bea" }, "type": "metadata.update" }] });
    assert_eq!(apply(&mut host, same, Some("reused")).unwrap()["replayed"], true);
}

#[test]
fn a_single_verb_is_a_one_op_change_whose_op_id_undoes_it() {
    let mut host = open();
    let before = facts(&mut host);
    for request in [
        json!({ "kind": "metadata.update", "patch": { "title": "Verb" } }),
        json!({ "kind": "metadata.updateCustom", "patch": { "Verb": "yes" } }),
    ] {
        let updated = send(&mut host, request).expect("the verb applies");
        assert_eq!(updated["result"]["meta"]["undoable"], true, "{updated}");
        let op_id = updated["result"]["meta"]["opId"].as_str().expect("a minted opId");
        assert!(!op_id.is_empty());
        applied(&mut host, json!({ "undoOf": op_id }));
    }
    assert_eq!(facts(&mut host), before);
}

#[test]
fn an_op_id_that_is_not_visible_ascii_is_refused() {
    let mut host = open();
    for op_id in ["", "has space", "é"] {
        let refused = apply(&mut host, json!({ "ops": [] }), Some(op_id)).expect_err(op_id);
        assert_eq!(refused["code"], "InvalidArg", "{op_id}: {refused}");
        assert_eq!(refused["details"]["field"], "opId");
    }
}

#[test]
fn a_change_this_session_never_answered_cannot_be_undone() {
    let mut host = open();
    let refused = apply(&mut host, json!({ "undoOf": "nobody-made-this" }), None)
        .expect_err("nothing to undo");
    assert_eq!(refused["code"], "UndoUnavailable", "{refused}");
    assert_eq!(refused["details"]["reason"], "expired");
}

#[test]
fn an_op_of_the_wrong_shape_refuses_the_change_before_anything_runs() {
    let mut host = open();
    let before = facts(&mut host);
    let refused = apply(
        &mut host,
        json!({ "ops": [update(json!({ "title": "Changed" })),
            { "type": "forms.setSignatureAppearance" }] }),
        None,
    )
    .expect_err("a signature appearance names its field and its PDF");
    assert_eq!(refused["code"], "WireFormat", "{refused}");
    assert_eq!(facts(&mut host), before);
}

#[test]
fn only_the_user_who_made_a_change_may_undo_it() {
    let mut host = open();
    let as_user = |user: &str| json!({ "identity": { "userId": user }, "grants": null });
    let mine = send(
        &mut host,
        json!({ "kind": "document.apply", "change": { "ops": [update(json!({ "title": "Ann's" }))] },
            "authority": as_user("ann") }),
    )
    .expect("ann's change")["result"]
        .take();
    let refused = send(
        &mut host,
        json!({ "kind": "document.apply", "change": { "undoOf": mine["meta"]["opId"] },
            "authority": as_user("bea") }),
    )
    .expect_err("bea may not undo ann's change");
    assert_eq!(refused["code"], "Forbidden", "{refused}");
    assert_eq!(facts(&mut host)["metadata"]["title"], "Ann's");
    send(
        &mut host,
        json!({ "kind": "document.apply", "change": { "undoOf": mine["meta"]["opId"] },
            "authority": as_user("ann") }),
    )
    .expect("ann may");
}

#[test]
fn each_op_is_checked_against_the_changes_authority() {
    let mut host = open();
    let bits = json!({ "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
        "bit10": true, "bit11": true, "bit12": true });
    let refused = send(
        &mut host,
        json!({ "kind": "document.apply", "change": { "ops": [update(json!({ "title": "x" }))] },
            "authority": { "identity": {}, "grants": { "scope": ["doc.open"], "pdfBits": bits } } }),
    )
    .expect_err("doc.open does not grant a metadata write");
    assert_eq!(refused["code"], "Forbidden", "{refused}");
}

#[test]
fn a_server_request_answers_one_outcome_per_change() {
    // `document.applyChanges`: each change in its own transaction, a kept
    // refusal its own answer, and an undo of an earlier change of the same
    // request runs the record this job kept for it.
    let mut host = open();
    let before = facts(&mut host);
    let authority = json!({ "identity": { "userId": "ann" }, "grants": null });
    let answer = send(
        &mut host,
        json!({ "kind": "document.applyChanges", "effect": "write", "changes": [
            { "opId": "a1", "authority": authority,
              "change": { "ops": [update(json!({ "title": "Server" }))] } },
            { "opId": "a2", "authority": authority,
              "change": { "ops": [update(json!({ "subject": "kept" })),
                update_custom(json!({ "/Leading": "x" }))] } },
            { "opId": "a3", "authority": authority, "change": { "undoOf": "a1" } },
        ] }),
    )
    .expect("the job runs");
    let outcomes = answer["outcomes"].as_array().expect("outcomes");
    let statuses: Vec<&str> = outcomes.iter().map(|o| o["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["applied", "refused", "applied"]);
    assert_eq!(outcomes[0]["opId"], "a1");
    assert_eq!(outcomes[0]["record"]["userId"], "ann");
    assert_eq!(outcomes[0]["record"]["steps"][0]["kind"], "metadata.revert");
    assert_eq!(outcomes[1]["error"]["code"], "InvalidArg");
    assert_eq!(outcomes[2]["result"]["items"][0]["type"], "metadata.update");
    assert_eq!(facts(&mut host), before, "a2 left a write behind, or a3 did not undo a1");
    // Upstream publishes nothing for a server request: the server relays its
    // changes from the audit log.
    assert!(answer.get("events").is_none(), "{answer}");
    assert!(answer["mutationSequence"].is_u64(), "{answer}");
    assert_eq!(answer.as_object().unwrap().len(), 2, "{answer}");

    // An undo of a change outside the request runs the record the caller
    // kept, and one that left none undoes nothing.
    let record = outcomes[0]["record"].clone();
    let redo = send(
        &mut host,
        json!({ "kind": "document.applyChanges", "changes": [
            { "opId": "b1", "authority": authority, "change": { "undoOf": "a3" },
              "record": outcomes[2]["record"] },
            { "opId": "b2", "authority": authority, "change": { "undoOf": "elsewhere" },
              "record": null },
            { "opId": "b3", "authority": authority, "change": { "undoOf": "unsent" } },
        ] }),
    )
    .expect("the job runs");
    let outcomes = redo["outcomes"].as_array().unwrap();
    assert_eq!(outcomes[0]["status"], "applied", "{redo}");
    assert_eq!(facts(&mut host)["metadata"]["title"], "Server");
    assert_eq!(outcomes[1]["result"]["items"], json!([]));
    assert_eq!(outcomes[2]["error"]["code"], "UndoUnavailable");
    assert!(record["steps"].is_array());
    assert!(redo.get("events").is_none(), "{redo}");
}

#[test]
fn a_server_change_holding_an_import_runs_it_and_document_apply_refuses_it() {
    // `document.apply` refuses an import as its own verb's (`assertNoImport`);
    // a server request's change may hold one, which runs as its op: a bundle
    // that isn't one is the change's kept refusal.
    let mut host = open();
    let import = json!({ "type": "forms.importValues", "bundle": {}, "attribution": "stamp" });
    let answered = send(
        &mut host,
        json!({ "kind": "document.applyChanges", "changes": [
            { "opId": "i1", "authority": { "identity": {}, "grants": null },
              "change": { "ops": [import] } },
        ] }),
    )
    .expect("the job runs");
    let outcome = &answered["outcomes"][0];
    assert_eq!(outcome["status"], "refused", "{answered}");
    assert_eq!(outcome["error"]["code"], "InvalidArg", "{answered}");
    assert!(outcome["error"]["message"].as_str().unwrap().contains("form bundle"), "{answered}");

    let refused = apply(&mut host, json!({ "ops": [import] }), None).expect_err("no import");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("doc.forms.importValues"), "{refused}");
}

#[test]
fn an_undo_lists_what_it_skipped_in_the_order_the_patch_named_it() {
    // `Object.keys(op.patch)`: the wire's key order, not the alphabet.
    let mut host = open();
    let mine = applied(
        &mut host,
        json!({ "ops": [
            update(json!({ "title": "Mine", "author": "Mine", "subject": "Mine" })),
            update_custom(json!({ "Zed": "mine", "Alpha": "mine", "Mid": "mine" })),
        ] }),
    );
    applied(
        &mut host,
        json!({ "ops": [
            update(json!({ "author": "Theirs", "title": "Theirs" })),
            update_custom(json!({ "Alpha": "theirs", "Zed": "theirs" })),
        ] }),
    );
    let undo = applied(&mut host, json!({ "undoOf": mine["meta"]["opId"] }));
    // The undo runs the reverses in reverse op order: the custom op first.
    assert_eq!(item_types(&undo), ["metadata.updateCustom", "metadata.update"]);
    assert_eq!(undo["items"][0]["skipped"], json!(["Zed", "Alpha"]), "{undo}");
    assert_eq!(undo["items"][1]["skipped"], json!(["title", "author"]), "{undo}");
    let after = facts(&mut host);
    assert_eq!(after["metadata"]["subject"], "");
    assert!(after["custom"].get("Mid").is_none(), "{after}");
}

#[test]
fn a_final_change_ends_undo_for_the_changes_before_it() {
    // `FINAL_CHANGES`: an applied `annotations.flatten` ends undo for every
    // change before it, and plain edits after it stay undoable.
    const APPEARANCES: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
    let mut host = Host::new();
    let (doc, page) = common::open_fixture(&mut host, APPEARANCES);
    assert_eq!(doc, DOC);
    let before = applied(&mut host, json!({ "ops": [update(json!({ "title": "Before" }))] }));

    let listed = common::call(
        &mut host,
        json!({ "kind": "annotations.list", "docId": DOC, "pages": [common::page_ref(page)] }),
    );
    let target = listed["annotations"]
        .as_array()
        .and_then(|all| all.first())
        .unwrap_or_else(|| panic!("no annotation on the page: {listed}"))["ref"]
        .clone();
    let flattened = send(
        &mut host,
        json!({ "kind": "annotations.flatten", "page": common::page_ref(page),
            "refs": [target], "usage": "display" }),
    )
    .expect("the flatten applies");
    assert_eq!(flattened["result"]["results"][0]["status"], "applied", "{flattened}");

    let refused = apply(&mut host, json!({ "undoOf": before["meta"]["opId"] }), None)
        .expect_err("the flatten ended undo");
    assert_eq!(refused["code"], "UndoUnavailable", "{refused}");
    assert_eq!(refused["details"], json!({ "reason": "final-change" }));

    let after = applied(&mut host, json!({ "ops": [update(json!({ "title": "After" }))] }));
    assert_eq!(after["meta"]["undoable"], true);
    applied(&mut host, json!({ "undoOf": after["meta"]["opId"] }));
}
