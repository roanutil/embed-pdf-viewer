//! G3's lifecycle gate: open, wrong password, permission, close, leases.
//!
//! `multiplatform-port-order.md` G3 asks for "open, wrong password,
//! permission, close, cancel traces; owner-thread confinement test; balanced
//! acquire/release; close rejects new work and waits for outstanding leases
//! and finalization". Confinement is covered by
//! `epdf-uniffi/tests/native_surface.rs`, which drives a real owner thread.
//! The rest is here.
//!
//! The written contract these assert against is
//! `docs/conventions/engine-lifecycle.md`.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::revision::WeakAnnotationState;
use epdf_engine::session::document::{DocumentSession, OpenKind};

const ENCRYPTED: &str = "tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf";

fn bytes(relative: &str) -> Vec<u8> {
    let path = test_support::workspace_root().join(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn report() -> Vec<u8> {
    test_support::fixture(test_support::REPORT)
}

fn opened() -> DocumentSession {
    let mut session = DocumentSession::new();
    session.open(report(), None).expect("open");
    session
}

fn first_page(session: &mut DocumentSession) -> PageObjectNumber {
    session.all_records().expect("records")[0].page_object_number
}

#[test]
fn a_fresh_session_is_neither_open_nor_locked() {
    let session = DocumentSession::new();
    assert!(!session.is_open());
    assert!(!session.is_locked());
    assert_eq!(session.kind(), None);
    assert_eq!(session.page_count().expect_err("no document").code, EngineErrorCode::DocNotOpen,);
}

#[test]
fn sessions_get_distinct_ids() {
    // The whole reason the id exists: a revision token minted by one session
    // must not validate against another.
    let a = DocumentSession::new();
    let b = DocumentSession::new();
    assert_ne!(a.session_id(), b.session_id());
}

#[test]
fn opening_twice_is_refused_rather_than_leaking_the_first_document() {
    let mut session = opened();
    assert_eq!(
        session.open(report(), None).expect_err("already open").code,
        EngineErrorCode::InvalidArg,
    );
    // And the first document is still the one that is open.
    assert_eq!(session.page_count().expect("still open"), 14);
    assert_eq!(session.kind(), Some(OpenKind::FatMemory));
}

#[test]
fn a_missing_password_and_a_wrong_one_are_different_answers() {
    // A prompt can act on the first. Only the second means the user got it
    // wrong, and collapsing them makes a prompt impossible to write.
    let mut session = DocumentSession::new();
    assert_eq!(
        session.open(bytes(ENCRYPTED), None).expect_err("no password").code,
        EngineErrorCode::DocPasswordRequired,
    );
    assert_eq!(
        session.open(bytes(ENCRYPTED), Some("wrong")).expect_err("wrong password").code,
        EngineErrorCode::DocPasswordIncorrect,
    );
    session.open(bytes(ENCRYPTED), Some("user")).expect("the right password");
    assert!(session.is_open());
}

#[test]
fn a_locked_session_holds_its_bytes_and_refuses_work() {
    let mut session = DocumentSession::new();
    session.park_locked(bytes(ENCRYPTED)).expect("park");

    assert!(session.is_locked());
    assert!(!session.is_open());
    assert_eq!(session.locked_bytes().expect("bytes").len(), bytes(ENCRYPTED).len());

    // Locked is not the same answer as closed: a caller that sees
    // DocPasswordRequired knows to prompt, where DocNotOpen tells it to give
    // up.
    assert_eq!(
        session.page_count().expect_err("locked").code,
        EngineErrorCode::DocPasswordRequired,
    );
}

#[test]
fn unlocking_consumes_the_parked_bytes() {
    let mut session = DocumentSession::new();
    session.park_locked(bytes(ENCRYPTED)).expect("park");
    session.unlock("user").expect("unlock");

    assert!(session.is_open());
    assert!(!session.is_locked());
    assert_eq!(session.page_count().expect("open"), 1);
}

#[test]
fn a_failed_unlock_leaves_the_session_locked() {
    // Otherwise a mistyped password costs the caller another transfer of the
    // whole document, which for a large file is the expensive half of opening
    // it.
    let mut session = DocumentSession::new();
    session.park_locked(bytes(ENCRYPTED)).expect("park");

    assert_eq!(
        session.unlock("wrong").expect_err("wrong password").code,
        EngineErrorCode::DocPasswordIncorrect,
    );
    assert!(session.is_locked(), "the bytes were dropped on a failed attempt");
    session.unlock("user").expect("the right password, without retransferring");
}

#[test]
fn the_page_registry_maps_index_to_durable_identity() {
    let mut session = opened();
    let records = session.all_records().expect("records").to_vec();

    assert_eq!(records.len(), 14);
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.page_index, index as u32, "records are in display order");
        assert!(record.page_object_number.get() > 0, "an index is not identity");
    }

    let first = records[0];
    assert_eq!(session.record_by_object_number(first.page_object_number).expect("lookup"), first,);
}

#[test]
fn a_page_the_document_does_not_have_is_not_found() {
    let mut session = opened();
    let missing = PageObjectNumber::new(99_999).expect("number");
    assert_eq!(
        session.record_by_object_number(missing).expect_err("no such page").code,
        EngineErrorCode::NotFound,
    );
}

#[test]
fn refreshing_the_registry_keeps_revisions_and_weak_state() {
    // Both are keyed by durable object number and survive a page reorder,
    // which is the reason refresh clears one map and not the others.
    let mut session = opened();
    let page = first_page(&mut session);
    let token = session.bump_revision(page).expect("bump");
    session.record_weak_flag(page, true).expect("record");

    session.refresh_page_registry().expect("refresh");

    session.validate_revision(&token).expect("the token survived");
    assert_eq!(
        session.weak_annotation_state(page).expect("state"),
        WeakAnnotationState::Known { has_any_weak_annotations: true },
    );
}

#[test]
fn page_state_carries_the_current_revision_and_weak_knowledge() {
    let mut session = opened();
    let page = first_page(&mut session);

    let before = session.page_state(page).expect("state");
    assert_eq!(before.revision.generation, 0);
    assert_eq!(before.weak_annotation_state, WeakAnnotationState::Unknown);

    session.bump_revision(page).expect("bump");
    session.record_weak_flag(page, false).expect("record");

    let after = session.page_state(page).expect("state");
    assert_eq!(after.revision.generation, 1);
    // Known-false is a stronger claim than unknown, and the two must not
    // collapse.
    assert_eq!(
        after.weak_annotation_state,
        WeakAnnotationState::Known { has_any_weak_annotations: false },
    );
}

#[test]
fn a_token_does_not_validate_against_another_session() {
    let mut first = opened();
    let page = first_page(&mut first);
    let token = first.page_state(page).expect("state").revision;

    let second = opened();
    assert_eq!(
        second.validate_revision(&token).expect_err("foreign token").code,
        EngineErrorCode::InvalidReference,
    );
    // And it still validates against the session that minted it.
    first.validate_revision(&token).expect("own token");
    let _ = second.page_count();
}

#[test]
fn the_mutation_sequence_is_monotonic_and_starts_at_zero() {
    let session = opened();
    assert_eq!(session.mutation_sequence(), 0);
    session.note_mutation();
    session.note_mutation();
    assert_eq!(session.mutation_sequence(), 2);
}

#[test]
fn overlapping_acquires_share_one_page_and_release_together() {
    // The balanced acquire/release half of the gate.
    let mut session = opened();
    let page = first_page(&mut session);

    let first = session.acquire_page(page).expect("first");
    let second = session.acquire_page(page).expect("second");
    assert!(std::rc::Rc::ptr_eq(&first, &second), "the second acquire reloaded");
    assert_eq!(session.held_page_count(), 1);

    drop(first);
    assert!(session.is_page_held(page), "one holder left");
    drop(second);
    assert!(!session.is_page_held(page));
    assert_eq!(session.held_page_count(), 0);
}

#[test]
fn close_rejects_new_work_and_is_idempotent() {
    let mut session = opened();
    let page = first_page(&mut session);
    session.close();

    assert!(!session.is_open());
    assert_eq!(session.page_count().expect_err("closed").code, EngineErrorCode::DocNotOpen);
    assert_eq!(session.acquire_page(page).expect_err("closed").code, EngineErrorCode::DocNotOpen,);
    // Idempotent, per the lifecycle contract.
    session.close();
}

#[test]
fn close_does_not_invalidate_a_page_somebody_is_still_holding() {
    // The divergence from the TypeScript, which frees the pointer under its
    // holder. Here the page outlives the close, and the read still works.
    let mut session = opened();
    let page = first_page(&mut session);
    let held = session.acquire_page(page).expect("acquire");

    session.close();

    assert_eq!(session.held_page_count(), 0, "the session forgot the page");
    assert!(held.view().width() > 0.0, "the holder can still read it");
}

#[test]
fn retained_disposers_run_last_and_in_reverse() {
    // `pages.insert` needs this order: FPDF_ImportPagesByIndex leaves the
    // destination referencing the source, so the source may only be released
    // after the session document is closed.
    use std::cell::RefCell;
    use std::rc::Rc;

    let order = Rc::new(RefCell::new(Vec::new()));
    let mut session = opened();
    for label in ["first", "second", "third"] {
        let order = Rc::clone(&order);
        session.retain_until_close(move || order.borrow_mut().push(label));
    }

    session.close();
    assert_eq!(*order.borrow(), vec!["third", "second", "first"]);

    // And they run once, not on every close.
    session.close();
    assert_eq!(order.borrow().len(), 3);
}

#[test]
fn dropping_a_session_closes_it() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let closed = Rc::new(RefCell::new(false));
    {
        let mut session = opened();
        let closed = Rc::clone(&closed);
        session.retain_until_close(move || *closed.borrow_mut() = true);
    }
    assert!(*closed.borrow(), "a dropped session did not run its disposers");
}

#[test]
fn dropping_a_page_state_forgets_its_revision_and_weak_flag() {
    let mut session = opened();
    let page = first_page(&mut session);
    session.bump_revision(page).expect("bump");
    session.record_weak_flag(page, true).expect("record");

    session.drop_page_state(page).expect("drop");
    assert_eq!(
        session.page_state(page).expect("state").revision.generation,
        0,
        "the generation was not forgotten",
    );
    assert_eq!(session.weak_annotation_state(page).expect("state"), WeakAnnotationState::Unknown,);
}

// --- Permission enforcement, which on native has no other owner.

#[test]
fn a_session_opened_with_a_scope_enforces_it() {
    use epdf_engine::auth::scope::{DocCapability, IdentityClaims, PdfBits};
    use epdf_engine::session::authority::SessionAuthority;

    let mut session = DocumentSession::new();
    let authority = SessionAuthority::new(
        &["doc.open".to_string(), "doc.render".to_string()],
        IdentityClaims::default(),
        PdfBits::default(),
    )
    .expect("parse scope");
    session.open_with(report(), None, authority).expect("open");

    session.require(DocCapability::DocOpen).expect("granted");
    session.require(DocCapability::DocRender).expect("granted");
    assert_eq!(
        session.require(DocCapability::DocTextCopy).expect_err("denied").code,
        EngineErrorCode::Forbidden,
    );
}

#[test]
fn opening_reads_the_declared_permission_word_for_pdf_permissions() {
    use epdf_engine::auth::scope::{DocCapability, IdentityClaims, PdfBits};
    use epdf_engine::session::authority::SessionAuthority;

    // encrypted-r4-aes-128.pdf denies copying and allows the rest. The word
    // has to be read before the runtime owner elevation, which would otherwise
    // report every encrypted document as fully permitted.
    let mut session = DocumentSession::new();
    let authority = SessionAuthority::new(
        &["pdf.permissions".to_string()],
        IdentityClaims::default(),
        PdfBits::default(),
    )
    .expect("parse scope");
    session.open_with(bytes(ENCRYPTED), Some("user"), authority).expect("open");

    assert!(session.authority().allows(DocCapability::DocPrint), "printing is allowed");
    assert_eq!(
        session.require(DocCapability::DocTextCopy).expect_err("copying is denied").code,
        EngineErrorCode::Forbidden,
        "the elevation leaked into the permission word",
    );
}

#[test]
fn a_closed_session_grants_nothing() {
    use epdf_engine::auth::scope::DocCapability;

    // Even holding a reference to it. Close resets the authority rather than
    // leaving the last caller's grants in place.
    let mut session = opened();
    session.require(DocCapability::DocRender).expect("open sessions are unrestricted");

    session.close();
    for capability in DocCapability::ALL {
        assert_eq!(
            session.require(capability).expect_err("closed").code,
            EngineErrorCode::Forbidden,
            "{capability} survived close",
        );
    }
}

#[test]
fn opening_an_encrypted_document_elevates_pdfium_itself() {
    // PDFium's own subsystems consult the permission bits and would refuse
    // work this engine has already authorized; forms is the one that bites.
    // The fork's EPDF_SetRuntimeOwnerPermissions exists for that, and its
    // header says it "does not prove an owner password and does not authorize
    // the caller".
    //
    // Observable through the two permission getters: after the elevation
    // FPDF_GetDocPermissions reports the owner word, while
    // FPDF_GetDocUserPermissions still reports what the document declares.
    // Without this test the elevation could be deleted and every suite would
    // stay green until the first native forms write.
    let mut session = DocumentSession::new();
    session.open(bytes(ENCRYPTED), Some("user")).expect("open");
    let document = session.document().expect("open").document();

    assert!(document.is_encrypted());
    assert_eq!(
        document.permissions(),
        0xffff_fffc,
        "PDFium was not elevated, so its internals will refuse authorized work",
    );
    // The declared word is untouched, which is what the engine expands
    // pdf.permissions from. encrypted-r4-aes-128.pdf denies copying.
    assert_eq!(document.user_permissions(), 0xffff_ffec);
}

#[test]
fn the_elevation_makes_a_session_document_report_itself_owner_unlocked() {
    // A consequence worth knowing rather than a bug to fix, and it matches the
    // TypeScript, which elevates the same way and reads the same calls.
    //
    // EPDF_IsOwnerUnlocked reports true afterwards even though the user
    // password was the one supplied, so the security state read off a SESSION
    // document says "owner" for every encrypted document. Anything that needs
    // to know which password actually opened it has to probe the bytes
    // separately, before a session touches them; that is what
    // features::security::reader::probe_bytes is for, and why it opens its own
    // document instead of taking a session.
    let mut session = DocumentSession::new();
    session.open(bytes(ENCRYPTED), Some("user")).expect("open");
    let document = session.document().expect("open").document();
    assert!(document.is_owner_unlocked(), "the elevation did not reach EPDF_IsOwnerUnlocked");

    let probed = epdf_engine::features::security::reader::probe_bytes(
        pdfium::Library::acquire(),
        &bytes(ENCRYPTED),
        Some("user"),
        &epdf_engine::clock::SystemClock,
    );
    assert_eq!(
        probed.pdf_opened_as,
        Some(epdf_engine::features::security::state::OpenedAs::User),
        "a cold probe still tells the truth about which password was used",
    );
}

#[test]
fn an_unencrypted_document_is_not_elevated() {
    // Nothing to elevate, and calling it anyway would be a change to a handle
    // that had no restriction to lift.
    let session = opened();
    let document = session.document().expect("open").document();
    assert!(!document.is_encrypted());
}

// --- Layer sessions over a shared base.

#[test]
fn a_layer_session_reads_the_base_document() {
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    let registry = BaseDocumentRegistry::new();
    let base = registry.acquire_memory("report", &report(), None).expect("base");

    let mut session = DocumentSession::new();
    session.open_layer(base, SessionAuthority::unrestricted()).expect("open layer");

    assert_eq!(session.kind(), Some(OpenKind::Layer));
    assert_eq!(session.page_count().expect("open"), 14);
    assert_eq!(session.all_records().expect("records").len(), 14);
}

#[test]
fn two_sessions_share_one_parsed_base() {
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    // The reason the registry exists: the second viewer of a large document
    // costs a layer, not another copy of the bytes.
    let registry = BaseDocumentRegistry::new();
    let mut first = DocumentSession::new();
    let mut second = DocumentSession::new();

    first
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("first");
    second
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("second");

    assert_eq!(registry.ref_count("report"), 2);
    assert_eq!(registry.len(), 1, "one parse, two sessions");
    assert_eq!(first.page_count().expect("first"), 14);
    assert_eq!(second.page_count().expect("second"), 14);

    // And closing one leaves the other working.
    first.close();
    registry.release("report");
    assert_eq!(second.page_count().expect("second still open"), 14);
}

#[test]
fn closing_a_layer_session_releases_the_base_last() {
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    // The layer points into the base, so the base has to outlive it. If the
    // order were wrong this would be a use-after-free rather than a failed
    // assertion, so the check that matters is that it runs clean.
    let registry = BaseDocumentRegistry::new();
    let mut session = DocumentSession::new();
    session
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("open");

    registry.release("report");
    registry.release_all();
    assert!(!registry.is_registered("report"), "the registry let go of the base");

    // The session still holds it, so the document is readable.
    assert_eq!(session.page_count().expect("still open"), 14);
    session.close();
    assert!(!session.is_open());
}

#[test]
fn a_layer_session_cannot_be_opened_twice() {
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    let registry = BaseDocumentRegistry::new();
    let mut session = DocumentSession::new();
    session
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("open");
    let second = registry.acquire_memory("report", &report(), None).expect("base");
    assert_eq!(
        session
            .open_layer(second, SessionAuthority::unrestricted())
            .expect_err("already open")
            .code,
        EngineErrorCode::InvalidArg,
    );
}

#[test]
fn a_layer_opens_from_its_raw_delta_and_not_its_artifact_against_another_base() {
    use epdf_engine::cancel::CancelToken;
    use epdf_engine::features::save::{LayerArtifactKind, save_layer_to_buffer};
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;
    use epdf_engine::session::document::LayerOpenSource;

    // `LayerOpenSource`: a raw delta names no base, an artifact names its
    // own and is refused against any other.
    let registry = BaseDocumentRegistry::new();
    let mut session = DocumentSession::new();
    session
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("open");
    let cancel = CancelToken::never();
    // An empty layer has no delta to write.
    let patch: epdf_engine::dto::MetadataPatch =
        serde_json::from_value(serde_json::json!({ "title": "Layered" })).expect("a patch");
    epdf_engine::features::metadata_write::update_metadata(
        session.document().expect("open").document(),
        &patch,
        &cancel,
    )
    .expect("a write");
    let delta = save_layer_to_buffer(&session, LayerArtifactKind::Delta, &cancel).expect("delta");
    let artifact =
        save_layer_to_buffer(&session, LayerArtifactKind::Artifact, &cancel).expect("artifact");

    let mut from_delta = DocumentSession::new();
    from_delta
        .open_layer_from(
            registry.acquire_memory("report", &report(), None).expect("base"),
            &LayerOpenSource::RawDelta(delta),
            SessionAuthority::unrestricted(),
        )
        .expect("a delta opens");
    let read = epdf_engine::features::metadata::read_metadata(
        from_delta.document().expect("open").document(),
        &cancel,
    )
    .expect("read");
    assert_eq!(serde_json::to_value(&read).unwrap()["title"], "Layered");

    let other = bytes("tooling/engine-conformance/fixtures/annotation_kinds.pdf");
    let mut elsewhere = DocumentSession::new();
    let refused = elsewhere
        .open_layer_from(
            registry.acquire_memory("other", &other, None).expect("base"),
            &LayerOpenSource::Artifact(artifact),
            SessionAuthority::unrestricted(),
        )
        .expect_err("an artifact of another base");
    assert_eq!(refused.code, EngineErrorCode::MalformedPdf);
}

#[test]
fn a_layer_session_reserves_numbers_and_a_plain_one_has_none() {
    use epdf_engine::identity::ObjectNumberRange;
    use epdf_engine::session::authority::SessionAuthority;
    use epdf_engine::session::base_registry::BaseDocumentRegistry;

    // `DocumentSession.reserveObjectNumbers`: the layer's last object number
    // moves up by `count`, and the numbers passed are the session's.
    let registry = BaseDocumentRegistry::new();
    let mut session = DocumentSession::new();
    session
        .open_layer(
            registry.acquire_memory("report", &report(), None).expect("base"),
            SessionAuthority::unrestricted(),
        )
        .expect("open");
    let last = session.last_object_number().expect("open").expect("a layer has one");
    let range = session.reserve_object_numbers(4).expect("reserve");
    assert_eq!(range, ObjectNumberRange { first: last + 1, count: 4 });
    assert_eq!(session.last_object_number().unwrap(), Some(last + 4));
    assert!((last + 1..=last + 4).all(|n| session.holds_object_number(n)));
    assert!(!session.holds_object_number(last));

    // A raise never lowers, and outlives what it was for.
    session.raise_last_object_number(last).expect("raise");
    assert_eq!(session.last_object_number().unwrap(), Some(last + 4));

    let mut plain = opened();
    assert_eq!(plain.last_object_number().expect("open"), None);
    assert_eq!(
        plain.reserve_object_numbers(1).expect_err("no layer").code,
        EngineErrorCode::NotImplemented,
    );
    plain.raise_last_object_number(10).expect("nothing to raise");
}
