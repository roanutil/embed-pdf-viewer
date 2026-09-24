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
