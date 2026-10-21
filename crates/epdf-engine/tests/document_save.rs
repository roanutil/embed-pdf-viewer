//! G10's save gate: a saved document reopens as the document it was.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run save
//!
//! Semantic, for the reason `multiplatform-port-order.md` G10 gives: the
//! conformance suites do not establish byte-stable PDF serialization. What is
//! compared is what the reopened document IS, which is what a save is for.
//!
//! Both modes for every case. An incremental save appends a revision to the
//! original bytes and a full save rewrites the file, so the two produce
//! different files that have to mean the same thing.

use epdf_engine::access::DocumentAccess;
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::annotation::read::list::list_annotations;
use epdf_engine::features::metadata::read_metadata;
use epdf_engine::features::pages::list_pages;
use epdf_engine::features::save::{
    LayerArtifactKind, SaveMode, save_layer_to_buffer, save_to_buffer,
};
use epdf_engine::features::text::reader::read_page_text;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
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
    original: Reopened,
    modes: std::collections::BTreeMap<String, Reopened>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Reopened {
    page_count: u32,
    pages: Vec<ReopenedPage>,
    title: Option<String>,
    author: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ReopenedPage {
    width: f64,
    height: f64,
    rotation: u16,
    text: String,
    annotation_count: u32,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("save").join("roundtrip.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("roundtrip.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "save.roundtrip");
    file
}

fn session(bytes: Vec<u8>, password: Option<String>) -> DocumentSession {
    let mut session = DocumentSession::new();
    session.open_with(bytes, password.as_deref(), SessionAuthority::unrestricted()).expect("open");
    session
}

fn fixture(case: &Case) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture))
}

/// Open bytes and say what document they are.
fn describe(bytes: Vec<u8>, password: Option<String>) -> Reopened {
    let session = session(bytes, password);
    let owned = session.document().expect("document");
    let document = owned.document();
    // `OwnedDocument` is what `PageAccess` is implemented over; the bare
    // `Document` beneath it only answers document-level questions.
    let access = owned;
    let listing = list_pages(document, &CancelToken::never()).expect("list");
    let metadata = read_metadata(document, &CancelToken::never()).expect("metadata");

    // `annotations.list`, which leaves widgets to the form (1233d1349).
    let annotations = list_annotations(owned, None, &CancelToken::never()).expect("annotations");
    let mut counts: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    for annotation in &annotations.annotations {
        *counts.entry(annotation.base.page.get()).or_default() += 1;
    }

    let pages = listing
        .pages
        .iter()
        .map(|layout| {
            let page = access.page(layout.page_object_number).expect("page");
            let text = read_page_text(&page, &CancelToken::never()).expect("text");
            ReopenedPage {
                width: layout.size.width,
                height: layout.size.height,
                rotation: layout.rotation.degrees(),
                text: text.text,
                annotation_count: counts
                    .get(&layout.page_object_number.get())
                    .copied()
                    .unwrap_or(0),
            }
        })
        .collect();

    Reopened {
        page_count: listing.page_count,
        pages,
        title: metadata.title,
        author: metadata.author,
    }
}

/// A token that is already cancelled.
fn cancelled() -> CancelToken {
    let token = CancelToken::never();
    token.cancel();
    token
}

fn mode_of(name: &str) -> SaveMode {
    match name {
        "incremental" => SaveMode::Incremental,
        // The generator keeps its `full` key for what `PdfSaveMode` spells
        // `rewrite` (`generate-save-vectors.ts`).
        "rewrite" | "full" => SaveMode::Rewrite,
        other => panic!("the vector names a save mode {other} this runner does not map"),
    }
}

#[test]
fn a_saved_document_reopens_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 5, "the corpus shrank");
    let mut saved = 0usize;

    for case in &file.cases {
        let password = case.password.clone();

        // The original first, so a case where this runner and the oracle
        // disagree about the SOURCE fails here rather than being blamed on
        // the save.
        assert_eq!(
            describe(fixture(case), password.clone()),
            case.original,
            "{}: the source document already differs: {}",
            case.name,
            case.covers,
        );

        for (name, theirs) in &case.modes {
            let session = session(fixture(case), password.clone());
            let bytes = save_to_buffer(&session, mode_of(name), &CancelToken::never())
                .unwrap_or_else(|e| panic!("{}: save {name}: {e}", case.name));
            assert!(!bytes.is_empty());
            saved += 1;

            assert_eq!(
                &describe(bytes, password.clone()),
                theirs,
                "{}: after a {name} save: {}",
                case.name,
                case.covers,
            );
        }
    }

    assert!(saved > 0, "nothing in the corpus was saved");
}

#[test]
fn a_save_changes_nothing_about_the_open_session() {
    // A save is a READ. No revision bump, no registry refresh, no events, and
    // the session goes on serving the same pages afterwards. A save that
    // mutated would make every token a caller was holding stale.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("the rotated case");
    let mut session = session(fixture(case), None);

    let before: Vec<u32> = session
        .all_records()
        .expect("records")
        .iter()
        .map(|r| r.page_object_number.get())
        .collect();
    let state_before =
        session.page_state(PageObjectNumber::new(before[0]).unwrap()).expect("page state");

    let bytes = save_to_buffer(&session, SaveMode::Rewrite, &CancelToken::never()).expect("save");
    assert!(!bytes.is_empty());

    let after: Vec<u32> = session
        .all_records()
        .expect("records")
        .iter()
        .map(|r| r.page_object_number.get())
        .collect();
    assert_eq!(before, after, "the page registry moved");
    assert_eq!(
        state_before,
        session.page_state(PageObjectNumber::new(before[0]).unwrap()).expect("page state"),
        "the revision moved",
    );
}

#[test]
fn the_two_modes_produce_different_files_that_mean_the_same_thing() {
    // The property that makes recording both worthwhile. An incremental save
    // appends to the original bytes and a full save rewrites them, so the
    // buffers differ; what they say about the document does not.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "annotations").expect("the annotations case");

    let incremental =
        save_to_buffer(&session(fixture(case), None), SaveMode::Incremental, &CancelToken::never())
            .expect("incremental");
    let full =
        save_to_buffer(&session(fixture(case), None), SaveMode::Rewrite, &CancelToken::never())
            .expect("full");

    assert_ne!(incremental, full, "the two modes produced identical bytes");
    // And an incremental save really is the original plus a revision: it
    // starts with the source file.
    let source = fixture(case);
    assert!(
        incremental.starts_with(&source),
        "an incremental save did not append to the original bytes",
    );
    assert!(!full.starts_with(&source), "a full save copied the original bytes through");

    assert_eq!(describe(incremental, None), describe(full, None));
}

#[test]
fn an_encrypted_document_saves_to_one_that_needs_the_same_password() {
    // The case most likely to come out readable-but-wrong: a save that
    // dropped the encryption dictionary produces a file that opens WITHOUT a
    // password, which looks like a success and is a disclosure.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "encrypted").expect("the encrypted case");
    let bytes = save_to_buffer(
        &session(fixture(case), Some("user".to_owned())),
        SaveMode::Rewrite,
        &CancelToken::never(),
    )
    .expect("save");

    let mut without = DocumentSession::new();
    let refused = without
        .open_with(bytes.clone(), None, SessionAuthority::unrestricted())
        .expect_err("the saved document still needs a password");
    assert!(refused.is(EngineErrorCode::DocPasswordRequired));

    // And it opens with the right one, which is the other half: a save that
    // re-encrypted under a different key would also refuse the empty
    // password.
    let mut with = DocumentSession::new();
    with.open_with(bytes, Some("user"), SessionAuthority::unrestricted())
        .expect("the saved document opens with its password");
}

#[test]
fn a_base_session_refuses_to_produce_a_layer_artifact() {
    // The one precondition this engine checks that the wrapper cannot:
    // PDFium has no way to tell a layer handle from a base one, and an
    // artifact written from a base is not a thing anyone wants.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("rotated");
    let session = session(fixture(case), None);

    for output in [LayerArtifactKind::Artifact, LayerArtifactKind::Delta] {
        let refused = save_layer_to_buffer(&session, output, &CancelToken::never())
            .expect_err("a base session is not a layer");
        assert!(refused.is(EngineErrorCode::InvalidArg));
    }
}

#[test]
fn saving_a_closed_session_is_refused() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("rotated");
    let mut session = session(fixture(case), None);
    session.close();

    let refused = save_to_buffer(&session, SaveMode::Rewrite, &CancelToken::never())
        .expect_err("a closed session has nothing to save");
    assert!(refused.is(EngineErrorCode::DocNotOpen));
}

#[test]
fn a_cancelled_save_reports_aborted() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("rotated");
    let session = session(fixture(case), None);

    let refused = save_to_buffer(&session, SaveMode::Rewrite, &cancelled())
        .expect_err("cancelled before the write");
    assert!(refused.is(EngineErrorCode::Aborted));
}

#[test]
fn a_layer_session_writes_an_artifact_that_opens_again() {
    // The round trip `multiplatform-port-status.md` recorded as untested:
    // `open_layer_delta` and `open_layer_artifact` had only refusal paths
    // because nothing produced a delta. `EPDFLayer_SaveDelta` is this group,
    // so both now have something to open.
    //
    // The layer carries no edits, because writes are G11 onward, and that
    // turns out to separate the two forms: an artifact of an empty layer
    // writes and reopens, while a bare delta of one is refused because there
    // is no overlay to serialize. Both halves are asserted below.
    use epdf_engine::session::base_registry::BaseDocumentRegistry;
    use pdfium::FileAccess;

    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("rotated");
    let bytes = fixture(case);

    let registry = BaseDocumentRegistry::new();
    let base = registry.acquire_memory("rotated", &bytes, None).expect("base");
    let mut layer = DocumentSession::new();
    layer.open_layer(base.clone(), SessionAuthority::unrestricted()).expect("open layer");

    let artifact = save_layer_to_buffer(&layer, LayerArtifactKind::Artifact, &CancelToken::never())
        .expect("artifact");
    assert!(!artifact.is_empty());

    // It reopens against the base it was written from, and the layer it
    // produces is the same document.
    let reopened_artifact = base
        .open_layer_artifact(&FileAccess::from_bytes(artifact.clone()).expect("access"))
        .expect("the artifact reopens");
    assert_eq!(reopened_artifact.document().page_count(), 4);
    drop(reopened_artifact);

    // THE BARE DELTA CANNOT BE WRITTEN YET, and not because of this port.
    // `EPDFLayer_SaveDeltaToOwnedBuffer` refuses a layer with no promoted
    // objects: there is no overlay to serialize. Writes are G11 onward, so
    // nothing in this repository can produce one, and the delta half of the
    // round trip becomes testable the day a mutator lands.
    assert_eq!(
        pdfium::BaseDocument::promoted_object_count(layer.document().expect("open").document()),
        0,
        "the layer has no edits, which is why there is no delta",
    );
    let refused = save_layer_to_buffer(&layer, LayerArtifactKind::Delta, &CancelToken::never())
        .expect_err("an empty layer has no delta to write");
    assert!(refused.is(EngineErrorCode::DocOpenFailed));

    // And the artifact refuses a base it was not written against, which is
    // the whole difference between the two forms: a bare delta carries no
    // such marker and would apply to anything.
    let other =
        fixture(file.cases.iter().find(|c| c.name == "annotations").expect("the annotations case"));
    let wrong_base = registry.acquire_memory("annotations", &other, None).expect("base");
    let refused = wrong_base
        .open_layer_artifact(&FileAccess::from_bytes(artifact).expect("access"))
        .expect_err("an artifact written against another base");
    assert_eq!(refused, pdfium::LayerOpenError::BaseMismatch);
}

#[test]
fn a_standalone_save_of_a_layer_is_a_pdf_and_not_an_artifact() {
    // The confusion the two calls exist to prevent. A standalone save of a
    // layer session copies the base bytes through and appends the delta as an
    // ordinary revision, so the result opens as a document; the artifact does
    // not, and handing one to a reader would fail.
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("rotated");
    let bytes = fixture(case);

    let registry = BaseDocumentRegistry::new();
    let base = registry.acquire_memory("rotated", &bytes, None).expect("base");
    let mut layer = DocumentSession::new();
    layer.open_layer(base, SessionAuthority::unrestricted()).expect("open layer");

    let standalone =
        save_to_buffer(&layer, SaveMode::Incremental, &CancelToken::never()).expect("save");
    assert!(standalone.starts_with(b"%PDF-"), "a standalone save is a PDF");
    assert_eq!(describe(standalone, None).page_count, 4);

    let artifact = save_layer_to_buffer(&layer, LayerArtifactKind::Artifact, &CancelToken::never())
        .expect("artifact");
    let mut as_document = DocumentSession::new();
    assert!(
        as_document.open_with(artifact, None, SessionAuthority::unrestricted()).is_err(),
        "a layer artifact is not a document and must not open as one",
    );
}
