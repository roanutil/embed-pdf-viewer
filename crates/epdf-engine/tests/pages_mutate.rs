//! G13's first gate: a page list is reordered, rotated and thinned the way
//! the TypeScript engine does it, and a chosen set of one page's annotations
//! is flattened or exported the way it does.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate
//!
//! # What the comparison watches
//!
//! Which OBJECT NUMBER ends up at which index. An index is a position and
//! only the object number is identity, so a reorder that produced the right
//! shape with the wrong pages in it would otherwise pass.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::pages_write::{
    FlattenUsage, delete_pages, flatten_pages, insert_blank_pages, insert_pages_from, move_pages,
    rotate_pages,
};
use epdf_engine::geom::PdfRotation;
use epdf_engine::identity::{AnnotationRef, PageObjectNumber, PageRef};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(rename = "move")]
    move_: Option<MoveInput>,
    rotate: Option<RotateInput>,
    delete: Option<DeleteInput>,
    insert_blank: Option<InsertBlankInput>,
    insert: Option<InsertInput>,
    flatten: Option<FlattenInput>,
    flatten_annotations: Option<FlattenAnnotationsInput>,
    export_appearance: Option<ExportAppearanceInput>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    layout: Option<serde_json::Value>,
    /// The pages an insert handed out. Absent for the operations that add
    /// nothing.
    #[serde(default)]
    inserted: Option<Vec<PageRef>>,
    /// Per-page flatten outcomes, for the cases that flatten.
    #[serde(default)]
    flattened: Option<serde_json::Value>,
    /// `annotations.flatten`'s result less its `meta`, whether there was a
    /// `meta` (an apply is a mutation and a skip is not), and how many
    /// annotations the page has afterwards.
    #[serde(default)]
    annotations_flattened: Option<serde_json::Value>,
    #[serde(default)]
    applied: Option<bool>,
    #[serde(default)]
    remaining: Option<u32>,
    /// The layout of the document `annotations.exportAppearance` produced.
    #[serde(default)]
    exported: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlattenAnnotationsInput {
    fixture: String,
    page: PageRef,
    nms: Vec<String>,
    usage: String,
    #[serde(default)]
    ref_page: Option<PageRef>,
}

#[derive(Debug, Deserialize)]
struct ExportAppearanceInput {
    fixture: String,
    page: PageRef,
    nms: Vec<String>,
}

fn nm_refs(page: PageRef, nms: &[String]) -> Vec<AnnotationRef> {
    let page = page.object_number();
    nms.iter().map(|nm| AnnotationRef::Nm { page, nm: nm.clone() }).collect()
}

/// The per-annotation flatten verbs, which need the session rather than
/// the bare document: an apply bumps the page's revision.
fn run_annotation_case(
    session: &mut DocumentSession,
    case: &Case,
) -> Option<Result<serde_json::Value, epdf_engine::error::EngineError>> {
    use epdf_engine::access::{DocumentAccess, PageAccess};
    use epdf_engine::features::annotation::write::mutator;
    let cancel = CancelToken::never();
    if let Some(input) = &case.flatten_annotations {
        let usage = match input.usage.as_str() {
            "print" => FlattenUsage::Print,
            "display" => FlattenUsage::Display,
            other => panic!("{}: unhandled usage {other:?}", case.name),
        };
        let page = input.page.object_number();
        let refs = nm_refs(input.ref_page.unwrap_or(input.page), &input.nms);
        return Some(mutator::flatten(session, page, &refs, usage, &cancel).map(|result| {
            let mut answer = serde_json::to_value(&result).expect("serialize");
            let meta = answer.as_object_mut().expect("an object").remove("meta");
            let owned = session.document().expect("open");
            let remaining = owned.page(page).expect("the page").view().annotation_count();
            serde_json::json!({
                "annotationsFlattened": answer,
                "applied": meta.is_some_and(|m| !m.is_null()),
                "remaining": remaining,
            })
        }));
    }
    if let Some(input) = &case.export_appearance {
        let page = input.page.object_number();
        let refs = nm_refs(input.page, &input.nms);
        return Some(mutator::export_appearance(session, page, &refs, &cancel).map(|bytes| {
            let mut copy = DocumentSession::new();
            copy.open_with(bytes, None, SessionAuthority::unrestricted())
                .expect("the export opens");
            let owned = copy.document().expect("open");
            let listing =
                epdf_engine::features::pages::list_pages(owned.document(), &cancel).expect("list");
            let listing = serde_json::to_value(&listing).expect("serialize");
            serde_json::json!({ "exported": listing["pages"] })
        }));
    }
    None
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlattenInput {
    /// A flatten needs a page with annotations on it, which the page-order
    /// fixture has none of, so these cases name their own.
    fixture: String,
    pages: Vec<PageRef>,
    usage: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InsertBlankInput {
    size: SizeInput,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    dest_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SizeInput {
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InsertInput {
    source: String,
    #[serde(default)]
    dest_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInput {
    pages: Vec<PageRef>,
    dest_index: u32,
}

#[derive(Debug, Deserialize)]
struct RotateInput {
    pages: Vec<PageRef>,
    rotation: u16,
}

#[derive(Debug, Deserialize)]
struct DeleteInput {
    pages: Vec<PageRef>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("pages").join("mutate.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("mutate.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.mutate");
    file
}

fn pages(refs: &[PageRef]) -> Vec<PageObjectNumber> {
    refs.iter().map(|page| page.object_number()).collect()
}

/// Run one case against a fresh copy of the fixture.
fn run(
    file: &VectorFile,
    case: &Case,
) -> Result<serde_json::Value, epdf_engine::error::EngineError> {
    let fixture = case
        .flatten
        .as_ref()
        .map(|f| f.fixture.as_str())
        .or(case.flatten_annotations.as_ref().map(|f| f.fixture.as_str()))
        .or(case.export_appearance.as_ref().map(|f| f.fixture.as_str()))
        .unwrap_or(file.fixture.as_str());
    let bytes = std::fs::read(test_support::workspace_root().join(fixture))
        .unwrap_or_else(|e| panic!("{}: read {fixture}: {e}", case.name));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    if let Some(answer) = run_annotation_case(&mut session, case) {
        return answer;
    }
    let owned = session.document().expect("open");
    let cancel = CancelToken::never();

    if let Some(input) = &case.flatten {
        let usage = match input.usage.as_str() {
            "print" => FlattenUsage::Print,
            "display" => FlattenUsage::Display,
            other => panic!("{}: unhandled usage {other:?}", case.name),
        };
        let result = flatten_pages(
            owned.document(),
            |page| {
                use epdf_engine::access::DocumentAccess;
                owned.page(page)
            },
            &pages(&input.pages),
            usage,
            &cancel,
        )?;
        return Ok(serde_json::json!({ "flattened": result }));
    }

    if let Some(input) = &case.insert_blank {
        let result = insert_blank_pages(
            owned.document(),
            input.size.width,
            input.size.height,
            input.count.unwrap_or(1),
            input.dest_index,
            &cancel,
        )?;
        return Ok(serde_json::json!({
            "layout": result.layout,
            "inserted": result.inserted_page_object_numbers.iter().copied().map(PageRef::new).collect::<Vec<_>>(),
        }));
    }
    if let Some(input) = &case.insert {
        // The source document is held for the whole call and dropped with
        // this scope. It has to outlive the import, because an imported
        // stream still reads through it; it does NOT have to outlive this
        // test, which never saves.
        let source_bytes = std::fs::read(test_support::workspace_root().join(&input.source))
            .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, input.source));
        let mut source_session = DocumentSession::new();
        source_session
            .open_with(source_bytes, None, SessionAuthority::unrestricted())
            .expect("the source opens");
        let source = source_session.document().expect("open");
        let result =
            insert_pages_from(owned.document(), source.document(), input.dest_index, &cancel)?;
        return Ok(serde_json::json!({
            "layout": result.layout,
            "inserted": result.inserted_page_object_numbers.iter().copied().map(PageRef::new).collect::<Vec<_>>(),
        }));
    }

    let result = if let Some(input) = &case.move_ {
        move_pages(owned.document(), &pages(&input.pages), input.dest_index, &cancel)?
    } else if let Some(input) = &case.rotate {
        let rotation = PdfRotation::try_from(input.rotation).expect("a /Rotate value");
        rotate_pages(owned.document(), &pages(&input.pages), rotation, &cancel)?
    } else {
        let input = case.delete.as_ref().expect("an operation");
        delete_pages(owned.document(), &pages(&input.pages), &cancel)?
    };
    Ok(serde_json::to_value(&result.layout).expect("serialize"))
}

#[test]
fn a_page_mutation_leaves_the_layout_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 12, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let mine = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        if let Some(flattened) = mine.get("annotationsFlattened") {
            assert_eq!(
                test_support::canonical_numbers(flattened),
                test_support::canonical_numbers(
                    case.annotations_flattened.as_ref().expect("recorded flatten results")
                ),
                "{}: {}",
                case.name,
                case.covers,
            );
            assert_eq!(mine["applied"].as_bool(), case.applied, "{}: applied", case.name);
            assert_eq!(
                mine["remaining"].as_u64().map(|n| n as u32),
                case.remaining,
                "{}: annotations left on the page",
                case.name,
            );
            continue;
        }
        if let Some(exported) = mine.get("exported") {
            assert_eq!(
                test_support::canonical_numbers(exported),
                test_support::canonical_numbers(
                    case.exported.as_ref().expect("a recorded exported layout")
                ),
                "{}: {}",
                case.name,
                case.covers,
            );
            continue;
        }
        // An insert answers with two things; everything else with one.
        if let Some(flattened) = mine.get("flattened") {
            assert_eq!(
                test_support::canonical_numbers(flattened),
                test_support::canonical_numbers(
                    case.flattened.as_ref().expect("recorded flatten results")
                ),
                "{}: {}",
                case.name,
                case.covers,
            );
            continue;
        }
        let (layout, inserted) = match mine.get("layout") {
            Some(layout) => (layout.clone(), mine.get("inserted").cloned()),
            None => (mine, None),
        };
        assert_eq!(
            test_support::canonical_numbers(&layout),
            test_support::canonical_numbers(case.layout.as_ref().expect("a recorded layout")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            inserted,
            case.inserted.as_ref().map(|n| serde_json::to_value(n).expect("serialize")),
            "{}: the inserted page numbers differ",
            case.name,
        );
    }

    assert!(applied >= 8, "the applied list shrank");
}

#[test]
fn a_refused_page_mutation_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let error = run(&file, case).expect_err(&case.name);
        let expected = match case.refused_with.as_deref() {
            Some("NotFound") => EngineErrorCode::NotFound,
            Some("InvalidArg") => EngineErrorCode::InvalidArg,
            Some("InvalidReference") => EngineErrorCode::InvalidReference,
            other => panic!("{}: unhandled refusal code {other:?}", case.name),
        };
        assert!(error.is(expected), "{}: {} (got {error})", case.name, case.covers);
    }

    assert!(refusals >= 5, "the refusal list shrank");
}

#[test]
fn a_destination_past_the_end_says_which_end() {
    // PDFium refuses this one too, with the same code, and its refusal is
    // about the request as a whole. The message here names the frame the
    // destination is measured in, which is the thing a caller gets wrong:
    // the index space AFTER the block is detached, not before.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "move-past-the-end-refused").expect("the case");
    let message = run(&file, case).expect_err("past the end").to_string();
    assert!(
        message.contains("after the block is detached"),
        "the message does not name the frame: {message}",
    );
}

#[test]
fn a_refused_mutation_changes_nothing() {
    // The half the vector cannot show. A list naming one page that does not
    // exist must leave the others alone, and the only way to see that is to
    // ask the same document afterwards.
    let file = load();
    for name in ["rotate-unknown-page-refused", "delete-unknown-page-refused"] {
        let case = file.cases.iter().find(|c| c.name == name).expect(name);
        let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
            .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
        let mut session = DocumentSession::new();
        session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");

        let before =
            epdf_engine::features::pages::list_pages(owned.document(), &CancelToken::never())
                .expect("the pages list");

        let cancel = CancelToken::never();
        let error = if let Some(input) = &case.rotate {
            let rotation = PdfRotation::try_from(input.rotation).expect("a /Rotate value");
            rotate_pages(owned.document(), &pages(&input.pages), rotation, &cancel).unwrap_err()
        } else {
            let input = case.delete.as_ref().expect("an operation");
            delete_pages(owned.document(), &pages(&input.pages), &cancel).unwrap_err()
        };
        assert!(error.is(EngineErrorCode::NotFound), "{name}: {error}");

        let after =
            epdf_engine::features::pages::list_pages(owned.document(), &CancelToken::never())
                .expect("the pages list");
        assert_eq!(before, after, "{name}: the refusal changed the document");
    }
}
