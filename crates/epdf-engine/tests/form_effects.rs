//! G12's effects batch: an ordered run of one client form script.
//!
//! Replays `vectors/forms/effects.json`, which the TypeScript produced by
//! calling `forms.applyEffects` on `form_fields.pdf`.
//!
//! # Why the statuses are what this compares
//!
//! Every other form suite compares documents. This one compares a
//! VOCABULARY, because two of its distinctions are invisible in the
//! resulting field values and both change what the caller does next.
//!
//! `unchanged` against `applied` decides whether a version bump happens.
//! A script that recomputes every field on every keystroke sends mostly
//! no-ops, and reporting those as applied would invalidate every cache
//! downstream on every character typed.
//!
//! `rejected` against `failed` decides whether the caller can show a user
//! their mistake or has to refetch the document. An engine that reported one
//! as the other would either cause spurious refetches or leave a stale view
//! nobody knows is stale.
//!
//! The error MESSAGES are not compared, only the codes. Each implementation
//! words its own, and a comparison of prose would pin the wording.

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::forms::effects::{FormEffect, FormEffectStatus, apply};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;

const VECTOR: &str = "tooling/engine-conformance/vectors/forms/effects.json";
const FIXTURE: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";

fn open() -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(FIXTURE))
        .unwrap_or_else(|e| panic!("read {FIXTURE}: {e}"));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn status_of(raw: &str) -> FormEffectStatus {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .unwrap_or_else(|e| panic!("this build has no '{raw}' status: {e}"))
}

#[test]
fn every_effects_batch_agrees_with_the_oracle() {
    let raw = std::fs::read_to_string(test_support::workspace_root().join(VECTOR))
        .unwrap_or_else(|e| panic!("read {VECTOR}: {e}"));
    let file: serde_json::Value = serde_json::from_str(&raw).expect("the vector parses");
    assert_eq!(file["schemaVersion"], 1, "the vector's envelope changed shape");
    assert_eq!(file["fixture"], FIXTURE, "the vector was recorded against another document");
    let cases = file["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "the vector is empty");

    for case in cases {
        let name = case["name"].as_str().expect("name");
        let effects: Vec<FormEffect> = serde_json::from_value(case["effects"].clone())
            .unwrap_or_else(|e| panic!("{name}: the effects do not decode: {e}"));

        // A fresh document per case. The batches are stateful by design and
        // several of them depend on starting from the shipped values.
        let session = open();
        let document = session.document().expect("open").document();
        let result = apply(document, &effects, &CancelToken::never())
            .unwrap_or_else(|e| panic!("{name}: the batch did not run: {e}"));

        let expected = case["results"].as_array().expect("results");
        assert_eq!(
            result.results.len(),
            expected.len(),
            "{name}: a different number of effects reported",
        );

        for (got, want) in result.results.iter().zip(expected) {
            let at = want["index"].as_u64().expect("index");
            assert_eq!(u64::from(got.index), at, "{name}: the results are out of order");
            assert_eq!(
                got.status,
                status_of(want["status"].as_str().expect("status")),
                "{name}: effect {at} had a different outcome",
            );

            match want.get("errorCode").and_then(|c| c.as_str()) {
                Some(code) => {
                    let error = got.error.as_ref().unwrap_or_else(|| {
                        panic!("{name}: effect {at} was refused with no error attached")
                    });
                    assert_eq!(
                        error["code"].as_str(),
                        Some(code),
                        "{name}: effect {at} was refused for a different reason",
                    );
                }
                None => assert!(
                    got.error.is_none(),
                    "{name}: effect {at} carried an error the oracle does not report",
                ),
            }

            // The changed-widget list is half the point. A radio switch
            // changes two widgets for one write, and a caller holding
            // rasters keyed by annotation cannot work out which to redraw
            // from the value alone.
            let want_widgets: Vec<(u32, u32)> = want["changedWidgets"]
                .as_array()
                .expect("changedWidgets")
                .iter()
                .map(|w| {
                    (
                        // `page: PageRef | null`; null is an unplaced widget,
                        // which the engine's record holds as 0.
                        w["page"]["objectNumber"].as_u64().unwrap_or(0) as u32,
                        w["annotObjectNumber"].as_u64().expect("annot") as u32,
                    )
                })
                .collect();
            let got_widgets: Vec<(u32, u32)> = got
                .changed_widgets
                .iter()
                .map(|w| (w.page_object_number, w.annot_object_number))
                .collect();
            assert_eq!(
                sorted(got_widgets),
                sorted(want_widgets),
                "{name}: effect {at} reported different changed widgets",
            );

            // The fields each effect names, read back.
            let want_fields = want["fields"].as_array().expect("fields");
            assert_eq!(
                got.fields.len(),
                want_fields.len(),
                "{name}: effect {at} read back a different number of fields",
            );
            for (field, expected_field) in got.fields.iter().zip(want_fields) {
                assert_eq!(
                    field.base.field_object_number,
                    expected_field["fieldObjectNumber"].as_u64().expect("number") as u32,
                    "{name}: effect {at} read back a different field",
                );
                assert_eq!(
                    serde_json::to_value(&field.base.value_entry).expect("serialize"),
                    expected_field["valueEntry"],
                    "{name}: effect {at} left field {} holding something else",
                    field.base.name,
                );
            }
        }

        // Whether the batch wrote at all, which is what decides an artifact,
        // an event and a version bump.
        assert_eq!(
            result.meta.is_some(),
            case["wrote"].as_bool().expect("wrote"),
            "{name}: the two disagree about whether anything landed",
        );
    }
}

fn sorted(mut items: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    items.sort_unstable();
    items
}

#[test]
fn a_batch_of_nothing_writes_nothing() {
    // The degenerate case, and not a vacuous one: an empty batch that
    // reported a mutation would bump the version and publish an event for a
    // script run that did not run.
    let session = open();
    let result = apply(session.document().expect("open").document(), &[], &CancelToken::never())
        .expect("an empty batch runs");
    assert!(result.results.is_empty());
    assert!(result.changed_widgets.is_empty());
    assert!(result.meta.is_none(), "an empty batch claimed to have written something");
}

#[test]
fn a_cancelled_batch_is_aborted_rather_than_half_reported() {
    // Cancellation is the one thing that comes back as an `Err`. A partial
    // result list would look like a batch that ran and mostly succeeded,
    // which is the opposite of what happened.
    let session = open();
    let effects: Vec<FormEffect> = serde_json::from_value(serde_json::json!([
        { "kind": "setValue", "ref": { "kind": "objectNumber", "fieldObjectNumber": 4 },
          "value": { "type": "text", "value": "Ada" } }
    ]))
    .expect("the effects decode");

    let cancel = CancelToken::never();
    cancel.cancel();
    let error = apply(session.document().expect("open").document(), &effects, &cancel)
        .expect_err("a cancelled batch");
    assert!(error.is(epdf_engine::error::EngineErrorCode::Aborted), "{error}");
}
