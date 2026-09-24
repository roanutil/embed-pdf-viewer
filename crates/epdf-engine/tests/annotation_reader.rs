//! G7: the annotation reads agree with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-read
//!
//! # What this compares, and what it cannot yet
//!
//! The vector holds the WHOLE DTO the TypeScript produces, including the
//! subtype-specific fields no Rust reader exists for. That is deliberate: a
//! vector trimmed to what Rust can currently produce would be regenerated
//! every time a subtype reader lands, and the diff under review would be the
//! vector rather than the behaviour.
//!
//! So the comparison is drawn along one line, `FALLS_BACK_TO_UNSUPPORTED`. A
//! subtype not on that list is compared WHOLE, every field. One on it is
//! compared as a base shell and additionally asserted to read as
//! `unsupported`. When a reader lands its subtype leaves the list, and the
//! only way to make this pass again is for that subtype's whole DTO to
//! agree.

use std::collections::BTreeSet;

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::annotation::read::page::read_document_annotations;
use epdf_engine::features::annotation::read::registry::FALLS_BACK_TO_UNSUPPORTED;
use epdf_engine::features::annotation::subtype::AnnotationSubtype;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::revision::RevisionToken;
use pdfium::{Document, Library};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

/// The generator replaces every document session id with this, because one is
/// minted per open and a verbatim recording would never compare equal twice.
const SESSION_PLACEHOLDER: &str = "<session>";

/// The fields `AnnotationBase` owns, as they are spelled on the wire.
///
/// Written out rather than derived from the struct, so that a field added to
/// the shell without being added here is a comparison that silently stops
/// covering it.
const BASE_FIELDS: [&str; 20] = [
    "ref",
    "pageObjectNumber",
    "index",
    "identityQuality",
    "nm",
    "flags",
    "rect",
    "contents",
    "subject",
    "author",
    "created",
    "modified",
    "blendMode",
    "inReplyTo",
    "replyType",
    "userId",
    "groupId",
    "createdBy",
    "updatedBy",
    "actions",
];

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
    fixture: String,
    covers: String,
    #[serde(default)]
    password: Option<String>,
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    page_state: serde_json::Value,
    annotations: Vec<serde_json::Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("read.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("read.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.read");
    file
}

fn open(case: &Case) -> Document {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    Document::from_bytes(Library::acquire(), &bytes, case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

/// Read a case the way the generator did, with the session id the generator
/// substituted so the two revision tokens can be compared as values.
fn read(case: &Case) -> serde_json::Value {
    let document = open(case);
    let snapshot = read_document_annotations(
        &document,
        |page: PageObjectNumber| RevisionToken::new(SESSION_PLACEHOLDER, page, 0),
        &CancelToken::never(),
    )
    .unwrap_or_else(|e| panic!("{}: read: {e}", case.name));
    serde_json::to_value(&snapshot).expect("serialize")
}

/// Rewrite every integral float as an integer, on both sides.
///
/// JSON has one number type and these two serializers disagree about how to
/// spell a whole one: JavaScript writes `700` for a coordinate and `serde_json`
/// writes `700.0` for the same `f64`. Comparing `Value`s therefore fails on a
/// rect that is identical. Only integral values are touched, and both sides
/// get the same treatment, so a real difference still is one.
fn canonical_numbers(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9e15 => {
                serde_json::Value::from(float as i64)
            }
            _ => value.clone(),
        },
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_numbers).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(key, entry)| (key.clone(), canonical_numbers(entry))).collect(),
        ),
        _ => value.clone(),
    }
}

fn object(value: &serde_json::Value, what: &str) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().unwrap_or_else(|| panic!("{what} is not an object")).clone()
}

/// The base half of a DTO, with everything else dropped.
fn base_of(dto: &serde_json::Value, what: &str) -> serde_json::Map<String, serde_json::Value> {
    let all = object(&canonical_numbers(dto), what);
    all.into_iter().filter(|(key, _)| BASE_FIELDS.contains(&key.as_str())).collect()
}

/// Whether this build produces a subtype's own fields, or only the shell.
fn is_ported(subtype: &str) -> bool {
    subtype == "unsupported"
        || !FALLS_BACK_TO_UNSUPPORTED.iter().any(|listed| listed.as_str() == subtype)
}

#[test]
fn the_reads_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");
    let mut compared_whole = 0usize;
    let mut compared_shell = 0usize;

    for case in &file.cases {
        let actual = read(case);
        let pages = actual["pages"].as_array().expect("pages");
        assert_eq!(
            pages.len(),
            case.pages.len(),
            "{}: page count differs: {}",
            case.name,
            case.covers,
        );

        for (page_index, (mine, theirs)) in pages.iter().zip(&case.pages).enumerate() {
            let mine_annotations = mine["annotations"].as_array().expect("annotations");
            assert_eq!(
                mine_annotations.len(),
                theirs.annotations.len(),
                "{}: page {page_index} has a different number of annotations",
                case.name,
            );

            for (index, (mine, theirs)) in
                mine_annotations.iter().zip(&theirs.annotations).enumerate()
            {
                let what = format!("{}: page {page_index} annotation {index}", case.name);
                let subtype = theirs["subtype"].as_str().expect("a subtype");

                if is_ported(subtype) {
                    compared_whole += 1;
                    assert_eq!(
                        canonical_numbers(mine),
                        canonical_numbers(theirs),
                        "{what} is {subtype}, which is ported: {}",
                        case.covers,
                    );
                } else {
                    compared_shell += 1;
                    assert_eq!(
                        base_of(mine, &what),
                        base_of(theirs, &what),
                        "{what} is {subtype}, compared as a shell: {}",
                        case.covers,
                    );
                    assert_eq!(
                        mine["subtype"].as_str(),
                        Some("unsupported"),
                        "{what} is {subtype} and has no reader, so it should read as \
                         unsupported",
                    );
                }
            }
        }
    }

    // Both arms have to be exercised, or one of them is checking nothing.
    assert!(compared_whole > 0, "no annotation in the corpus has a ported subtype");
    assert!(compared_shell > 0, "no annotation in the corpus has an unported subtype");
}

#[test]
fn the_page_state_agrees_including_the_weak_flag() {
    // Separate from the annotation comparison because it is a per-page
    // conclusion drawn ACROSS the annotations: one weak identity makes the
    // page weak, and a reader that reported the flag per annotation would
    // agree with the vector on every annotation and disagree here.
    let file = load();
    let mut saw_weak = false;
    let mut saw_durable = false;

    for case in &file.cases {
        let actual = read(case);
        let pages = actual["pages"].as_array().expect("pages");
        for (page_index, (mine, theirs)) in pages.iter().zip(&case.pages).enumerate() {
            assert_eq!(
                canonical_numbers(&mine["pageState"]),
                canonical_numbers(&theirs.page_state),
                "{}: page {page_index} state differs",
                case.name,
            );
            let weak = theirs.page_state["weakAnnotationState"]["hasAnyWeakAnnotations"]
                .as_bool()
                .expect("a known weak state");
            saw_weak |= weak;
            saw_durable |= !weak;
        }
    }

    // The comparison above would pass on a corpus where every page answered
    // the same thing, and would then be checking nothing.
    assert!(saw_weak, "no page in the corpus has a weak annotation");
    assert!(saw_durable, "no page in the corpus is free of weak annotations");
}

#[test]
fn the_unported_list_names_real_subtypes_and_does_not_repeat() {
    // The list is what the comparison above draws its line with, so it is
    // checked against the catalog rather than against itself. `unsupported`
    // must never appear on it: for that subtype the fallback IS the reader,
    // and listing it would turn the whole-DTO comparison into a shell one for
    // the one subtype that is fully ported.
    let listed: BTreeSet<&str> = FALLS_BACK_TO_UNSUPPORTED.iter().map(|s| s.as_str()).collect();
    assert_eq!(listed.len(), FALLS_BACK_TO_UNSUPPORTED.len(), "the list repeats a subtype");
    assert!(!listed.contains("unsupported"), "unsupported is not an unported subtype");

    let catalog: BTreeSet<&str> = epdf_engine::features::annotation::subtype::ANNOTATION_SUBTYPES
        .iter()
        .map(|s| s.as_str())
        .collect();
    for name in &listed {
        assert!(catalog.contains(name), "{name} is not an annotation subtype");
    }
    assert!(
        !listed.contains(AnnotationSubtype::Highlight.as_str()),
        "highlight has a reader and should have left the list",
    );
}

#[test]
fn a_rect_is_reported_in_the_corner_order_the_file_wrote() {
    // The only rect in this engine that is not normalized, and the reason is
    // that a writer has to be able to leave one alone. `annotation_base.pdf`
    // has an annotation whose /Rect runs the other way.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "base").expect("the base fixture");
    let actual = read(case);
    let annotations = actual["pages"][0]["annotations"].as_array().expect("annotations");

    let reversed = annotations
        .iter()
        .find(|a| a["rect"]["left"].as_f64() > a["rect"]["right"].as_f64())
        .expect("the fixture has a /Rect in reversed corner order");
    assert_eq!(reversed["rect"]["left"], 200.0);
    assert_eq!(reversed["rect"]["right"], 180.0);
    assert!(reversed["rect"]["top"].as_f64() < reversed["rect"]["bottom"].as_f64());
}

#[test]
fn a_reply_edge_carries_a_type_exactly_when_it_carries_a_parent() {
    // The invariant the relationship reader exists to hold. PDFium answers
    // `/RT`'s default for an annotation with no `/IRT` at all, so a reader
    // that asked without gating would report `reply` on every top-level
    // annotation in the document.
    let file = load();
    let mut saw_reply = false;
    let mut saw_group = false;
    let mut saw_none = false;

    for case in &file.cases {
        let actual = read(case);
        for page in actual["pages"].as_array().expect("pages") {
            for annotation in page["annotations"].as_array().expect("annotations") {
                let parent = &annotation["inReplyTo"];
                let reply_type = &annotation["replyType"];
                assert_eq!(
                    parent.is_null(),
                    reply_type.is_null(),
                    "{}: a parent without a type, or the reverse: {annotation}",
                    case.name,
                );
                match reply_type.as_str() {
                    Some("reply") => saw_reply = true,
                    Some("group") => saw_group = true,
                    None => saw_none = true,
                    Some(other) => panic!("{}: unknown reply type {other}", case.name),
                }
            }
        }
    }

    assert!(saw_reply && saw_group && saw_none, "the corpus does not cover all three cases");
}
