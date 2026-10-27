//! G7: the annotation reads agree with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-read
//!
//! # What this compares
//!
//! Every annotation, whole, as the wire answers it: `annotations.list`
//! through `Host::call`, so the page-space boundary is part of what is
//! compared. Widgets left the annotation list for the form (1233d1349), and
//! the generator reads them back through `forms.list`, after each page's
//! other annotations (our 0044ea9b9). This does the same, so the vector
//! still covers every `/Annots` entry.

mod common;

use std::collections::BTreeSet;

use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Value, json};

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
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
struct Page {
    page: Value,
    annotations: Vec<Value>,
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

/// What the wire answered for a case's fixture: `annotations.list` and
/// `forms.list`.
struct Read {
    list: Value,
    widgets: Vec<Value>,
}

fn read(case: &Case) -> Read {
    let mut replay = Replay::open(&case.fixture, "annotations");
    let list = replay
        .replay(&json!({ "kind": "annotations.list", "effect": "read", "jobId": "<job>" }))
        .unwrap_or_else(|e| panic!("{}: annotations.list refused: {e}", case.name));
    let form = replay
        .replay(&json!({ "kind": "forms.list", "effect": "read", "jobId": "<job>" }))
        .unwrap_or_else(|e| panic!("{}: forms.list refused: {e}", case.name));
    let widgets = form["widgets"].as_array().cloned().unwrap_or_default();
    Read { list, widgets }
}

impl Read {
    /// One entry per listed page, as the generator records it: the page's
    /// annotations, then its widgets.
    fn pages(&self) -> Vec<(Value, Vec<Value>)> {
        let on = |page: &Value, annotation: &Value| {
            annotation["page"]["objectNumber"] == page["objectNumber"]
        };
        let annotations = self.list["annotations"].as_array().expect("annotations");
        self.list["pages"]
            .as_array()
            .expect("pages")
            .iter()
            .map(|page| {
                let all: Vec<Value> = annotations
                    .iter()
                    .chain(&self.widgets)
                    .filter(|annotation| on(page, annotation))
                    .cloned()
                    .collect();
                (page.clone(), all)
            })
            .collect()
    }

    fn all(&self) -> Vec<Value> {
        self.pages().into_iter().flat_map(|(_, annotations)| annotations).collect()
    }
}

#[test]
fn the_reads_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");
    let mut compared = 0usize;

    for case in &file.cases {
        let pages = read(case).pages();
        assert_eq!(
            pages.len(),
            case.pages.len(),
            "{}: page count differs: {}",
            case.name,
            case.covers
        );

        for (page_index, ((page, mine), theirs)) in pages.iter().zip(&case.pages).enumerate() {
            assert_eq!(page, &theirs.page, "{}: page {page_index} is a different page", case.name);
            assert_eq!(
                mine.len(),
                theirs.annotations.len(),
                "{}: page {page_index} has a different number of annotations",
                case.name,
            );
            for (index, (mine, theirs)) in mine.iter().zip(&theirs.annotations).enumerate() {
                let subtype = theirs["subtype"].as_str().expect("a subtype");
                compared += 1;
                assert_eq!(
                    test_support::canonical_numbers(mine),
                    test_support::canonical_numbers(theirs),
                    "{}: page {page_index} annotation {index} is {subtype}: {}",
                    case.name,
                    case.covers,
                );
            }
        }
    }

    assert!(compared > 0, "the corpus has no annotations at all");
}

#[test]
fn the_list_names_its_pages_and_every_annotation_names_its_page() {
    // Replaces `the_page_state_agrees_including_the_weak_flag`: upstream's
    // read has no per-page state (83f6598d2). What took its place is the
    // list's `pages`, the pages it read in their order, and each record's
    // own `page`, which is how a consumer joins a flat list back to pages.
    let file = load();
    let mut saw_empty_page = false;

    for case in &file.cases {
        let read = read(case);
        let listed = read.list["pages"].as_array().expect("pages");
        let theirs: Vec<&Value> = case.pages.iter().map(|page| &page.page).collect();
        assert_eq!(listed.iter().collect::<Vec<_>>(), theirs, "{}: the pages differ", case.name);
        for annotation in read.list["annotations"].as_array().expect("annotations") {
            assert!(
                listed.contains(&annotation["page"]),
                "{}: an annotation names a page the list did not read: {annotation}",
                case.name,
            );
            assert_eq!(annotation["ref"]["page"], annotation["page"], "{}", case.name);
            assert_ne!(annotation["subtype"], "widget", "{}: a widget in the list", case.name);
        }
        saw_empty_page |= read.pages().iter().any(|(_, annotations)| annotations.is_empty());
    }
    assert!(saw_empty_page, "no page in the corpus is empty");

    // And a list of chosen pages reads just those, in the order asked.
    let case = file.cases.iter().find(|c| c.name == "report").expect("the report case");
    let mut replay = Replay::open(&case.fixture, "pages");
    let wanted: Vec<Value> = case.pages.iter().rev().take(2).map(|p| p.page.clone()).collect();
    let list = replay
        .replay(&json!({ "kind": "annotations.list", "effect": "read", "jobId": "<job>",
            "pages": wanted }))
        .expect("a list of chosen pages");
    assert_eq!(list["pages"], json!(wanted));
}

#[test]
fn a_page_the_document_does_not_have_is_not_found() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "base").expect("the base case");
    let mut replay = Replay::open(&case.fixture, "missing");
    let refused = replay
        .replay(&json!({ "kind": "annotations.list", "effect": "read", "jobId": "<job>",
            "pages": [{ "kind": "objectNumber", "objectNumber": 9999 }] }))
        .expect_err("a page nothing has");
    assert_eq!(refused["code"], "NotFound", "{refused}");
}

#[test]
fn the_corpus_covers_every_subtype_that_has_a_reader() {
    // A comparison over a corpus that never produces a subtype says nothing
    // about that subtype's reader. Every subtype of the catalog has one now,
    // `popup` included (c78997346).
    let file = load();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for case in &file.cases {
        for page in &case.pages {
            for annotation in &page.annotations {
                seen.insert(annotation["subtype"].as_str().expect("a subtype").to_string());
            }
        }
    }

    for subtype in epdf_engine::features::annotation::subtype::ANNOTATION_SUBTYPES {
        assert!(
            seen.contains(subtype.as_str()),
            "no annotation in the corpus is a {}, so its reader is never run",
            subtype.as_str(),
        );
    }
}

#[test]
fn the_subtypes_with_no_reader_read_as_unsupported_on_both_sides() {
    // A `/Subtype` the catalog does not name (`annotation_base.pdf`'s
    // `/Sound`) has no reader on either side, and both answer `unsupported`
    // with its code and name. A subtype one engine modelled and the other
    // did not would be a wire difference nothing else here would catch.
    let file = load();
    let mut saw = 0usize;

    for case in &file.cases {
        let mine = read(case).all();
        let theirs: Vec<&Value> = case.pages.iter().flat_map(|p| &p.annotations).collect();
        for (mine, theirs) in mine.iter().zip(theirs) {
            if theirs["subtype"] != "unsupported" {
                continue;
            }
            saw += 1;
            assert_eq!(mine["subtype"], "unsupported", "{}", case.name);
            assert_eq!(mine["rawSubtypeCode"], theirs["rawSubtypeCode"], "{}", case.name);
            assert_eq!(mine["rawSubtypeName"], theirs["rawSubtypeName"], "{}", case.name);
        }
    }

    assert!(saw > 0, "the corpus has no subtype the catalog does not name");
}

#[test]
fn a_widget_is_joined_to_the_field_that_owns_it() {
    // Through `forms.list`, as the generator reads widgets. The pair in the
    // fixture is what makes this a test rather than a coincidence: one
    // widget reachable from `/AcroForm` and one inert, both on the same
    // page, and a join that stamped everything or nothing would agree with
    // one of them.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "form").expect("the form fixture");
    let widgets = read(case).widgets;
    let by_name = |nm: &str| {
        widgets
            .iter()
            .find(|a| a["nm"].as_str() == Some(nm))
            .unwrap_or_else(|| panic!("no widget named {nm}"))
    };

    let attached = by_name("a-real-field-widget");
    assert_eq!(attached["subtype"], "widget");
    assert_eq!(attached["field"]["kind"], "objectNumber");
    assert!(
        attached["field"]["objectNumber"].as_u64().is_some_and(|n| n > 0),
        "an attached widget reports no field",
    );
    assert_eq!(attached["fieldFamily"], "text");

    let inert = by_name("a-bare-widget");
    assert_eq!(inert["field"], Value::Null);
    assert_eq!(inert["fieldFamily"], "unknown");

    // This widget has an object number, so it reaches the lookup rather than
    // being skipped for want of a key, and the lookup has to answer that it
    // belongs to nothing. The document has one field, so a join that took
    // the first field would agree with the attached widget and differ here.
    let orphan = by_name("an-orphan-widget");
    assert_eq!(orphan["ref"]["kind"], "objectNumber");
    assert_eq!(orphan["field"], Value::Null);
    assert_eq!(orphan["fieldFamily"], "unknown");
}

#[test]
fn a_rect_is_reported_in_the_corner_order_the_file_wrote() {
    // `pageBoxOf` does not normalize: a `/Rect` whose corners run the other
    // way reads with a negative width and height, so a writer can leave it
    // alone. `annotation_base.pdf` has one, `[200 720 180 700]`.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "base").expect("the base fixture");
    let annotations = read(case).all();

    let reversed = annotations
        .iter()
        .find(|a| a["rect"]["width"].as_f64().is_some_and(|w| w < 0.0))
        .expect("the fixture has a /Rect in reversed corner order");
    assert_eq!(reversed["rect"], json!({ "x": 200.0, "y": 72.0, "width": -20.0, "height": -20.0 }));
}

#[test]
fn a_reply_edge_carries_a_type_exactly_when_it_carries_a_parent() {
    // `reply` is `{to, type}` or `null`. PDFium answers `/RT`'s default for
    // an annotation with no `/IRT` at all, so a reader that asked without
    // gating would report a reply on every top-level annotation.
    let file = load();
    let mut saw_reply = false;
    let mut saw_group = false;
    let mut saw_none = false;

    for case in &file.cases {
        for annotation in read(case).all() {
            let reply = &annotation["reply"];
            if reply.is_null() {
                saw_none = true;
                continue;
            }
            assert!(reply["to"].is_object(), "{}: a reply to nothing: {annotation}", case.name);
            match reply["type"].as_str() {
                Some("reply") => saw_reply = true,
                Some("group") => saw_group = true,
                other => panic!("{}: unknown reply type {other:?}", case.name),
            }
        }
    }

    assert!(saw_reply && saw_group && saw_none, "the corpus does not cover all three cases");
}

const KINDS: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

/// The `/NM`s and refs `annotations.list` answers for page 3.
fn named_refs(replay: &mut Replay) -> Vec<(String, Value)> {
    let list = replay
        .replay(&json!({ "kind": "annotations.list", "effect": "read", "jobId": "<job>" }))
        .expect("annotations.list");
    list["annotations"]
        .as_array()
        .expect("annotations")
        .iter()
        .map(|a| (a["nm"].as_str().unwrap_or_default().to_owned(), a["ref"].clone()))
        .collect()
}

fn redact(replay: &mut Replay, refs: &[&Value]) -> Result<Value, Value> {
    replay.replay(&json!({ "kind": "redaction.apply", "effect": "contentWrite",
        "jobId": "<job>", "opId": "<op>", "scope": { "annotations": refs } }))
}

#[test]
fn several_base_index_refs_on_one_page_redact_together() {
    // Every ref of a page resolves before any applies, as upstream's
    // `applyRefsOnPage` does, so a second `baseIndex` still names the
    // marking it was born as.
    let mut replay = Replay::open(KINDS, "redact");
    let refs = named_refs(&mut replay);
    let of = |nm: &str| &refs.iter().find(|(n, _)| n == nm).expect(nm).1;
    let text = of("a-text-redaction");
    let area = of("an-area-redaction");
    assert_eq!(text["kind"], "baseIndex");
    assert_eq!(area["kind"], "baseIndex");

    let applied = redact(&mut replay, &[text, area]).expect("both markings apply");
    assert_eq!(applied["result"]["results"][0]["status"], "applied", "{applied}");
    let left: Vec<String> = named_refs(&mut replay).into_iter().map(|(nm, _)| nm).collect();
    assert!(!left.contains(&"a-text-redaction".to_owned()), "{left:?}");
    assert!(!left.contains(&"an-area-redaction".to_owned()), "{left:?}");
    assert!(left.contains(&"an-autosize-redaction".to_owned()), "the third was not named");
}

#[test]
fn a_promoted_inline_annotation_keeps_its_birth_name() {
    // On a layer page whose inline annotations were promoted to objects
    // (`EPDFPage_PromoteInlineAnnotsRaw`), each still reads by the position
    // it was born at (`EPDFLayer_GetBirthName`), and that `baseIndex`
    // resolves to the object it became (`EPDFLayer_GetBirthObjectNumber`).
    use epdf_engine::host::Host;
    use epdf_engine::session::registry::SessionKey;

    let mut host = Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.layerMemBase", "effect": "open", "jobId": 1, "docId": "layer",
            "baseKey": "base-layer", "layer": { "kind": "fresh" }, "password": null })
        .to_string(),
        vec![test_support::fixture(KINDS)],
    );
    let opened: Value = serde_json::from_str(&opened.json).expect("JSON");
    assert!(opened.get("error").is_none(), "{opened}");
    let mut replay = Replay::adopt(host, "layer");

    let before = named_refs(&mut replay);
    let document = replay
        .host
        .sessions()
        .session(&SessionKey::document("layer"))
        .expect("the layer session")
        .document()
        .expect("its document")
        .document();
    let moved = document.promote_inline_annotations_raw(0).expect("the page promotes");
    assert!(moved > 0, "nothing on the page was inline");

    let after = named_refs(&mut replay);
    assert_eq!(after, before, "a promoted annotation changed its name");

    let text = &after.iter().find(|(nm, _)| nm == "a-text-redaction").expect("the marking").1;
    let area = &after.iter().find(|(nm, _)| nm == "an-area-redaction").expect("the marking").1;
    assert_eq!(text["kind"], "baseIndex", "{text}");
    let applied = redact(&mut replay, &[text, area]).expect("the birth names resolve");
    assert_eq!(applied["result"]["results"][0]["status"], "applied", "{applied}");
    let left: Vec<String> = named_refs(&mut replay).into_iter().map(|(nm, _)| nm).collect();
    assert!(!left.contains(&"a-text-redaction".to_owned()), "{left:?}");
    assert!(left.contains(&"an-autosize-redaction".to_owned()), "{left:?}");
}

// ── `runAnnotationReadConformance` and `runAnnotationRotationConformance` ──

const DEMO: &str = "examples/engine-runtime-demo/public/annotations.pdf";

fn list_of(replay: &mut Replay, pages: Option<Value>) -> Value {
    let mut request = json!({ "kind": "annotations.list", "effect": "read", "jobId": "<job>" });
    if let Some(pages) = pages {
        request["pages"] = pages;
    }
    replay.replay(&request).expect("annotations.list")
}

#[test]
fn the_demo_pages_annotations_have_the_counts_and_fields_of_their_families() {
    // `packages/engine/main/test/annotations.test.ts`: page 3 of
    // `annotations.pdf` holds 14 annotations of these kinds.
    let mut replay = Replay::open(DEMO, "demo");
    let page = json!({ "kind": "objectNumber", "objectNumber": 3 });
    let all = list_of(&mut replay, None);
    let on_page =
        all["annotations"].as_array().unwrap().iter().filter(|a| a["page"] == page).count();
    assert_eq!(on_page, 14);

    let list = list_of(&mut replay, Some(json!([page])));
    assert_eq!(list["pages"], json!([page]));
    let annotations = list["annotations"].as_array().unwrap();
    assert_eq!(annotations.len(), 14);
    let count = |subtype: &str| annotations.iter().filter(|a| a["subtype"] == subtype).count();
    for (subtype, at_least) in [
        ("highlight", 4),
        ("unsupported", 1),
        ("circle", 1),
        ("square", 1),
        ("polygon", 1),
        ("polyline", 1),
        ("line", 1),
        ("ink", 1),
        ("free-text", 2),
        ("caret", 1),
    ] {
        assert!(count(subtype) >= at_least, "{subtype}: {}", count(subtype));
    }

    for a in annotations {
        let has = |key: &str| a.as_object().unwrap().contains_key(key);
        match a["subtype"].as_str().unwrap() {
            "ink" => {
                assert!(a["color"].is_string() && a["strokeWidth"].is_number(), "{a}");
                assert!(!has("interiorColor"), "{a}");
                assert!(a["inkList"][0][0]["x"].is_number(), "{a}");
            }
            "free-text" => {
                assert!(a["fontFamily"].is_string() && a["fontSize"].is_number(), "{a}");
                assert!(a["color"].is_string() && has("interiorColor"), "{a}");
            }
            "caret" => {
                assert!(a["color"].is_string() && a["opacity"].is_number(), "{a}");
                assert!(!has("strokeWidth") && !has("quadPoints"), "{a}");
            }
            "circle" | "square" | "polygon" | "polyline" | "line" => {
                assert!(a["color"].is_string() && a["strokeWidth"].is_number(), "{a}");
                assert!(a["borderStyle"].is_string() && has("interiorColor"), "{a}");
            }
            _ => {}
        }
    }

    // An annotation born inline is named by its `baseIndex`, and reading
    // by that name again finds the same annotation.
    let inline =
        annotations.iter().find(|a| a["ref"]["kind"] == "baseIndex").expect("an inline one");
    let again = list_of(&mut replay, Some(json!([page])));
    assert!(again["annotations"].as_array().unwrap().iter().any(|a| a == inline));
}

/// The first page's crop box in the file's coordinates, and a page-space
/// box from file-space edges measured on it (`pageBoxOf`).
fn page_box(replay: &mut Replay, [left, bottom, right, top]: [f64; 4]) -> [f64; 4] {
    let pages = replay
        .replay(&json!({ "kind": "pages.list", "effect": "read", "jobId": "<job>" }))
        .expect("pages.list");
    let crop = &pages["pages"][0]["pdfCropBox"];
    let (crop_left, crop_top) = (crop["left"].as_f64().unwrap(), crop["top"].as_f64().unwrap());
    [left - crop_left, crop_top - top, right - left, top - bottom]
}

fn assert_box(actual: &Value, expected: [f64; 4], tolerance: f64) {
    for (edge, want) in ["x", "y", "width", "height"].into_iter().zip(expected) {
        let got = actual[edge].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() <= tolerance, "{edge}: {got} against {want} in {actual}");
    }
}

#[test]
fn a_stamp_acrobat_turned_reads_its_turn_and_its_own_box() {
    let mut replay =
        Replay::open("packages/engine/main/test/fixtures/stamp-rotated-acrobat.pdf", "stamp");
    let list = list_of(&mut replay, None);
    let stamp = list["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["subtype"] == "stamp")
        .expect("a stamp")
        .clone();
    assert_eq!(stamp["rotation"], 36.0, "{stamp}");
    // Acrobat stores no box: it's the drawing's own, stretched onto /Rect.
    let content_box = page_box(&mut replay, [155.784, 508.966, 466.366, 618.542]);
    assert_box(&stamp["box"], content_box, 0.02);
    let rect = page_box(&mut replay, [153.238, 428.152, 468.912, 699.356]);
    assert_box(&stamp["rect"], rect, 0.01);
}

#[test]
fn a_text_box_acrobat_turned_a_quarter_reads_its_turn() {
    let mut replay =
        Replay::open("packages/engine/main/test/fixtures/freetext-rotated-acrobat.pdf", "text");
    let list = list_of(&mut replay, None);
    let by_contents = |contents: &str| {
        list["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["subtype"] == "free-text" && a["contents"] == contents)
            .unwrap_or_else(|| panic!("no text box {contents:?}"))
            .clone()
    };
    // /Rotate 90 is counterclockwise: the text reads from the bottom up.
    let turned = by_contents("test 123");
    assert_eq!(turned["rotation"], 270.0, "{turned}");
    let content_box = page_box(&mut replay, [233.423, 309.413, 341.423, 341.423]);
    assert_box(&turned["box"], content_box, 0.01);
    let upright = by_contents("test\rtest 123");
    assert_eq!(upright["rotation"], Value::Null);
    assert_eq!(upright["box"], upright["rect"]);
}

// ── `runColorConformance`'s read case ───────────────────────────────────────

/// A PDF file of `objects`, numbered from 1, with its cross-reference table
/// (`pdfOf`).
fn pdf_of(objects: &[&str]) -> Vec<u8> {
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
fn a_gray_or_cmyk_color_reads_as_its_srgb_value_lowercase() {
    // `COLOR_FIXTURE_PDF`: a square whose border the file stores as gray
    // (`/C [0.5]`) and whose fill as CMYK (`/IC [0 0 0 1]`).
    let bytes = pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Square /Rect [20 20 120 120] /F 4 /C [0.5] /IC [0 0 0 1] \
         /NM (colored) >>",
    ]);
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "color",
            "password": null })
        .to_string(),
        vec![bytes],
    );
    let opened: Value = serde_json::from_str(&opened.json).expect("JSON");
    assert!(opened.get("error").is_none(), "{opened}");
    let mut replay = Replay::adopt(host, "color");
    let list = list_of(&mut replay, None);
    let square = &list["annotations"][0];
    assert_eq!(square["nm"], "colored", "{list}");

    let color = square["color"].as_str().expect("a colour");
    let gray = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        && color[1..3] == color[3..5]
        && color[3..5] == color[5..7];
    assert!(gray, "{color} is not a lowercase gray");
    assert_eq!(square["interiorColor"], "#000000");
}

// ── `ANNOTATION_FIELD_SPACES` ───────────────────────────────────────────────

/// Each kind's `spaces` in upstream's `kind-facts.generated.ts`.
fn upstream_field_spaces() -> Vec<(String, BTreeSet<(String, String)>)> {
    let path = test_support::workspace_root()
        .join("packages/engine/core/src/annotation/kind-facts.generated.ts");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut kinds = Vec::new();
    let mut kind: Option<String> = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        // A kind opens at two spaces of indent: `  square: {` or
        // `  'free-text': {`.
        if let Some(rest) = line.strip_prefix("  ")
            && !rest.starts_with(' ')
            && let Some(name) = rest.strip_suffix(": {")
        {
            kind = Some(name.trim_matches('\'').to_owned());
        }
        if line.trim() == "spaces: {" {
            let mut spaces = BTreeSet::new();
            for entry in lines.by_ref() {
                if entry.trim().starts_with('}') {
                    break;
                }
                let (name, space) = entry.trim().trim_end_matches(',').split_once(": ").unwrap();
                spaces.insert((
                    name.trim_matches('\'').to_owned(),
                    space.trim_matches('\'').to_owned(),
                ));
            }
            kinds.push((kind.clone().expect("spaces outside a kind"), spaces));
        }
    }
    kinds
}

#[test]
fn the_field_spaces_are_upstreams() {
    use epdf_engine::page_space::annotations::field_spaces;
    let upstream = upstream_field_spaces();
    assert_eq!(upstream.len(), 20, "a kind came or went: {upstream:?}");
    for (kind, theirs) in upstream {
        let mine: BTreeSet<(String, String)> = field_spaces(&kind)
            .iter()
            .map(|(name, space)| ((*name).to_owned(), space.as_str().to_owned()))
            .collect();
        assert_eq!(mine, theirs, "{kind}");
    }
}
