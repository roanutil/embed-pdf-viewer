//! Plain and rich annotation line breaks agree with the public TS boundary.

use epdf_engine::features::annotation::write::primitives::set_string_or_clear;
use pdfium::{Document, Library};

fn document() -> Document {
    Document::from_bytes(
        Library::acquire(),
        &test_support::fixture("tooling/engine-conformance/fixtures/rich_metadata.pdf"),
        None,
    )
    .expect("open fixture")
}

#[test]
fn contents_write_pdf_line_breaks_without_normalizing_other_strings() {
    let document = document();
    let page = document.page(0).expect("page");
    let annotation = page.create_annotation(1).expect("text annotation");
    let mixed = "a\r\nb\rc\nd";
    set_string_or_clear(&annotation, "Contents", Some(mixed)).expect("contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), Some("a\rb\rc\rd".into()));
    set_string_or_clear(&annotation, "Subj", Some(mixed)).expect("subject");
    assert_eq!(annotation.string_value("Subj").expect("read"), Some(mixed.into()));
}

#[test]
fn null_contents_removes_the_key_but_empty_contents_stays_present() {
    let document = document();
    let page = document.page(0).expect("page");
    let annotation = page.create_annotation(1).expect("text annotation");
    assert_eq!(annotation.string_value("Contents").expect("read"), None);
    set_string_or_clear(&annotation, "Contents", Some("")).expect("empty contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), Some(String::new()));
    set_string_or_clear(&annotation, "Contents", None).expect("clear contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), None);
}

fn vectors() -> serde_json::Value {
    let path = test_support::corpus::vectors_dir("annotation").join("line-breaks.json");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("vectors")).expect("JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["kind"], "annotation.lineBreaks");
    value
}

#[test]
fn public_contents_reads_agree_with_the_typescript_oracle() {
    use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
    use epdf_engine::features::annotation::read::base::read_annotation_base;
    use epdf_engine::identity::PageObjectNumber;
    use epdf_engine::revision::RevisionToken;
    let document = document();
    let page = document.page(0).expect("page");
    let page_number = PageObjectNumber::new(page.object_number()).expect("number");
    for case in vectors()["contentsCases"].as_array().expect("cases") {
        let index = page.annotation_count();
        let annotation = page.create_annotation(1).expect("annotation");
        if let Some(contents) = case["input"].get("contents") {
            // Imported PDF strings can contain all three line break spellings.
            if let Some(text) = contents.as_str() {
                assert!(annotation.set_string_value("Contents", text).expect("raw contents"));
            }
        }
        let base = read_annotation_base(
            &document,
            &annotation,
            page_number,
            index,
            &RevisionToken::new("lines", page_number, 0),
            &mut ActionReadBudgetTracker::default(),
        )
        .expect("base");
        assert_eq!(
            serde_json::to_value(base.contents).expect("contents"),
            case["contents"],
            "{}",
            case["name"]
        );
        if let Some(contents) = case["input"].get("contents") {
            set_string_or_clear(&annotation, "Contents", contents.as_str())
                .expect("write contents");
        }
        assert_eq!(
            serde_json::to_value(annotation.string_value("Contents").expect("PDF contents"))
                .expect("JSON"),
            case["pdfContents"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn rich_plain_projection_agrees_with_the_typescript_oracle() {
    use epdf_engine::features::annotation::rich_text::{RichTextDocumentInput, plain_text};
    for case in vectors()["richCases"].as_array().expect("cases") {
        let input: RichTextDocumentInput =
            serde_json::from_value(case["input"].clone()).expect("input");
        assert_eq!(
            plain_text(&input.paragraphs),
            case["plainText"].as_str().expect("text"),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn rich_engine_json_writes_pdf_line_breaks_like_typescript() {
    use epdf_engine::features::annotation::rich_text::{
        RichTextDocumentInput, engine_rich_text_json,
    };
    for case in vectors()["richCases"].as_array().expect("cases") {
        let input: RichTextDocumentInput =
            serde_json::from_value(case["input"].clone()).expect("input");
        let actual: serde_json::Value =
            serde_json::from_str(&engine_rich_text_json(&input, None)).expect("JSON");
        assert_eq!(actual, case["engine"], "{}", case["name"]);
    }
}

#[test]
fn parsed_rich_runs_read_public_line_breaks_like_typescript() {
    use epdf_engine::features::annotation::rich_text::{RichTextDocument, parse_engine_rich_text};
    for case in vectors()["richCases"].as_array().expect("cases") {
        let parsed = parse_engine_rich_text(&case["rawEngine"].to_string()).expect("parse");
        let expected: RichTextDocument =
            serde_json::from_value(case["parsed"].clone()).expect("expected rich document");
        assert_eq!(parsed, expected, "{}", case["name"]);
    }
}

#[test]
fn rich_text_round_trip_uses_pdf_cr_and_public_lf() {
    use epdf_engine::features::annotation::rich_text::{
        RichTextDocumentInput, plain_text, read_rich_text, write_rich_text,
    };
    let document = document();
    let page = document.page(0).expect("page");
    let annotation = page.create_annotation(3).expect("free text annotation");
    assert!(annotation.set_rect(pdfium::RectF {
        left: 10.0,
        bottom: 10.0,
        right: 220.0,
        top: 120.0
    }));
    let input: RichTextDocumentInput = serde_json::from_value(serde_json::json!({
        "body": { "family": "helvetica", "size": 12, "color": "#000000" },
        "paragraphs": [{ "runs": [{ "text": "one\ntwo" }] }, { "runs": [{ "text": "next" }] }]
    }))
    .expect("input");
    write_rich_text(&annotation, &input, None).expect("write");
    assert_eq!(
        annotation.string_value("Contents").expect("PDF contents"),
        Some("one\rtwo\rnext".into())
    );
    let read = read_rich_text(&annotation).expect("public rich text");
    assert_eq!(plain_text(&read.paragraphs), "one\ntwo\nnext");
    assert!(read.paragraphs.iter().flat_map(|p| &p.runs).all(|r| !r.text.contains('\r')));
}
