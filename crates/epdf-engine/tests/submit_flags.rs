//! G6: the SubmitForm flags decode agrees with the TypeScript.
//!
//! ISO 32000-2:2020 Table 240. The piece of the actions read where a mistake
//! is silent: a bit at the wrong position sends a form submission to the wrong
//! place in the wrong format, and nothing downstream can tell.
//!
//! Regenerate with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run actions

use epdf_engine::features::actions::submit_flags::{
    SubmitFormat, SubmitMethod, decode_submit_form_flags,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    covers: String,
    raw: u32,
    flags: WireFlags,
}

/// The decoded word as the wire carries it.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WireFlags {
    raw: u32,
    exclude: bool,
    include_no_value_fields: bool,
    format: String,
    method: String,
    submit_coordinates: bool,
    include_append_saves: bool,
    include_annotations: bool,
    canonical_format: bool,
    excl_non_user_annots: bool,
    excl_f_key: bool,
    embed_form: bool,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("actions").join("submit-flags.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("submit-flags.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "actions.submitFlags");
    file
}

#[test]
fn the_decode_agrees_with_typescript() {
    let file = load();
    assert!(file.cases.len() >= 32, "the corpus shrank");

    let mut failures = Vec::new();
    for case in &file.cases {
        let decoded = decode_submit_form_flags(case.raw);
        let actual = WireFlags {
            raw: decoded.raw,
            exclude: decoded.exclude,
            include_no_value_fields: decoded.include_no_value_fields,
            format: match decoded.format {
                SubmitFormat::Fdf => "fdf",
                SubmitFormat::Html => "html",
                SubmitFormat::Xfdf => "xfdf",
                SubmitFormat::Pdf => "pdf",
            }
            .to_string(),
            method: match decoded.method {
                SubmitMethod::Post => "post",
                SubmitMethod::Get => "get",
            }
            .to_string(),
            submit_coordinates: decoded.submit_coordinates,
            include_append_saves: decoded.include_append_saves,
            include_annotations: decoded.include_annotations,
            canonical_format: decoded.canonical_format,
            excl_non_user_annots: decoded.excl_non_user_annots,
            excl_f_key: decoded.excl_f_key,
            embed_form: decoded.embed_form,
        };
        if actual != case.flags {
            failures.push(format!(
                "{} ({:#x}): {:?} against {:?} ({})",
                case.name, case.raw, actual, case.flags, case.covers,
            ));
        }
    }
    assert!(failures.is_empty(), "submit flag divergence:\n  {}", failures.join("\n  "));
}

#[test]
fn every_single_bit_is_covered() {
    // The corpus sets each of the first sixteen bits alone, which is what
    // catches a decoder whose table is shifted: a shift of one moves every
    // field, and a single-bit sweep names exactly which.
    let file = load();
    for position in 1..=16u32 {
        let name = format!("bit-{position}-alone");
        assert!(file.cases.iter().any(|c| c.name == name), "the corpus has no case for {name}",);
    }
}

#[test]
fn the_reserved_bit_changes_nothing() {
    // Bit 13 is reserved. It is the reason EmbedForm is bit 14, and a decoder
    // that used 13 would read a word nobody sets.
    let none = decode_submit_form_flags(0);
    let reserved = decode_submit_form_flags(1 << 12);
    assert!(!reserved.embed_form, "the reserved bit was read as EmbedForm");
    assert_eq!(SubmitFormat::Fdf, reserved.format);
    assert_eq!(none.method, reserved.method);
}

#[test]
fn the_raw_word_is_carried_through_unchanged() {
    // So a consumer that needs a bit this decode does not name can still
    // read it, rather than the engine being the ceiling on what a document
    // can say.
    for raw in [0u32, 1, 0xffff, 0xffff_ffff, 0xdead_beef] {
        assert_eq!(decode_submit_form_flags(raw).raw, raw);
    }
}

// --- G6: the shared action vocabulary.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypeFile {
    schema_version: u32,
    kind: String,
    cases: Vec<TypeCase>,
}

#[derive(Debug, Deserialize)]
struct TypeCase {
    code: i32,
    #[serde(rename = "type")]
    action_type: String,
}

#[test]
fn action_type_codes_agree_with_typescript() {
    use epdf_engine::features::actions::vocabulary::ActionType;

    // The shared vocabulary reaches page actions, annotation actions, field
    // actions and the document catalog, so a code mapped differently is a
    // divergence in four places at once.
    let path = test_support::corpus::vectors_dir("actions").join("types.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: TypeFile = serde_json::from_str(&text).expect("types.json");
    assert_eq!(file.schema_version, 1, "vector envelope moved");
    assert_eq!(file.kind, "actions.types");
    assert!(file.cases.len() >= 26, "the corpus shrank");

    for case in &file.cases {
        assert_eq!(
            ActionType::from_code(case.code).as_str(),
            case.action_type,
            "code {}",
            case.code,
        );
    }
}

#[test]
fn an_unrecognized_code_is_unknown_rather_than_an_error() {
    // A newer PDFium adding an action type must not make a document
    // unreadable, and every consumer already handles Unknown.
    use epdf_engine::features::actions::vocabulary::ActionType;

    for code in [i32::MIN, -99, 19, 1_000, i32::MAX] {
        assert_eq!(ActionType::from_code(code), ActionType::Unknown, "code {code}");
    }
}

#[test]
fn the_invalid_node_sentinel_is_recognized_in_both_widths() {
    // EPDF_ACTION_NODE_INVALID reaches a caller as -1 or as the unsigned
    // maximum, depending on how the boundary widened it. A reader that knew
    // only one would walk off the end of a tree.
    use epdf_engine::features::actions::vocabulary::is_valid_action_node_id;

    assert!(!is_valid_action_node_id(-1));
    assert!(!is_valid_action_node_id(0xffff_ffff));
    assert!(is_valid_action_node_id(0));
    assert!(is_valid_action_node_id(1));
    assert!(is_valid_action_node_id(0xffff_fffe));
}

#[test]
fn warning_flags_decode_and_keep_what_they_do_not_know() {
    use epdf_engine::features::actions::vocabulary::{ActionWarning, ActionWarningFlags};

    assert_eq!(ActionWarningFlags(0).to_warnings(), Vec::new());
    assert_eq!(ActionWarningFlags(0x1).to_warnings(), [ActionWarning::CycleDropped]);
    assert_eq!(ActionWarningFlags(0x2).to_warnings(), [ActionWarning::MalformedNext]);
    assert_eq!(ActionWarningFlags(0x4).to_warnings(), [ActionWarning::Incomplete]);
    assert_eq!(
        ActionWarningFlags(0x7).to_warnings(),
        [ActionWarning::CycleDropped, ActionWarning::MalformedNext, ActionWarning::Incomplete,]
    );

    // A bit this build does not know is not reported as a warning and is not
    // lost either: the raw word is what lets a consumer on a newer runtime
    // see that something was flagged.
    let future = ActionWarningFlags(0x8000_0001);
    assert_eq!(future.to_warnings(), [ActionWarning::CycleDropped]);
    assert_eq!(future.0, 0x8000_0001);
}

#[test]
fn a_budget_stops_granting_once_it_is_spent() {
    use epdf_engine::features::actions::vocabulary::{ActionReadBudget, ActionReadBudgetTracker};

    let mut tracker = ActionReadBudgetTracker::new(ActionReadBudget {
        max_models: 2,
        max_nodes: 3,
        max_script_code_units: 10,
        max_target_entries: 2,
        max_payload_code_units: 5,
    });

    assert!(tracker.take_model());
    assert!(tracker.take_model());
    assert!(!tracker.take_model(), "a third model was granted against a budget of two");

    for _ in 0..3 {
        assert!(tracker.take_node());
    }
    assert!(!tracker.take_node());

    // A single request larger than the whole budget is refused rather than
    // partly granted, since the caller is about to allocate the whole thing.
    assert!(!tracker.take_script(11));
    assert!(!tracker.take_payload(6));
    assert!(tracker.take_targets(2));
    assert!(!tracker.take_targets(1));
}

#[test]
fn an_exhausted_budget_stays_exhausted() {
    use epdf_engine::features::actions::vocabulary::{ActionReadBudget, ActionReadBudgetTracker};

    // A counter that wrapped would start granting again, which is how a
    // budget becomes an invitation.
    let mut tracker = ActionReadBudgetTracker::new(ActionReadBudget {
        max_payload_code_units: 4,
        ..ActionReadBudget::default()
    });
    assert!(tracker.take_payload(4));
    for _ in 0..1_000 {
        assert!(!tracker.take_payload(u32::MAX));
    }
    assert!(!tracker.take_payload(1));
}
