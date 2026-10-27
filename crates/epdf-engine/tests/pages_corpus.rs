//! G6: the page listing agrees with the TypeScript engine.
//!
//! The vectors record page space, as the TypeScript engine API answers
//! (`pageListOf`): every box measured from the crop box's top-left, y down,
//! and `pdfCropBox` in the file's coordinates. The listing is read in the
//! file's coordinates and compared through the host's conversion.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run metadata
//!
//! Nothing in this read loads a page, which is what keeps listing a large
//! document cheap. The gate that matters is the one the port order names:
//! page identity has to be stable between a lazy per-page lookup and a full
//! enumeration, so the object numbers this produces are compared against the
//! ones the session's registry produces by a different route.

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::pages::{PageListSnapshot, PageTabOrder, list_pages};
use epdf_engine::geom::{PageRect, PdfRect, PdfRotation, PdfSize};
use epdf_engine::page_space::PageCoordinates;
use epdf_engine::page_space::pages::page_list_of;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use pdfium::{Document, Library};
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
    fixture: String,
    covers: String,
    #[serde(default)]
    password: Option<String>,
    pages: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("metadata").join("pages.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("pages.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.list");
    file
}

fn bytes(case: &Case) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture))
}

fn open(case: &Case) -> Document {
    Document::from_bytes(Library::acquire(), &bytes(case), case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

/// The listing as the wire answers it, in page space.
fn listed(case: &Case) -> PageListSnapshot<PageCoordinates> {
    let read = list_pages(&open(case), &CancelToken::never())
        .unwrap_or_else(|e| panic!("{}: list: {e}", case.name));
    page_list_of(read).unwrap_or_else(|e| panic!("{}: in page space: {e}", case.name))
}

#[test]
fn the_listing_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 5, "the corpus shrank");

    for case in &file.cases {
        // Compared as JSON rather than field by field, so a field added on one
        // side and not the other fails here rather than being skipped.
        //
        // Quantized first, because the two languages spell the same number
        // differently in JSON: Rust writes 0.0 where JavaScript writes 0, and
        // a raw comparison would report every page as a difference. The
        // quantizer is the corpus-wide one, so a coordinate is pinned to 1e-4
        // of a PDF point rather than to a float's last bit.
        let actual = test_support::corpus::quantized(
            &serde_json::to_value(listed(case)).expect("serialize"),
        );
        let expected = test_support::corpus::quantized(&case.pages);
        if let Some(difference) = test_support::corpus::first_difference(&expected, &actual) {
            panic!("{}: {difference} ({})", case.name, case.covers);
        }
    }
}

#[test]
fn a_page_with_actions_reports_them() {
    // PageLayout.actions was absent on both sides until page_actions.pdf
    // existed, so the comparison above could not tell an unread action from a
    // missing one. This asserts the corpus still contains one.
    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "page-actions")
        .expect("the page-actions fixture is in the corpus");

    let pages = case.pages["pages"].as_array().expect("pages");
    let with_actions = pages.iter().filter(|p| p.get("actions").is_some()).count();
    assert_eq!(with_actions, 1, "exactly one page in that fixture carries actions");

    let actions = pages[0]["actions"].as_object().expect("actions");
    assert!(actions.contains_key("open"), "the open action is missing");
    assert!(actions.contains_key("close"), "the close action is missing");

    // And the listing produced by Rust has them too, which the comparison
    // above already proves but which is worth failing separately: a mapping
    // that dropped the field would otherwise look like a corpus problem.
    let snapshot = listed(case);
    assert!(snapshot.pages[0].actions.is_some(), "the Rust listing dropped the actions");
    assert!(snapshot.pages[1].actions.is_none(), "a page with no actions reported some");
}

#[test]
fn rotation_is_reported_and_the_size_is_normalized() {
    // rotated_pages.pdf is four pages with identical content at 0, 90, 180 and
    // 270. The listing must report the rotation AND the unrotated size: a
    // reader using the rotated size would disagree with the page a caller
    // opens through the normalized loader.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("the rotated fixture");
    let snapshot = listed(case);

    assert_eq!(snapshot.page_count, 4);
    let rotations: Vec<PdfRotation> = snapshot.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(
        rotations,
        [
            PdfRotation::None,
            PdfRotation::Clockwise90,
            PdfRotation::Clockwise180,
            PdfRotation::Clockwise270,
        ]
    );
    for page in &snapshot.pages {
        assert_eq!(
            (page.size.width, page.size.height),
            (612.0, 792.0),
            "page {} reported a rotated size",
            page.index,
        );
    }
}

#[test]
fn listing_and_the_session_registry_agree_about_identity() {
    // The gate the port order names: page identity stable between the lazy
    // per-page lookup and the full enumeration. Two different routes to the
    // same object numbers, and they have to produce the same answer or a
    // reference minted by one is meaningless to the other.
    let file = load();
    for case in &file.cases {
        let snapshot = listed(case);

        let mut session = DocumentSession::new();
        session
            .open_with(bytes(case), case.password.as_deref(), SessionAuthority::unrestricted())
            .unwrap_or_else(|e| panic!("{}: open session: {e}", case.name));

        let records = session.all_records().expect("records").to_vec();
        assert_eq!(records.len(), snapshot.pages.len(), "{}: page counts differ", case.name);
        for (record, page) in records.iter().zip(&snapshot.pages) {
            assert_eq!(record.page_index, page.index, "{}: index", case.name);
            assert_eq!(
                record.page_object_number, page.page_object_number,
                "{}: object number at index {}",
                case.name, page.index,
            );
        }

        // And the per-page lookup route, which is the one that could diverge.
        for page in &snapshot.pages {
            let record = session
                .record_by_object_number(page.page_object_number)
                .unwrap_or_else(|e| panic!("{}: lookup: {e}", case.name));
            assert_eq!(record.page_index, page.index, "{}: lazy lookup disagreed", case.name);
        }
    }
}

#[test]
fn a_cancelled_listing_reports_aborted() {
    use epdf_engine::error::EngineErrorCode;

    let file = load();
    let document = open(&file.cases[0]);
    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        list_pages(&document, &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}

#[test]
fn the_wire_answers_the_listing_the_typescript_engine_did() {
    // Through `Host::call`, so the boundary is the one doing the converting.
    use epdf_engine::host::Host;

    let file = load();
    for case in file.cases.iter().filter(|c| c.password.is_none()) {
        let mut host = Host::new();
        let opened = host.call_filled(
            &serde_json::json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
            vec![bytes(case)],
        );
        assert!(!opened.json.contains("\"error\""), "{}: {}", case.name, opened.json);
        let answer = host.call_filled(
            &serde_json::json!({ "kind": "pages.list", "jobId": 2, "docId": "d" }).to_string(),
            Vec::new(),
        );
        let envelope: serde_json::Value = serde_json::from_str(&answer.json).expect("JSON");
        let actual = test_support::corpus::quantized(&envelope["result"]);
        let expected = test_support::corpus::quantized(&case.pages);
        if let Some(difference) = test_support::corpus::first_difference(&expected, &actual) {
            panic!("{}: {difference} ({})", case.name, case.covers);
        }
    }
}

/// A PDF whose pages are these page dictionaries' bodies, without `/Type`
/// and `/Parent`. Cross-reference offsets are computed, so PDFium reads it
/// as written.
fn pdf(pages: &[&str]) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 3 + i)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()),
    ];
    objects.extend(pages.iter().map(|body| format!("<< /Type /Page /Parent 2 0 R {body} >>")));
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn the_boxes_follow_iso_32000_and_the_crop_box_is_the_origin() {
    // `pageBoxesOf`: the media box the file writes, the crop box cut to it,
    // bleed, trim and art defaulting to the crop box; then page space from
    // the crop box's top-left.
    let document = Document::from_bytes(
        Library::acquire(),
        &pdf(&[
            "/MediaBox [0 0 612 792] /CropBox [50 60 700 732] /TrimBox [100 100 300 400] /Tabs /R",
        ]),
        None,
    )
    .expect("open");
    let read = list_pages(&document, &CancelToken::never()).expect("list");
    let page = &read.pages[0];
    // In the file's coordinates, cut to the media box.
    assert_eq!(page.boxes.crop, PdfRect { left: 50.0, bottom: 60.0, right: 612.0, top: 732.0 });
    assert_eq!(page.boxes.bleed, page.boxes.crop);
    assert_eq!(page.size, PdfSize { width: 562.0, height: 672.0 });
    assert_eq!(page.tab_order, Some(PageTabOrder::Rows));

    let listed = page_list_of(read).expect("in page space");
    let page = &listed.pages[0];
    assert_eq!(page.boxes.crop, PageRect { x: 0.0, y: 0.0, width: 562.0, height: 672.0 });
    assert_eq!(page.boxes.media, PageRect { x: -50.0, y: -60.0, width: 612.0, height: 792.0 });
    assert_eq!(page.boxes.trim, PageRect { x: 50.0, y: 332.0, width: 200.0, height: 300.0 });
    assert_eq!(page.pdf_crop_box, PdfRect { left: 50.0, bottom: 60.0, right: 612.0, top: 732.0 });
    let json = serde_json::to_value(page).expect("serialize");
    assert_eq!(json["tabOrder"], "rows");
}

#[test]
fn a_page_without_a_media_box_is_letter_and_a_rotate_off_the_quarter_turns_is_none() {
    // The engine's defaults now, not the wrapper's (Task 6 carried item):
    // no media box is US Letter, and `/Rotate 45` reads as no turn where the
    // old reduction truncated, and -90 as 270.
    let document = Document::from_bytes(
        Library::acquire(),
        &pdf(&[
            "/Rotate 45",
            "/MediaBox [0 0 100 200] /Rotate -90",
            "/MediaBox [0 0 100 200] /CropBox [200 0 300 200]",
        ]),
        None,
    )
    .expect("open");
    let read = list_pages(&document, &CancelToken::never()).expect("list");
    assert_eq!(
        read.pages[0].boxes.media,
        PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 }
    );
    assert_eq!(read.pages[0].size, PdfSize { width: 612.0, height: 792.0 });
    assert_eq!(read.pages[0].rotation, PdfRotation::None);
    assert_eq!(read.pages[0].tab_order, None);
    assert_eq!(read.pages[1].rotation, PdfRotation::Clockwise270);
    // A crop box off the page leaves a page with no size.
    assert_eq!(read.pages[2].size, PdfSize { width: 0.0, height: 0.0 });
}
