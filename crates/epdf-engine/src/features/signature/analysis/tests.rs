//! `packages/engine/core/test/signature/analysis.test.ts` and
//! `resolve.test.ts`: the rule engine on hand-built steps, case for case.
//!
//! The inputs are built as the TypeScript builds them, as JSON (`change`,
//! `signature`, `structure`), and read into the Rust types, so each case
//! reads as its TypeScript does. Results are compared as JSON.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::types::{ObjectChange, ObjectReferrer, RevisionStructure, Sides, StepInput};
use super::{
    DEFAULT_EDGE_RESOLVER_BUDGET, EdgeResolver, EdgeResolverBudget, evaluate_step, parse_pdf_value,
    restrictions_of, stable_stringify,
};
use crate::features::signature::analysis::pdf_value::changed_keys;

// ── parsePdfValue ─────────────────────────────────────────────────────────

#[test]
fn parse_pdf_value_reads_the_forks_shallow_serialisation() {
    let v = parse_pdf_value(
        "<</A 1/B [2 0 R 3 -4.5 true null]/C (hi \\(there\\))/D /Na#20me/E <</F 6 0 R>>>>",
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&v).unwrap(),
        json!({ "t": "dict", "entries": {
            "A": { "t": "number", "v": 1 },
            "B": { "t": "array", "items": [
                { "t": "ref", "num": 2 },
                { "t": "number", "v": 3 },
                { "t": "number", "v": -4.5 },
                { "t": "bool", "v": true },
                { "t": "null" },
            ] },
            "C": { "t": "string", "v": "hi (there)" },
            "D": { "t": "name", "v": "Na me" },
            "E": { "t": "dict", "entries": { "F": { "t": "ref", "num": 6 } } },
        } })
    );
    let s = parse_pdf_value("stream(12,abcd)<</Length 12/Type /XObject>>").unwrap();
    assert_eq!(
        serde_json::to_value(&s).unwrap(),
        json!({ "t": "stream", "length": 12, "sha256": "abcd",
            "dict": { "Length": { "t": "number", "v": 12 }, "Type": { "t": "name", "v": "XObject" } } })
    );
    let text = stable_stringify(Some(&v));
    assert_eq!(text, stable_stringify(Some(&parse_pdf_value(&text).unwrap())));
    let mut keys = changed_keys(
        Some(&parse_pdf_value("<</A 1/B 2>>").unwrap()),
        Some(&parse_pdf_value("<</A 1/B 3/C 4>>").unwrap()),
    );
    keys.sort();
    assert_eq!(keys, ["B", "C"]);
}

// ── builders ──────────────────────────────────────────────────────────────

fn edge(parent: u32, label: &str) -> Value {
    json!({ "parent": parent, "label": label })
}

fn edge_via(parent: u32, label: &str, via: &[u32]) -> Value {
    json!({ "parent": parent, "label": label, "via": via })
}

/// `change`: one changed object, its kind read off its raw text, its
/// values parsed from it.
fn change(object_number: u32, old: Option<&str>, new: Option<&str>, usage: Usage) -> Value {
    let kind_of = |s: &str| {
        if s.starts_with("stream(") {
            "stream"
        } else if s.starts_with("<<") {
            "dictionary"
        } else if s.starts_with('[') {
            "array"
        } else {
            "scalar"
        }
    };
    let value = |raw: Option<&str>| {
        raw.map_or(Value::Null, |raw| serde_json::to_value(parse_pdf_value(raw).unwrap()).unwrap())
    };
    let generation = |raw: Option<&str>| {
        if raw.is_none() || object_number == 0 { Value::Null } else { json!(0) }
    };
    json!({
        "objectNumber": object_number,
        "change": if old.is_none() { "added" } else if new.is_none() { "freed" } else { "modified" },
        "kind": if object_number == 0 { "trailer" } else { kind_of(new.or(old).unwrap_or("")) },
        "present": { "old": old.is_some(), "new": new.is_some() },
        "generation": { "old": generation(old), "new": generation(new) },
        "value": { "old": value(old), "new": value(new), "truncated": false },
        "raw": { "old": old, "new": new },
        "streamDataChanged": false,
        "usage": { "old": usage.old, "new": usage.new },
    })
}

/// `{ old?, new? }` usage of a change.
#[derive(Default)]
struct Usage {
    old: Vec<Value>,
    new: Vec<Value>,
}

fn both(edges: &[Value]) -> Usage {
    Usage { old: edges.to_vec(), new: edges.to_vec() }
}

fn old_only(edges: &[Value]) -> Usage {
    Usage { old: edges.to_vec(), new: Vec::new() }
}

fn new_only(edges: &[Value]) -> Usage {
    Usage { old: Vec::new(), new: edges.to_vec() }
}

/// `{ ...change, ...extra }`.
fn with(mut change: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        change[k] = v.clone();
    }
    change
}

/// `signature(index, fieldObjectNumber, extra)`: a signed approval.
fn signature(index: u32, field_object_number: u32, extra: Value) -> Value {
    let mut sig = json!({
        "index": index,
        "field": { "kind": "objectNumber", "objectNumber": field_object_number },
        "fieldName": format!("sig{index}"),
        "widget": null,
        "signed": true,
        "kind": "signature",
        "filter": "Adobe.PPKLite",
        "subFilter": "ETSI.CAdES.detached",
        "byteRange": [0, 100, 200, 100],
        "contentsSize": 8,
        "coverage": "whole-revision",
        "revisionIndex": 1,
        "signer": { "name": null, "reason": null, "location": null, "contactInfo": null,
            "signedAt": null },
        "docMdp": null,
        "catalogCertification": false,
        "fieldMdp": null,
        "lock": null,
        "seedValue": null,
    });
    for (k, v) in extra.as_object().unwrap() {
        sig[k] = v.clone();
    }
    sig
}

fn approval() -> Value {
    signature(0, 30, json!({}))
}

fn certification(permission: u8) -> Value {
    signature(0, 30, json!({ "docMdp": permission, "catalogCertification": true }))
}

fn unsigned(index: u32, field: u32, extra: Value) -> Value {
    let mut sig = signature(
        index,
        field,
        json!({ "signed": false, "revisionIndex": null, "coverage": null, "byteRange": null }),
    );
    for (k, v) in extra.as_object().unwrap() {
        sig[k] = v.clone();
    }
    sig
}

fn base_fields() -> Value {
    json!([
        { "objectNumber": 20, "name": "name", "family": "text", "widgets": [21] },
        { "objectNumber": 30, "name": "sig0", "family": "signature", "widgets": [30] },
    ])
}

/// `structure(overrides)`: catalog 1, pages 2, page 3, AcroForm 10, text
/// field 20 (widget 21), signature field 30 (merged widget).
fn structure(overrides: Value) -> Value {
    let mut s = json!({
        "root": 1,
        "acroForm": 10,
        "pagesRoot": 2,
        "pages": [3],
        "fields": base_fields(),
        "signatures": [approval()],
    });
    for (k, v) in overrides.as_object().unwrap() {
        s[k] = v.clone();
    }
    s
}

fn signed_with(signatures: Value) -> Value {
    structure(json!({ "signatures": signatures }))
}

fn healthy() -> Value {
    json!({ "sparseXref": false, "bareReferences": 0, "referrersComplete": true })
}

/// `evaluateStep({ older, newer, changes, before, after, health })`, as JSON.
fn step(older: u32, newer: u32, changes: &[Value], before: &Value, after: &Value) -> Value {
    step_with(older, newer, changes, before, after, None)
}

fn step_with(
    older: u32,
    newer: u32,
    changes: &[Value],
    before: &Value,
    after: &Value,
    health: Option<(Value, Value)>,
) -> Value {
    let changes: Vec<ObjectChange> =
        changes.iter().map(|c| serde_json::from_value(c.clone()).unwrap()).collect();
    let structure = |v: &Value| -> RevisionStructure { serde_json::from_value(v.clone()).unwrap() };
    let health = health.map(|(old, new)| Sides {
        old: serde_json::from_value(old).unwrap(),
        new: serde_json::from_value(new).unwrap(),
    });
    let analysis = evaluate_step(StepInput {
        older,
        newer,
        changes,
        before: structure(before),
        after: structure(after),
        health,
        restrictions: None,
        level_override: None,
    });
    serde_json::to_value(analysis).unwrap()
}

fn verdict(step: &Value) -> &str {
    step["verdict"].as_str().unwrap()
}

fn findings(step: &Value) -> &Vec<Value> {
    step["findings"].as_array().unwrap()
}

fn rules(step: &Value) -> Vec<&str> {
    findings(step).iter().map(|f| f["rule"].as_str().unwrap()).collect()
}

fn any(step: &Value, test: impl Fn(&Value) -> bool) -> bool {
    findings(step).iter().any(test)
}

fn forbidden(step: &Value) -> Vec<&Value> {
    findings(step).iter().filter(|f| f["verdict"] == "forbidden").collect()
}

fn fill_change() -> Value {
    change(
        20,
        Some("<</FT /Tx/T (name)/V (a)>>"),
        Some("<</FT /Tx/T (name)/V (b)>>"),
        both(&[edge(10, "Fields/[0]")]),
    )
}

// ── evaluateStep: the edge-claim law ──────────────────────────────────────

#[test]
fn restrictions_come_from_the_older_revision() {
    // Nothing signed: nothing forbids. An approval signature: judged the way
    // Acrobat reads it, form fill-in, signing and commenting keep it.
    let of = |v: Value| restrictions_of(&serde_json::from_value(v).unwrap());
    let level = |v: Value| serde_json::to_value(of(v).level).unwrap();
    assert_eq!(level(structure(json!({ "signatures": [] }))), "annotate");
    assert_eq!(level(structure(json!({}))), "annotate");
    assert_eq!(level(signed_with(json!([certification(2)]))), "fill");
    let locked = of(signed_with(json!([signature(
        0,
        30,
        json!({ "fieldMdp": { "action": "include", "fields": ["name"] } })
    )])));
    assert_eq!(locked.locks.len(), 1);
}

#[test]
fn a_failed_read_is_missing_evidence_never_a_value() {
    let before = structure(json!({}));
    // Both sides serialise as "null", but the new side could not be read.
    let unreadable = with(
        change(20, Some("null"), Some("null"), both(&[edge(10, "Fields/[0]")])),
        json!({ "read": { "old": "ok", "new": "failed" } }),
    );
    let s = step(1, 2, &[unreadable], &before, &before);
    assert_eq!(verdict(&s), "indeterminate");
    assert!(!rules(&s).contains(&"identical-rewrite"));
    assert!(any(&s, |f| f["verdict"] == "incomplete"));
    // A real null object rewritten as null, both sides read: an identical
    // rewrite.
    let real_null = with(
        change(20, Some("null"), Some("null"), both(&[edge(10, "Fields/[0]")])),
        json!({ "read": { "old": "ok", "new": "ok" } }),
    );
    assert!(rules(&step(1, 2, &[real_null], &before, &before)).contains(&"identical-rewrite"));
}

#[test]
fn a_proven_violation_decides_even_when_other_evidence_is_incomplete() {
    let before = structure(json!({}));
    let page_content = change(
        3,
        Some("<</Type /Page/Parent 2 0 R/Contents 40 0 R>>"),
        Some("<</Type /Page/Parent 2 0 R/Contents 41 0 R>>"),
        both(&[edge(2, "Kids/[0]")]),
    );
    let too_big = with(
        change(50, Some("<</X 1>>"), Some("<</X 2>>"), both(&[edge(1, "Big")])),
        json!({ "value": { "old": null, "new": null, "truncated": true } }),
    );
    let s = step(1, 2, &[page_content, too_big], &before, &before);
    assert!(any(&s, |f| f["verdict"] == "forbidden"));
    assert!(any(&s, |f| f["verdict"] == "incomplete"));
    assert_eq!(verdict(&s), "forbidden");
}

#[test]
fn a_sealed_revision_acrobat_cannot_verify_is_judged_indeterminate() {
    let before = structure(json!({}));
    let fill = fill_change();
    let run = |old: Value, new: Value| {
        step_with(1, 2, std::slice::from_ref(&fill), &before, &before, Some((old, new)))
    };
    assert_eq!(verdict(&run(healthy(), healthy())), "permitted");
    let mut sparse = healthy();
    sparse["sparseXref"] = json!(true);
    let mut bare = healthy();
    bare["bareReferences"] = json!(1);
    for sick in [sparse.clone(), bare] {
        let s = run(sick, healthy());
        assert_eq!(verdict(&s), "indeterminate");
        let finding = findings(&s).iter().find(|f| f["rule"] == "base-unverifiable").unwrap();
        assert_eq!(finding["verdict"], "incomplete");
        assert_eq!(finding["objectNumber"], 0);
        assert!(finding["detail"].as_str().unwrap().contains("Acrobat"), "{finding}");
    }
    // The newer revision's health is not the sealed revision's problem...
    assert_eq!(verdict(&run(healthy(), sparse)), "permitted");
    // ...but an incomplete reachability walk on either side leaves every
    // use unproven.
    let mut walked_health = healthy();
    walked_health["referrersComplete"] = json!(false);
    let walked = run(healthy(), walked_health);
    assert_eq!(verdict(&walked), "indeterminate");
    assert!(any(&walked, |f| f["rule"] == "unexplained" && f["verdict"] == "incomplete"));
}

#[test]
fn no_changes_is_unchanged_and_under_p1_an_identical_rewrite_is_still_forbidden() {
    let before = signed_with(json!([certification(1)]));
    assert_eq!(verdict(&step(1, 2, &[], &before, &before)), "unchanged");
    let same = change(
        20,
        Some("<</FT /Tx/T (name)/V (x)>>"),
        Some("<</FT /Tx/T (name)/V (x)>>"),
        both(&[edge(10, "Fields/[0]")]),
    );
    let s = step(1, 2, &[same], &before, &before);
    assert_eq!(s["levelInForce"], "lta");
    assert_eq!(verdict(&s), "forbidden");
    assert_eq!(findings(&s)[0]["rule"], "identical-rewrite");
    assert!(
        findings(&s)[0]["detail"].as_str().unwrap().contains("sealed value under level 'lta'"),
        "{s}"
    );
}

fn same_page() -> Value {
    change(
        3,
        Some("<</Type /Page/Parent 2 0 R/Annots [30 0 R]>>"),
        Some("<</Type /Page/Parent 2 0 R/Annots [30 0 R]>>"),
        both(&[edge(2, "Kids/[0]")]),
    )
}

#[test]
fn a_page_rewritten_identically_after_an_approval_signature_is_no_change() {
    let before = structure(json!({}));
    let s = step(1, 2, &[same_page()], &before, &before);
    assert_eq!(s["levelInForce"], "annotate");
    assert_eq!(verdict(&s), "unchanged");
    assert_eq!(findings(&s).len(), 1);
    let only = &findings(&s)[0];
    assert_eq!(
        (&only["rule"], &only["verdict"], &only["objectNumber"]),
        (&json!("identical-rewrite"), &json!("permitted"), &json!(3))
    );
    // Under a P=2 or P=3 certification as well (v3/70, 71).
    for permission in [2, 3] {
        let certified = signed_with(json!([certification(permission)]));
        assert_eq!(verdict(&step(1, 2, &[same_page()], &certified, &certified)), "unchanged");
    }
}

#[test]
fn an_identical_rewrite_before_the_first_signature_is_no_change_either() {
    let unsigned = signed_with(json!([signature(0, 30, json!({ "signed": false }))]));
    let s = step(0, 1, &[same_page()], &unsigned, &unsigned);
    assert_eq!(verdict(&s), "unchanged");
    assert_eq!(findings(&s).len(), 1);
    assert_eq!(findings(&s)[0]["rule"], "identical-rewrite");
    assert_eq!(findings(&s)[0]["verdict"], "permitted");
    assert_eq!(findings(&s)[0]["objectNumber"], 3);
}

#[test]
fn a_form_fill_is_permitted_under_fill_and_annotate_forbidden_under_lta() {
    let fill = [
        fill_change(),
        change(
            21,
            Some("<</AP <</N 22 0 R>>/Subtype /Widget>>"),
            Some("<</AP <</N 40 0 R>>/Subtype /Widget>>"),
            both(&[edge(3, "Annots/[0]"), edge(20, "Kids/[0]")]),
        ),
        change(
            40,
            None,
            Some("stream(20,ff)<</Resources <</Font <</Helv 41 0 R>>>>/Subtype /Form>>"),
            new_only(&[edge(21, "AP/N")]),
        ),
        change(
            41,
            None,
            Some("<</BaseFont /Helvetica/Type /Font>>"),
            new_only(&[edge(40, "Resources/Font/Helv")]),
        ),
        change(22, Some("stream(10,aa)<</Subtype /Form>>"), None, old_only(&[edge(21, "AP/N")])),
        change(
            0,
            Some("<</Root 1 0 R/Size 42>>"),
            Some("<</Prev 1234/Root 1 0 R/Size 43>>"),
            Usage::default(),
        ),
    ];
    let approval = structure(json!({}));
    let permitted = step(1, 2, &fill, &approval, &approval);
    assert_eq!(verdict(&permitted), "permitted");
    assert!(forbidden(&permitted).is_empty());
    let certified_fill = signed_with(json!([certification(2)]));
    assert_eq!(verdict(&step(1, 2, &fill, &certified_fill, &certified_fill)), "permitted");
    let lta = signed_with(json!([certification(1)]));
    let refused = step(1, 2, &fill, &lta, &lta);
    assert_eq!(verdict(&refused), "forbidden");
    assert!(any(&refused, |f| f["rule"] == "form-fill" && f["verdict"] == "forbidden"));
}

#[test]
fn a_shared_resource_keeps_its_other_reference_unexplained() {
    let fill = [
        change(
            21,
            Some("<</AP <</N 22 0 R>>/Subtype /Widget>>"),
            Some("<</AP <</N 40 0 R>>/Subtype /Widget>>"),
            both(&[edge(3, "Annots/[0]")]),
        ),
        change(
            40,
            None,
            Some("stream(20,ff)<</Resources <</Font <</Helv 41 0 R>>>>>>"),
            new_only(&[edge(21, "AP/N")]),
        ),
        // The font is also referenced by the page content resources: not a
        // fill.
        change(
            41,
            None,
            Some("<</Type /Font>>"),
            new_only(&[edge(40, "Resources/Font/Helv"), edge(3, "Resources/Font/Helv")]),
        ),
    ];
    let s = structure(json!({}));
    let analysed = step(1, 2, &fill, &s, &s);
    assert_eq!(verdict(&analysed), "forbidden");
    let unexplained = findings(&analysed)
        .iter()
        .find(|f| f["rule"] == "unexplained" && f["objectNumber"] == 41)
        .unwrap();
    assert_eq!(unexplained["edge"], "3:Resources/Font/Helv");
}

#[test]
fn a_page_content_change_a_catalog_change_and_a_deleted_page_are_forbidden() {
    let s = structure(json!({}));
    let content = [with(
        change(
            4,
            Some("stream(10,aa)<</Length 10>>"),
            Some("stream(12,bb)<</Length 12>>"),
            both(&[edge(3, "Contents")]),
        ),
        json!({ "streamDataChanged": true }),
    )];
    assert_eq!(verdict(&step(1, 2, &content, &s, &s)), "forbidden");
    let catalog = [change(
        1,
        Some("<</Pages 2 0 R/Type /Catalog>>"),
        Some("<</Pages 2 0 R/Type /Catalog/OpenAction 9 0 R>>"),
        both(&[edge(0, "Root")]),
    )];
    let cat = step(1, 2, &catalog, &s, &s);
    assert_eq!(verdict(&cat), "forbidden");
    assert_eq!(findings(&cat)[0]["rule"], "catalog-housekeeping");
    let deleted = [
        change(
            2,
            Some("<</Count 2/Kids [3 0 R 5 0 R]/Type /Pages>>"),
            Some("<</Count 1/Kids [3 0 R]/Type /Pages>>"),
            both(&[edge(1, "Pages")]),
        ),
        change(5, Some("<</Parent 2 0 R/Type /Page>>"), None, old_only(&[edge(2, "Kids/[1]")])),
    ];
    let before = structure(json!({ "pages": [3, 5] }));
    assert_eq!(verdict(&step(1, 2, &deleted, &before, &s)), "forbidden");
}

#[test]
fn metadata_info_trailer_housekeeping_and_dss_are_permitted_even_under_lta() {
    let lta = signed_with(json!([certification(1)]));
    let changes = [
        change(
            1,
            Some("<</Metadata 7 0 R/Pages 2 0 R/Type /Catalog>>"),
            Some("<</DSS 50 0 R/Metadata 8 0 R/Pages 2 0 R/Type /Catalog>>"),
            both(&[edge(0, "Root")]),
        ),
        change(
            8,
            None,
            Some("stream(100,cc)<</Type /Metadata>>"),
            new_only(&[edge(1, "Metadata")]),
        ),
        change(7, Some("stream(90,dd)<</Type /Metadata>>"), None, old_only(&[edge(1, "Metadata")])),
        change(
            6,
            Some("<</Producer (a)>>"),
            Some("<</ModDate (D:2026)/Producer (a)>>"),
            both(&[edge(0, "Info")]),
        ),
        change(50, None, Some("<</Certs [51 0 R]>>"), new_only(&[edge(1, "DSS")])),
        change(51, None, Some("stream(400,ee)<</Length 400>>"), new_only(&[edge(50, "Certs/[0]")])),
        change(
            0,
            Some("<</Info 6 0 R/Root 1 0 R/Size 60>>"),
            Some("<</Info 6 0 R/Prev 999/Root 1 0 R/Size 61>>"),
            Usage::default(),
        ),
    ];
    let s = step(1, 2, &changes, &lta, &lta);
    assert!(forbidden(&s).is_empty(), "{s}");
    assert_eq!(verdict(&s), "permitted");
}

fn with_sig1() -> Value {
    let mut fields = base_fields();
    fields.as_array_mut().unwrap().push(
        json!({ "objectNumber": 60, "name": "sig1", "family": "signature", "widgets": [60] }),
    );
    fields
}

fn signature_value(object_number: u32, contents: &str) -> Value {
    change(object_number, None, Some(contents), new_only(&[edge(60, "V")]))
}

#[test]
fn a_second_signature_with_a_field_mdp_lock_is_permitted_and_touching_the_locked_field_is_not() {
    let before = structure(json!({ "fields": with_sig1(),
        "signatures": [approval(), unsigned(1, 60, json!({ "fieldName": "sig1" }))] }));
    let after = structure(json!({ "fields": with_sig1(), "signatures": [approval(),
        signature(1, 60, json!({ "fieldName": "sig1", "revisionIndex": 2,
            "fieldMdp": { "action": "include", "fields": ["name"] } }))] }));
    let signing = [
        change(
            60,
            Some("<</FT /Sig/Subtype /Widget/T (sig1)>>"),
            Some("<</AP <</N 62 0 R>>/FT /Sig/Subtype /Widget/T (sig1)/V 61 0 R>>"),
            both(&[edge(10, "Fields/[2]"), edge(3, "Annots/[2]")]),
        ),
        signature_value(
            61,
            "<</ByteRange [0 1 2 3]/Contents (x)/Filter /Adobe.PPKLite/Type /Sig>>",
        ),
        change(62, None, Some("stream(5,ab)<</Subtype /Form>>"), new_only(&[edge(60, "AP/N")])),
        // The lock sets ReadOnly on the covered field.
        change(
            20,
            Some("<</FT /Tx/T (name)/V (a)>>"),
            Some("<</FT /Tx/Ff 1/T (name)/V (a)>>"),
            both(&[edge(10, "Fields/[0]")]),
        ),
        change(
            10,
            Some("<</Fields [20 0 R 30 0 R 60 0 R]>>"),
            Some("<</Fields [20 0 R 30 0 R 60 0 R]/SigFlags 3>>"),
            both(&[edge(1, "AcroForm")]),
        ),
    ];
    let s = step(1, 2, &signing, &before, &after);
    assert!(forbidden(&s).is_empty(), "{s}");
    assert_eq!(verdict(&s), "permitted");
    // Next step: the locked field changes, forbidden by the lock whatever
    // the level.
    let later = [change(
        20,
        Some("<</FT /Tx/Ff 1/T (name)/V (a)>>"),
        Some("<</FT /Tx/Ff 1/T (name)/V (b)>>"),
        both(&[edge(10, "Fields/[0]")]),
    )];
    let locked = step(2, 3, &later, &after, &after);
    assert_eq!(verdict(&locked), "forbidden");
    let on_field = findings(&locked).iter().find(|f| f["objectNumber"] == 20).unwrap();
    assert_eq!(on_field["rule"], "field-lock");
    assert_eq!(locked["locks"].as_array().unwrap().len(), 1);
}

#[test]
fn a_lock_added_to_an_existing_signature_field_while_signing_it_depends_on_the_level() {
    let sig1_unsigned = unsigned(1, 60, json!({ "fieldName": "sig1" }));
    let sig1_signed = signature(
        1,
        60,
        json!({ "fieldName": "sig1", "revisionIndex": 2,
            "fieldMdp": { "action": "include", "fields": ["name"] },
            "lock": { "action": "include", "fields": ["name"] } }),
    );
    let before =
        structure(json!({ "fields": with_sig1(), "signatures": [approval(), sig1_unsigned] }));
    let after =
        structure(json!({ "fields": with_sig1(), "signatures": [approval(), sig1_signed] }));
    let signing = [
        change(
            60,
            Some("<</FT /Sig/Subtype /Widget/T (sig1)>>"),
            Some("<</FT /Sig/Lock 63 0 R/Subtype /Widget/T (sig1)/V 61 0 R>>"),
            both(&[edge(10, "Fields/[2]"), edge(3, "Annots/[2]")]),
        ),
        signature_value(
            61,
            "<</ByteRange [0 1 2 3]/Contents (x)/Filter /Adobe.PPKLite/Type /Sig>>",
        ),
        change(
            63,
            None,
            Some("<</Action /Include/Fields [(name)]/Type /SigFieldLock>>"),
            new_only(&[edge(60, "Lock")]),
        ),
    ];
    let s = step(1, 2, &signing, &before, &after);
    assert_eq!(verdict(&s), "permitted", "{s}");
    assert!(any(&s, |f| f["objectNumber"] == 63 && f["verdict"] == "permitted"));

    // The same signing under a P=2 certification: the lock is not form
    // fill-in.
    let certified_before = structure(json!({ "fields": with_sig1(),
        "signatures": [certification(2), before["signatures"][1]] }));
    let certified_after = structure(json!({ "fields": with_sig1(),
        "signatures": [certification(2), after["signatures"][1]] }));
    let certified = step(1, 2, &signing, &certified_before, &certified_after);
    assert_eq!(verdict(&certified), "forbidden");
    let on_widget = findings(&certified)
        .iter()
        .find(|f| f["objectNumber"] == 60 && f["verdict"] == "forbidden")
        .unwrap();
    assert!(on_widget["detail"].as_str().unwrap().contains("Lock"), "{on_widget}");
    // One cause, not a cascade: the /V and /Lock edges are still claimed.
    assert_eq!(forbidden(&certified).len(), 1, "{certified}");
}

#[test]
fn a_certification_after_an_existing_signature_or_perms_out_of_place_is_forbidden() {
    let before = structure(
        json!({ "fields": with_sig1(), "signatures": [approval(), unsigned(1, 60, json!({}))] }),
    );
    let after = structure(json!({ "fields": with_sig1(), "signatures": [approval(),
        signature(1, 60, json!({ "docMdp": 2, "catalogCertification": true, "revisionIndex": 2 }))] }));
    let changes = [
        change(
            60,
            Some("<</FT /Sig/T (sig1)>>"),
            Some("<</FT /Sig/T (sig1)/V 61 0 R>>"),
            both(&[edge(10, "Fields/[2]")]),
        ),
        change(61, None, Some("<</Type /Sig>>"), new_only(&[edge(60, "V"), edge(70, "DocMDP")])),
        change(70, None, Some("<</DocMDP 61 0 R>>"), new_only(&[edge(1, "Perms")])),
        change(
            1,
            Some("<</Pages 2 0 R/Type /Catalog>>"),
            Some("<</Pages 2 0 R/Perms 70 0 R/Type /Catalog>>"),
            both(&[edge(0, "Root")]),
        ),
    ];
    let s = step(1, 2, &changes, &before, &after);
    assert_eq!(verdict(&s), "forbidden");
    assert!(any(&s, |f| f["rule"] == "signature-added"
        && f["detail"].as_str().is_some_and(|d| d.contains("certification"))));
}

#[test]
fn a_truncated_value_makes_the_step_indeterminate_never_permitted() {
    let s = structure(json!({}));
    let mut c = change(20, Some("<</V (a)>>"), Some("<</V (b)>>"), both(&[edge(10, "Fields/[0]")]));
    c["value"]["truncated"] = json!(true);
    assert_eq!(verdict(&step(1, 2, &[c], &s, &s)), "indeterminate");
}

#[test]
fn annotations_are_permitted_at_annotate_forbidden_at_fill_and_widgets_are_never_annotations() {
    let changes = [
        change(
            3,
            Some("<</Annots [21 0 R]/Type /Page>>"),
            Some("<</Annots [21 0 R 80 0 R]/Type /Page>>"),
            both(&[edge(2, "Kids/[0]")]),
        ),
        change(
            80,
            None,
            Some("<</AP <</N 81 0 R>>/Rect [0 0 1 1]/Subtype /Square/Type /Annot>>"),
            new_only(&[edge(3, "Annots/[1]")]),
        ),
        change(81, None, Some("stream(3,cd)<</Subtype /Form>>"), new_only(&[edge(80, "AP/N")])),
    ];
    // A P=3 certification declared annotations allowed.
    let annotate = signed_with(json!([certification(3)]));
    assert_eq!(verdict(&step(1, 2, &changes, &annotate, &annotate)), "permitted");
    // An approval signature is judged at the baseline (annotate).
    let approval = structure(json!({}));
    assert_eq!(verdict(&step(1, 2, &changes, &approval, &approval)), "permitted");
    // A P=2 certification declared only form fill-in.
    let certified = signed_with(json!([certification(2)]));
    assert_eq!(verdict(&step(1, 2, &changes, &certified, &certified)), "forbidden");
    // Removing a widget through /Annots is a form structure change, not an
    // annotation edit.
    let remove_widget = [change(
        3,
        Some("<</Annots [21 0 R]/Type /Page>>"),
        Some("<</Annots []/Type /Page>>"),
        both(&[edge(2, "Kids/[0]")]),
    )];
    assert_eq!(verdict(&step(1, 2, &remove_widget, &approval, &approval)), "forbidden");
}

// ── evaluateStep: hardening ───────────────────────────────────────────────

#[test]
fn a_present_side_whose_value_is_missing_is_incomplete() {
    let lta = signed_with(json!([certification(1)]));
    let mut c = change(
        1,
        Some("<</Pages 2 0 R/Type /Catalog>>"),
        Some("<</Pages 2 0 R/Type /Catalog>>"),
        both(&[edge(0, "Root")]),
    );
    c["value"] = json!({ "old": null, "new": null, "truncated": false });
    let s = step(1, 2, &[c], &lta, &lta);
    assert_eq!(verdict(&s), "indeterminate");
    assert!(any(&s, |f| f["objectNumber"] == 1 && f["verdict"] == "incomplete"));
}

#[test]
fn the_trailer_is_covered_by_the_evidence_check() {
    let fill = signed_with(json!([certification(2)]));
    let mut c =
        change(0, Some("<</Root 1 0 R/Size 5>>"), Some("<</Root 1 0 R/Size 5>>"), Usage::default());
    c["value"] = json!({
        "old": serde_json::to_value(parse_pdf_value("<</Root 1 0 R/Size 5>>").unwrap()).unwrap(),
        "new": null, "truncated": false });
    assert_eq!(verdict(&step(1, 2, &[c], &fill, &fill)), "indeterminate");
}

#[test]
fn unresolvable_references_are_incomplete_never_permitted() {
    let fill = signed_with(json!([certification(2)]));
    let c = with(
        change(
            9,
            Some("<</JS (a)/S /JavaScript>>"),
            Some("<</JS (b)/S /JavaScript>>"),
            Usage::default(),
        ),
        json!({ "usageIncomplete": true }),
    );
    assert_eq!(verdict(&step(1, 2, &[c], &fill, &fill)), "indeterminate");
}

#[test]
fn a_dss_update_cannot_vouch_for_an_unrelated_catalog_edge() {
    let lta = signed_with(json!([certification(1)]));
    let changes = [
        change(
            9,
            Some("<</JS (a)/S /JavaScript>>"),
            Some("<</JS (b)/S /JavaScript>>"),
            both(&[edge(1, "OpenAction")]),
        ),
        change(50, Some("<</Certs []>>"), Some("<</Certs []/VRI <<>>>>"), both(&[edge(1, "DSS")])),
    ];
    let s = step(1, 2, &changes, &lta, &lta);
    assert_eq!(verdict(&s), "forbidden");
    assert!(any(&s, |f| f["objectNumber"] == 9 && f["verdict"] == "forbidden"));
    assert!(any(&s, |f| f["objectNumber"] == 50 && f["verdict"] == "permitted"));
}

#[test]
fn an_appearance_stream_joined_through_a_fill_cannot_vouch_for_a_widget_action() {
    let fill = signed_with(json!([certification(2)]));
    let changes = [
        fill_change(),
        change(
            23,
            Some("stream(3,aa)<</Length 3>>"),
            Some("stream(9,bb)<</Length 9>>"),
            both(&[edge(20, "AP/N")]),
        ),
        change(
            24,
            Some("<</JS (a)/S /JavaScript>>"),
            Some("<</JS (b)/S /JavaScript>>"),
            both(&[edge(20, "A")]),
        ),
    ];
    let s = step(1, 2, &changes, &fill, &fill);
    assert_eq!(verdict(&s), "forbidden");
    assert!(any(&s, |f| f["objectNumber"] == 24 && f["verdict"] == "forbidden"));
    assert!(any(&s, |f| f["objectNumber"] == 20 && f["verdict"] == "permitted"));
}

#[test]
fn a_field_added_and_signed_in_one_step_is_permitted_its_v_and_indirect_lock_claimed() {
    let fill = signed_with(json!([certification(2)]));
    let after = structure(json!({ "fields": with_sig1(), "signatures": [certification(2),
        signature(1, 60, json!({ "fieldName": "sig1", "revisionIndex": 2,
            "fieldMdp": { "action": "include", "fields": ["name"] },
            "lock": { "action": "include", "fields": ["name"] } }))] }));
    let changes = [
        change(
            3,
            Some("<</Annots [21 0 R 30 0 R]/Type /Page>>"),
            Some("<</Annots [21 0 R 30 0 R 60 0 R]/Type /Page>>"),
            both(&[edge(2, "Kids/[0]")]),
        ),
        change(
            10,
            Some("<</Fields [20 0 R 30 0 R]>>"),
            Some("<</Fields [20 0 R 30 0 R 60 0 R]/SigFlags 3>>"),
            both(&[edge(1, "AcroForm")]),
        ),
        change(
            60,
            None,
            Some(
                "<</FT /Sig/Lock 63 0 R/P 3 0 R/Rect [0 0 1 1]/Subtype /Widget/T (sig1)/Type /Annot/V 61 0 R>>",
            ),
            new_only(&[edge(3, "Annots/[2]"), edge(10, "Fields/[2]")]),
        ),
        signature_value(61, "<</ByteRange [0 1 2 3]/Contents (x)/Type /Sig>>"),
        change(
            63,
            None,
            Some("<</Action /Include/Fields [(name)]/Type /SigFieldLock>>"),
            new_only(&[edge(60, "Lock")]),
        ),
    ];
    let s = step(1, 2, &changes, &fill, &after);
    let not_permitted: Vec<&Value> =
        findings(&s).iter().filter(|f| f["verdict"] != "permitted").collect();
    assert!(not_permitted.is_empty(), "{s}");
    assert_eq!(verdict(&s), "permitted");
}

#[test]
fn a_lock_landing_on_a_terminal_without_its_own_ff_is_judged_against_the_effective_flags() {
    let fields = json!([
        { "objectNumber": 20, "name": "name", "family": "text", "widgets": [21], "flags": 4096 },
        { "objectNumber": 30, "name": "sig0", "family": "signature", "widgets": [30] },
    ]);
    let before = structure(json!({ "fields": fields, "signatures": [] }));
    let after = structure(json!({ "fields": fields, "signatures": [signature(0, 30, json!({
        "fieldMdp": { "action": "include", "fields": ["name"] },
        "lock": { "action": "include", "fields": ["name"] } }))] }));
    let changes = [
        change(
            20,
            Some("<</FT /Tx/T (name)/V (x)>>"),
            Some("<</FT /Tx/Ff 4097/T (name)/V (x)>>"),
            both(&[edge(10, "Fields/[0]")]),
        ),
        change(
            30,
            Some("<</FT /Sig/Subtype /Widget/T (sig0)>>"),
            Some("<</FT /Sig/Lock 32 0 R/Subtype /Widget/T (sig0)/V 31 0 R>>"),
            both(&[edge(10, "Fields/[1]"), edge(3, "Annots/[1]")]),
        ),
        change(
            31,
            None,
            Some("<</ByteRange [0 1 2 3]/Contents (x)/Type /Sig>>"),
            new_only(&[edge(30, "V")]),
        ),
        change(
            32,
            None,
            Some("<</Action /Include/Fields [(name)]/Type /SigFieldLock>>"),
            new_only(&[edge(30, "Lock")]),
        ),
    ];
    let s = step(0, 1, &changes, &before, &after);
    let not_permitted: Vec<&Value> =
        findings(&s).iter().filter(|f| f["verdict"] != "permitted").collect();
    assert!(not_permitted.is_empty(), "{s}");
    assert_eq!(verdict(&s), "permitted");
}

// ── evaluateStep: appearance regeneration ─────────────────────────────────

#[test]
fn an_appearance_stream_regenerated_under_an_untouched_field_is_form_filling_at_p2() {
    let fill = signed_with(json!([certification(2)]));
    let lta = signed_with(json!([certification(1)]));
    let changes = [change(
        23,
        Some("stream(3,aa)<</Length 3>>"),
        Some("stream(9,bb)<</Length 9>>"),
        both(&[edge_via(21, "AP/N", &[22])]),
    )];
    let filled = step(1, 2, &changes, &fill, &fill);
    assert_eq!(verdict(&filled), "permitted");
    let on_stream = findings(&filled).iter().find(|f| f["objectNumber"] == 23).unwrap();
    assert_eq!(on_stream["rule"], "form-fill");
    assert_eq!(verdict(&step(1, 2, &changes, &lta, &lta)), "forbidden");
}

#[test]
fn an_appearance_stream_shared_between_two_fields_is_nobodys_regeneration() {
    let mut fields = base_fields();
    fields
        .as_array_mut()
        .unwrap()
        .push(json!({ "objectNumber": 40, "name": "other", "family": "text", "widgets": [40] }));
    let two = structure(json!({ "fields": fields, "signatures": [certification(2)] }));
    let changes = [
        fill_change(),
        change(
            23,
            Some("stream(3,aa)<</Length 3>>"),
            Some("stream(9,bb)<</Length 9>>"),
            both(&[edge_via(21, "AP/N", &[22]), edge_via(40, "AP/N", &[22])]),
        ),
    ];
    let s = step(1, 2, &changes, &two, &two);
    assert_eq!(verdict(&s), "forbidden");
    assert!(any(&s, |f| f["objectNumber"] == 23
        && f["verdict"] == "forbidden"
        && f["edge"] == "40:AP/N"));
}

// ── EdgeResolver (`resolve.test.ts`) ──────────────────────────────────────

type Index = HashMap<u32, Vec<ObjectReferrer>>;

fn referrer(parent: u32, label: &str) -> ObjectReferrer {
    ObjectReferrer { parent, label: label.to_owned(), via: None }
}

fn reader(
    index: Index,
) -> (impl FnMut(u32) -> Vec<ObjectReferrer>, std::rc::Rc<std::cell::Cell<usize>>) {
    let reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = reads.clone();
    let read = move |n: u32| {
        counter.set(counter.get() + 1);
        index.get(&n).cloned().unwrap_or_default()
    };
    (read, reads)
}

fn anchors(of: &[u32]) -> HashSet<u32> {
    of.iter().copied().collect()
}

fn resolved(parent: u32, label: &str, via: &[u32]) -> ObjectReferrer {
    ObjectReferrer { parent, label: label.to_owned(), via: Some(via.to_vec()) }
}

fn budget(max_depth: usize, max_edges_per_object: usize, max_reads: usize) -> EdgeResolverBudget {
    EdgeResolverBudget { max_depth, max_edges_per_object, max_reads }
}

#[test]
fn an_edge_from_an_unchanged_object_is_resolved_with_joined_labels() {
    // widget 20 --AP--> apDict 22 --N--> stream 23 (changed). Anchors: {20}.
    let (read, _) =
        reader(Index::from([(23, vec![referrer(22, "N")]), (22, vec![referrer(20, "AP")])]));
    let mut r = EdgeResolver::new(read, anchors(&[20, 23]));
    assert_eq!(r.resolve(23), Some(vec![resolved(20, "AP/N", &[22])]));
}

#[test]
fn a_shared_unchanged_dictionary_yields_one_anchored_edge_per_anchor() {
    let (read, _) = reader(Index::from([
        (23, vec![referrer(22, "N")]),
        (22, vec![referrer(20, "AP"), referrer(40, "AP")]),
    ]));
    let mut r = EdgeResolver::new(read, anchors(&[20, 40, 23]));
    assert_eq!(r.resolve(23), Some(vec![resolved(20, "AP/N", &[22]), resolved(40, "AP/N", &[22])]));
}

/// Layers of two unchanged nodes, each referenced by both nodes of the
/// layer above; the top, 1000, is an anchor, and 3000 the leaf.
fn lattice(depth: u32) -> Index {
    let mut index = Index::new();
    let mut ids = vec![1000];
    for d in 0..depth {
        let next = vec![2000 + d * 2, 2001 + d * 2];
        for n in &next {
            index.insert(*n, ids.iter().map(|p| referrer(*p, &format!("k{d}"))).collect());
        }
        ids = next;
    }
    index.insert(3000, ids.iter().map(|p| referrer(*p, "leaf")).collect());
    index
}

#[test]
fn a_diamond_lattice_costs_its_size_not_its_path_count() {
    let depth = 12;
    let (read, reads) = reader(lattice(depth));
    let mut r =
        EdgeResolver::with_budget(read, anchors(&[1000, 3000]), budget(64, 1_000_000, 1_000_000));
    let out = r.resolve(3000).expect("within budget");
    // 2^depth paths, every one ending at the single anchor; reads stay
    // linear in nodes.
    assert!(reads.get() <= (depth * 2 + 2) as usize, "{} reads", reads.get());
    assert_eq!(out.len(), 1 << depth);
}

#[test]
fn exceeding_the_per_object_edge_budget_reports_incomplete() {
    let (read, _) = reader(lattice(10));
    let mut r = EdgeResolver::with_budget(read, anchors(&[1000, 3000]), budget(64, 256, 1_000_000));
    assert_eq!(r.resolve(3000), None);
}

#[test]
fn exceeding_the_depth_budget_reports_incomplete() {
    let chain = || {
        let mut index = Index::new();
        for n in 1..=40 {
            index.insert(n, vec![referrer(n + 1, "p")]);
        }
        index.insert(41, vec![referrer(0, "Root")]);
        index
    };
    let (read, _) = reader(chain());
    let mut shallow = EdgeResolver::with_budget(read, anchors(&[0]), budget(16, 256, 1_000_000));
    assert_eq!(shallow.resolve(1), None);
    let (read, _) = reader(chain());
    let mut deep = EdgeResolver::with_budget(read, anchors(&[0]), budget(64, 256, 1_000_000));
    // `via` runs from the anchor's side down to the object: 41, 40, ..., 2.
    let via: Vec<u32> = (0..40).map(|i| 41 - i).collect();
    let label = format!("Root/{}p", "p/".repeat(39));
    assert_eq!(deep.resolve(1), Some(vec![resolved(0, &label, &via)]));
}

#[test]
fn exhausting_the_read_budget_makes_every_later_resolution_incomplete() {
    let (read, _) = reader(Index::from([
        (5, vec![referrer(6, "a")]),
        (6, vec![referrer(0, "b")]),
        (7, vec![referrer(0, "c")]),
    ]));
    let mut r = EdgeResolver::with_budget(read, anchors(&[0]), budget(16, 256, 1));
    assert_eq!(r.resolve(5), None);
    assert_eq!(r.resolve(7), None);
}

#[test]
fn a_cycle_among_unchanged_objects_contributes_no_anchors_and_does_not_poison_the_memo() {
    // 10 <-> 11 reference each other; 11 is also referenced from the page
    // (anchor 3); 12 is under 10.
    let (read, _) = reader(Index::from([
        (12, vec![referrer(10, "x")]),
        (10, vec![referrer(11, "y")]),
        (11, vec![referrer(10, "z"), referrer(3, "Annots/[0]")]),
    ]));
    let mut r = EdgeResolver::new(read, anchors(&[3, 12]));
    assert_eq!(r.resolve(12), Some(vec![resolved(3, "Annots/[0]/y/x", &[11, 10])]));
    // Resolving 11 afresh must not reuse a result computed while 10 was on
    // the active chain.
    assert_eq!(r.resolve(11), Some(vec![referrer(3, "Annots/[0]")]));
}

#[test]
fn the_default_budget_is_upstreams() {
    assert_eq!(
        (
            DEFAULT_EDGE_RESOLVER_BUDGET.max_depth,
            DEFAULT_EDGE_RESOLVER_BUDGET.max_edges_per_object,
            DEFAULT_EDGE_RESOLVER_BUDGET.max_reads
        ),
        (16, 256, 20_000)
    );
}
