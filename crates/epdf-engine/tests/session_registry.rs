//! G3's control frames: close, layer.close, abort and shutdown.
//!
//! The frames are the only way a caller ends anything, so what they have to
//! get right is the order and the idempotence. Closing something already
//! closed is the state the caller asked for and never an error; shutdown
//! cancels before it closes and releases bases last, because a layer points
//! into its base.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::registry::{SessionKey, SessionRegistry};

const ENCRYPTED: &str = "tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf";

fn report() -> Vec<u8> {
    test_support::fixture(test_support::REPORT)
}

fn encrypted() -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(ENCRYPTED)).expect("read")
}

fn open(registry: &mut SessionRegistry, key: SessionKey) {
    registry.open_memory(key, report(), None, SessionAuthority::unrestricted()).expect("open");
}

#[test]
fn a_document_and_its_layers_are_separate_sessions() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("doc"));
    registry
        .open_layer_memory(
            SessionKey::layer("doc", "draft"),
            "doc-base",
            &report(),
            None,
            SessionAuthority::unrestricted(),
        )
        .expect("layer");

    assert_eq!(registry.len(), 2);
    assert!(registry.contains(&SessionKey::document("doc")));
    assert!(registry.contains(&SessionKey::layer("doc", "draft")));
}

#[test]
fn opening_the_same_key_twice_is_refused_rather_than_replacing() {
    // Replacing would close a document another caller is still reading.
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("doc"));
    let refused = registry
        .open_memory(SessionKey::document("doc"), report(), None, SessionAuthority::unrestricted())
        .expect_err("already open");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("document doc"), "{}", refused.message);
    assert_eq!(registry.len(), 1);
}

#[test]
fn layer_close_ends_one_session_and_leaves_the_rest() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("doc"));
    registry
        .open_layer_memory(
            SessionKey::layer("doc", "draft"),
            "doc-base",
            &report(),
            None,
            SessionAuthority::unrestricted(),
        )
        .expect("layer");

    assert!(registry.close_layer(&SessionKey::layer("doc", "draft")));
    assert_eq!(registry.len(), 1);
    assert!(registry.contains(&SessionKey::document("doc")));

    // Idempotent, and closing what is not open is not an error.
    assert!(!registry.close_layer(&SessionKey::layer("doc", "draft")));
    assert!(!registry.close_layer(&SessionKey::layer("other", "draft")));
}

#[test]
fn close_ends_every_session_for_one_document() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("a"));
    open(&mut registry, SessionKey::document("b"));
    for layer in ["draft", "review"] {
        registry
            .open_layer_memory(
                SessionKey::layer("a", layer),
                &format!("a-{layer}"),
                &report(),
                None,
                SessionAuthority::unrestricted(),
            )
            .expect("layer");
    }
    assert_eq!(registry.len(), 4);

    assert_eq!(registry.close_document("a"), 3, "the document and both layers");
    assert_eq!(registry.len(), 1);
    assert!(registry.contains(&SessionKey::document("b")), "another document is untouched");

    assert_eq!(registry.close_document("a"), 0, "idempotent");
}

#[test]
fn a_closed_session_reports_that_it_is_not_open() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("doc"));
    registry.close_document("doc");

    let error = registry.session(&SessionKey::document("doc")).expect_err("closed");
    assert_eq!(error.code, EngineErrorCode::DocNotOpen);
    assert!(error.message.contains("document doc"), "{}", error.message);
}

#[test]
fn a_document_needing_a_password_parks_and_keeps_its_bytes() {
    // The error still propagates: the caller has to know to prompt. What the
    // registry keeps is the session, so unlocking does not need the bytes
    // again.
    let mut registry = SessionRegistry::new();
    let key = SessionKey::document("locked");
    let error = registry
        .open_memory(key.clone(), encrypted(), None, SessionAuthority::unrestricted())
        .expect_err("needs a password");
    assert_eq!(error.code, EngineErrorCode::DocPasswordRequired);

    assert!(registry.contains(&key), "the session parked rather than vanishing");
    let refused = registry.session(&key).expect_err("locked");
    assert_eq!(refused.code, EngineErrorCode::DocPasswordRequired);
    assert!(refused.message.contains("locked"), "{}", refused.message);

    registry.unlock(&key, "user", SessionAuthority::unrestricted()).expect("unlock");
    assert_eq!(registry.session(&key).expect("open").page_count().expect("pages"), 1);
}

#[test]
fn a_wrong_password_on_an_unparked_document_leaves_nothing_behind() {
    // DocPasswordIncorrect is not a parkable state: the caller gave a password
    // and it was wrong, so there is nothing to hold for a retry that a fresh
    // open would not do better.
    let mut registry = SessionRegistry::new();
    let key = SessionKey::document("locked");
    let error = registry
        .open_memory(key.clone(), encrypted(), Some("wrong"), SessionAuthority::unrestricted())
        .expect_err("wrong password");
    assert_eq!(error.code, EngineErrorCode::DocPasswordIncorrect);
    assert!(!registry.contains(&key));
}

#[test]
fn abort_cancels_one_job_and_leaves_the_others() {
    let mut registry = SessionRegistry::new();
    let first = CancelToken::never();
    let second = CancelToken::never();
    registry.begin_job("job-1", first.clone());
    registry.begin_job("job-2", second.clone());
    assert_eq!(registry.running_jobs(), 2);

    assert!(registry.abort("job-1"));
    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
    assert_eq!(registry.running_jobs(), 1);

    // A job that already returned is a no-op: the two race by nature and the
    // caller cannot tell which won.
    assert!(!registry.abort("job-1"));
    assert!(!registry.abort("never-existed"));
}

#[test]
fn a_finished_job_cannot_be_aborted() {
    let mut registry = SessionRegistry::new();
    let cancel = CancelToken::never();
    registry.begin_job("job-1", cancel.clone());
    registry.end_job("job-1");

    assert!(!registry.abort("job-1"));
    assert!(!cancel.is_cancelled(), "a finished job was cancelled after the fact");
    assert_eq!(registry.running_jobs(), 0);
}

#[test]
fn shutdown_cancels_closes_and_releases_in_that_order() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("a"));
    registry
        .open_layer_memory(
            SessionKey::layer("a", "draft"),
            "a-base",
            &report(),
            None,
            SessionAuthority::unrestricted(),
        )
        .expect("layer");
    let cancel = CancelToken::never();
    registry.begin_job("job-1", cancel.clone());

    registry.shutdown();

    assert!(cancel.is_cancelled(), "an in-flight job survived shutdown");
    assert_eq!(registry.running_jobs(), 0);
    assert!(registry.is_empty());
    assert!(registry.bases().is_empty());
    assert!(registry.is_destroyed());
}

#[test]
fn shutdown_is_idempotent_and_refuses_new_work() {
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::document("a"));
    registry.shutdown();
    registry.shutdown();

    let refused = registry
        .open_memory(SessionKey::document("b"), report(), None, SessionAuthority::unrestricted())
        .expect_err("shut down");
    assert_eq!(refused.code, EngineErrorCode::RuntimeUnavailable);
    assert!(registry.is_empty());
}

#[test]
fn a_layer_refused_on_its_key_never_acquires_a_base() {
    // The key check runs before the acquisition, so this covers the cheap
    // half. The other half, where the base is acquired and then the session
    // fails to open, releases it in `open_layer_memory` and has no test: a
    // fresh layer over a valid base does not fail, and faking one would test
    // the fake.
    let mut registry = SessionRegistry::new();
    open(&mut registry, SessionKey::layer("doc", "draft"));
    let refused = registry
        .open_layer_memory(
            SessionKey::layer("doc", "draft"),
            "doc-base",
            &report(),
            None,
            SessionAuthority::unrestricted(),
        )
        .expect_err("key already in use");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert_eq!(registry.bases().ref_count("doc-base"), 0, "the base was not acquired at all");
}

#[test]
fn two_layers_over_one_base_share_its_parse() {
    let mut registry = SessionRegistry::new();
    for layer in ["draft", "review"] {
        registry
            .open_layer_memory(
                SessionKey::layer("doc", layer),
                "doc-base",
                &report(),
                None,
                SessionAuthority::unrestricted(),
            )
            .expect("layer");
    }
    assert_eq!(registry.bases().ref_count("doc-base"), 2);
    assert_eq!(registry.bases().len(), 1, "one parse, two layers");
    assert_eq!(
        registry
            .session(&SessionKey::layer("doc", "draft"))
            .expect("open")
            .page_count()
            .expect("pages"),
        14,
    );
}

// --- The two security frames.

#[test]
fn probing_a_file_needs_no_session() {
    use epdf_engine::clock::SystemClock;
    use epdf_engine::features::security::state::{EncryptionState, OpenedAs};

    // document.probeSecurityFile is the one frame with no session: it answers
    // what a file's security is before anybody opens it.
    let registry = SessionRegistry::new();
    let path = test_support::workspace_root().join(ENCRYPTED);

    let blocked = registry.probe_security_file(&path, None, &SystemClock).expect("probe");
    assert_eq!(blocked.encryption_state, EncryptionState::Encrypted);
    assert_eq!(blocked.encryption_requires_password, Some(true));

    let opened = registry.probe_security_file(&path, Some("user"), &SystemClock).expect("probe");
    assert_eq!(opened.pdf_opened_as, Some(OpenedAs::User));
    assert_eq!(opened.security_handler_revision, Some(4));
    assert!(registry.is_empty(), "probing opened no session");
}

#[test]
fn probing_a_file_and_probing_its_bytes_agree() {
    use epdf_engine::clock::{FixedClock, Timestamp};

    // The two differ in where the bytes come from and in nothing else, which
    // is worth pinning because they are separate code paths into PDFium.
    let clock = FixedClock(Timestamp::utc(1_700_000_000_000));
    let registry = SessionRegistry::new();
    let path = test_support::workspace_root().join(ENCRYPTED);

    let from_file = registry.probe_security_file(&path, Some("user"), &clock).expect("file");
    let from_bytes =
        registry.probe_security_bytes(&encrypted(), Some("user"), &clock).expect("bytes");
    assert_eq!(from_file, from_bytes);
}

#[test]
fn probing_a_missing_file_reports_unknown_rather_than_failing() {
    use epdf_engine::clock::SystemClock;
    use epdf_engine::features::security::state::EncryptionState;

    // The probe answers "what do we know about this file's security", and
    // nothing is an answer. An error here would make a caller handle the
    // absent-file case twice.
    let registry = SessionRegistry::new();
    let missing = test_support::workspace_root().join("no-such-document.pdf");
    let info = registry.probe_security_file(&missing, None, &SystemClock).expect("probe");
    assert_eq!(info.encryption_state, EncryptionState::Unknown);
    assert_eq!(info.pdf_permissions_bits, None);
}

#[test]
fn checking_a_password_against_an_open_session_does_not_change_it() {
    use epdf_engine::clock::SystemClock;
    use epdf_engine::features::security::reader::PasswordMode;
    use epdf_engine::features::security::state::OpenedAs;

    let mut registry = SessionRegistry::new();
    let key = SessionKey::document("doc");
    registry
        .open_memory(key.clone(), encrypted(), Some("user"), SessionAuthority::unrestricted())
        .expect("open");

    let as_owner = registry
        .check_password_permissions(&key, "owner", PasswordMode::Any, &SystemClock)
        .expect("owner password");
    assert_eq!(as_owner.pdf_opened_as, Some(OpenedAs::Owner));

    // And it still tells the two apart. Opening elevates PDFium with
    // EPDF_SetRuntimeOwnerPermissions, which makes EPDF_IsOwnerUnlocked report
    // true for this session; a check that read that flag instead of the
    // password would answer Owner here too.
    let as_user = registry
        .check_password_permissions(&key, "user", PasswordMode::Any, &SystemClock)
        .expect("user password");
    assert_eq!(as_user.pdf_opened_as, Some(OpenedAs::User));

    // Owner mode refuses the user password, which is the whole point of the
    // mode existing.
    assert_eq!(
        registry
            .check_password_permissions(&key, "user", PasswordMode::Owner, &SystemClock)
            .expect_err("user password in owner mode")
            .code,
        EngineErrorCode::DocPasswordIncorrect,
    );

    // The session is still the one that was opened with the user password.
    assert_eq!(registry.session(&key).expect("open").page_count().expect("pages"), 1);
}

#[test]
fn checking_a_password_on_a_closed_session_reports_not_open() {
    use epdf_engine::clock::SystemClock;
    use epdf_engine::features::security::reader::PasswordMode;

    let registry = SessionRegistry::new();
    let error = registry
        .check_password_permissions(
            &SessionKey::document("nope"),
            "user",
            PasswordMode::Any,
            &SystemClock,
        )
        .expect_err("no session");
    assert_eq!(error.code, EngineErrorCode::DocNotOpen);
}

#[test]
fn a_shut_down_registry_refuses_both_security_frames() {
    use epdf_engine::clock::SystemClock;
    use epdf_engine::features::security::reader::PasswordMode;

    let mut registry = SessionRegistry::new();
    registry.shutdown();

    let path = test_support::workspace_root().join(ENCRYPTED);
    assert_eq!(
        registry.probe_security_file(&path, None, &SystemClock).expect_err("shut down").code,
        EngineErrorCode::RuntimeUnavailable,
    );
    assert_eq!(
        registry
            .check_password_permissions(
                &SessionKey::document("doc"),
                "user",
                PasswordMode::Any,
                &SystemClock,
            )
            .expect_err("shut down")
            .code,
        EngineErrorCode::RuntimeUnavailable,
    );
}
