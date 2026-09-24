//! G8's `/PieceInfo` gate: private application data reads the same.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run piece-info
//!
//! Both holders for every case. The catalog and page accessor families are
//! the same calls with a page object number inserted, so a branch written the
//! wrong way round at one call site reads the catalog's data while claiming
//! the page's, and a corpus that only ever asked one of them would not
//! notice. The fixture therefore gives them DIFFERENT data.

use std::collections::BTreeMap;

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::files::piece_info::{
    PieceInfoEntry, read_piece_info, read_piece_info_applications,
};
use pdfium::{Document, Library, PieceInfoHolder};
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
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    fixture: String,
    covers: String,
    page_object_number: u32,
    holders: BTreeMap<String, Holder>,
}

#[derive(Debug, Deserialize)]
struct Holder {
    applications: Vec<String>,
    snapshots: BTreeMap<String, serde_json::Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("files").join("piece-info.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("piece-info.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "files.pieceInfo");
    file
}

fn open(case: &Case) -> Document {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    Document::from_bytes(Library::acquire(), &bytes, None)
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

/// See `annotation_reader.rs`: JSON has one number type and these two
/// serializers spell a whole one differently. A `/PieceInfo` number is a PDF
/// real, so a value of 7 is a float on this side and an integer on the other.
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

fn holder_of(case: &Case, label: &str) -> PieceInfoHolder {
    match label {
        "catalog" => PieceInfoHolder::Catalog,
        "page" => PieceInfoHolder::Page(case.page_object_number),
        other => panic!("the vector names a holder {other} this runner does not map"),
    }
}

#[test]
fn the_reads_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 2, "the corpus shrank");
    let mut present = 0usize;
    let mut absent = 0usize;

    for case in &file.cases {
        let document = open(case);
        for (label, expected) in &case.holders {
            let holder = holder_of(case, label);

            let applications =
                read_piece_info_applications(&document, holder, &CancelToken::never())
                    .unwrap_or_else(|e| panic!("{}: {label}: applications: {e}", case.name));
            assert_eq!(
                applications, expected.applications,
                "{}: {label}: the application list: {}",
                case.name, case.covers,
            );

            for (application, theirs) in &expected.snapshots {
                let mine = read_piece_info(&document, holder, application, &CancelToken::never())
                    .unwrap_or_else(|e| panic!("{}: {label}/{application}: {e}", case.name));
                if mine.is_some() {
                    present += 1;
                } else {
                    absent += 1;
                }
                assert_eq!(
                    canonical_numbers(&serde_json::to_value(&mine).expect("serialize")),
                    canonical_numbers(theirs),
                    "{}: {label}/{application}: {}",
                    case.name,
                    case.covers,
                );
            }
        }
    }

    // Both outcomes have to happen or one of them is checking nothing.
    assert!(present > 0, "no application in the corpus has data");
    assert!(absent > 0, "no application in the corpus is absent");
}

#[test]
fn the_two_holders_are_read_separately() {
    // The distinction the whole module is shaped around. The fixture gives
    // the catalog and the page different data under ONE application name, so
    // a reader that consulted the wrong family produces the other holder's
    // answer rather than the same one.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "both-holders").expect("both-holders");
    let document = open(case);

    let catalog =
        read_piece_info(&document, PieceInfoHolder::Catalog, "com.embedpdf", &CancelToken::never())
            .expect("read")
            .expect("the catalog has data");
    let page = read_piece_info(
        &document,
        PieceInfoHolder::Page(case.page_object_number),
        "com.embedpdf",
        &CancelToken::never(),
    )
    .expect("read")
    .expect("the page has data");

    assert_ne!(catalog.entries, page.entries);
    assert_ne!(catalog.last_modified, page.last_modified);
    assert!(catalog.entries.contains_key("aString"));
    assert!(!catalog.entries.contains_key("pageOnly"));
    assert!(page.entries.contains_key("pageOnly"));
    assert!(!page.entries.contains_key("aString"));

    // And an application the catalog has that the page does not.
    assert!(
        read_piece_info(&document, PieceInfoHolder::Catalog, "OtherApp", &CancelToken::never())
            .expect("read")
            .is_some(),
    );
    assert!(
        read_piece_info(
            &document,
            PieceInfoHolder::Page(case.page_object_number),
            "OtherApp",
            &CancelToken::never(),
        )
        .expect("read")
        .is_none(),
    );
}

#[test]
fn a_value_outside_the_vocabulary_is_present_and_unreadable() {
    // The boundary the vocabulary exists to draw. A dictionary, a null and an
    // array that is not a list of text strings are all in the file, all
    // preserved by a write to a sibling key, and none of them readable here.
    // Dropping them would tell a caller the application wrote fewer keys than
    // it did.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "both-holders").expect("both-holders");
    let document = open(case);
    let snapshot =
        read_piece_info(&document, PieceInfoHolder::Catalog, "com.embedpdf", &CancelToken::never())
            .expect("read")
            .expect("data");

    for key in ["aDictionary", "aMixedArray", "aNull"] {
        assert_eq!(
            snapshot.entries.get(key),
            Some(&PieceInfoEntry::Unknown),
            "{key} should be present and unreadable",
        );
    }

    // An EMPTY array is not unreadable. It is an array of no strings, which
    // is a value this vocabulary has, and collapsing the two would make a
    // cleared list indistinguishable from a foreign one.
    assert_eq!(
        snapshot.entries.get("anEmptyArray"),
        Some(&PieceInfoEntry::StringArray { value: Vec::new() }),
    );
}

#[test]
fn a_name_and_a_string_are_told_apart_by_their_tag() {
    // The characters cannot do it. `/Draft` and `(Draft)` read as the same
    // text, and only the tag says which the file held, which is what makes a
    // round trip through a write possible at all.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "both-holders").expect("both-holders");
    let document = open(case);
    let snapshot =
        read_piece_info(&document, PieceInfoHolder::Catalog, "com.embedpdf", &CancelToken::never())
            .expect("read")
            .expect("data");

    assert_eq!(
        snapshot.entries.get("aName"),
        Some(&PieceInfoEntry::Name { value: "Draft".into() }),
    );
    assert_eq!(
        snapshot.entries.get("aString"),
        Some(&PieceInfoEntry::String { value: "hello".into() }),
    );

    // Both booleans, because `false` is the one a reader that returned a
    // default would still get right.
    assert_eq!(snapshot.entries.get("aTrue"), Some(&PieceInfoEntry::Boolean { value: true }));
    assert_eq!(snapshot.entries.get("aFalse"), Some(&PieceInfoEntry::Boolean { value: false }));
}

#[test]
fn an_absent_application_is_null_rather_than_an_empty_snapshot() {
    // An application that never wrote here and one that wrote and cleared
    // every key are different things, and a caller deciding whether to
    // migrate old data needs to tell them apart.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "none").expect("the no-piece-info fixture");
    let document = open(case);

    assert!(
        read_piece_info(
            &document,
            PieceInfoHolder::Catalog,
            "com.embedpdf",
            &CancelToken::never(),
        )
        .expect("read")
        .is_none(),
    );
    assert!(
        read_piece_info_applications(&document, PieceInfoHolder::Catalog, &CancelToken::never())
            .expect("applications")
            .is_empty(),
    );
}

#[test]
fn an_empty_application_name_is_refused_rather_than_guessed_at() {
    // There is no such dictionary key, so a caller passing one has a bug this
    // names. Guessing at an intent would make a mistake read as data.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "both-holders").expect("both-holders");
    let document = open(case);

    let refused = read_piece_info(&document, PieceInfoHolder::Catalog, "", &CancelToken::never())
        .expect_err("an empty application name");
    assert!(refused.is(EngineErrorCode::InvalidArg));
}
