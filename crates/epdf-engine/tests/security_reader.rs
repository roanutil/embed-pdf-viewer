//! G3: reading security off real encrypted documents.
//!
//! The fixture half of G3's "open, wrong password, permission, close, cancel
//! traces" gate. The derivation half, which needs no document, is
//! `security_state.rs`.
//!
//! The documents come from `tooling/engine-conformance/fixtures/`, generated
//! by `fixtures/encrypt.mjs` at revisions 2, 3, 4 and 6. Every one uses user
//! password `user` and owner password `owner`, except the empty-user-password
//! one. Regenerate with:
//!   node tooling/engine-conformance/fixtures/generate.mjs
//!
//! One test here pins a known defect rather than correct behaviour. It is
//! named so, and `reader::LIVE_PERMISSIONS_DEFECT` says why.

use epdf_engine::clock::{Clock, FixedClock, Timestamp};
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::security::reader::{
    LIVE_PERMISSIONS_DEFECT, PasswordMode, check_password, probe_bytes, read_live,
};
use epdf_engine::features::security::state::{EncryptionState, OpenedAs};
use pdfium::{Document, Library};

/// Fixed so a probe's timestamp is an assertion rather than a moving target.
const NOW: i64 = 1_700_000_000_000;

fn clock() -> impl Clock {
    FixedClock(Timestamp::utc(NOW))
}

fn fixture(name: &str) -> Vec<u8> {
    let path =
        test_support::workspace_root().join("tooling/engine-conformance/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn open(name: &str, password: Option<&str>) -> Document {
    Document::from_bytes(Library::acquire(), &fixture(name), password)
        .unwrap_or_else(|e| panic!("open {name}: {e}"))
}

/// Every generated encrypted fixture, with the revision it declares.
const ENCRYPTED: &[(&str, i32)] = &[
    ("encrypted-r2-rc4-40.pdf", 2),
    ("encrypted-r3-rc4-128.pdf", 3),
    ("encrypted-r4-aes-128.pdf", 4),
    ("encrypted-r6-aes-256.pdf", 6),
];

#[test]
fn an_unencrypted_document_probes_as_unencrypted() {
    let info = probe_bytes(
        Library::acquire(),
        &test_support::fixture(test_support::REPORT),
        None,
        &clock(),
    );
    assert_eq!(info.encryption_state, EncryptionState::None);
    assert_eq!(info.pdf_opened_as, Some(OpenedAs::None));
    assert_eq!(info.encryption_requires_password, Some(false));
    // Nulled rather than passed through, whatever PDFium left in the word.
    assert_eq!(info.security_handler_revision, None);
    assert_eq!(info.pdf_permissions_all_allowed, Some(true));
    assert_eq!(info.security_probed_at, Some(NOW));
}

#[test]
fn probing_without_a_password_reports_that_one_is_needed() {
    for (name, _) in ENCRYPTED {
        let info = probe_bytes(Library::acquire(), &fixture(name), None, &clock());
        assert_eq!(info.encryption_state, EncryptionState::Encrypted, "{name}");
        assert_eq!(info.encryption_requires_password, Some(true), "{name}");
        // Nothing is known about the permissions, so nothing is claimed.
        assert_eq!(info.pdf_permissions_bits, None, "{name}");
        assert_eq!(info.pdf_opened_as, None, "{name}");
    }
}

#[test]
fn every_security_handler_revision_is_reported() {
    // Revisions 2 through 6 are four different key derivations, and a reader
    // that only ever saw one would not notice losing the others.
    for (name, revision) in ENCRYPTED {
        let info = probe_bytes(Library::acquire(), &fixture(name), Some("user"), &clock());
        assert_eq!(info.encryption_state, EncryptionState::Encrypted, "{name}");
        assert_eq!(info.security_handler_revision, Some(*revision), "{name}");
        assert_eq!(info.pdf_opened_as, Some(OpenedAs::User), "{name}");
    }
}

#[test]
fn a_user_password_and_an_owner_password_are_distinguished() {
    for (name, _) in ENCRYPTED {
        let as_user = probe_bytes(Library::acquire(), &fixture(name), Some("user"), &clock());
        let as_owner = probe_bytes(Library::acquire(), &fixture(name), Some("owner"), &clock());
        assert_eq!(as_user.pdf_opened_as, Some(OpenedAs::User), "{name}");
        assert_eq!(as_owner.pdf_opened_as, Some(OpenedAs::Owner), "{name}");

        // The cold probe reads the effective word, so an owner unlock is
        // all-allowed while the same document as a user is not.
        assert_eq!(as_owner.pdf_permissions_all_allowed, Some(true), "{name}");
        assert_eq!(as_user.pdf_permissions_all_allowed, Some(false), "{name}");
        assert_ne!(as_user.pdf_permissions_bits, as_owner.pdf_permissions_bits, "{name}");
    }
}

#[test]
fn a_wrong_password_is_refused_rather_than_ignored() {
    for (name, _) in ENCRYPTED {
        let info = probe_bytes(Library::acquire(), &fixture(name), Some("wrong"), &clock());
        assert_eq!(info.encryption_requires_password, Some(true), "{name}");
        assert_eq!(info.pdf_permissions_bits, None, "{name}");
    }
}

#[test]
fn an_unsupported_handler_is_not_a_password_prompt() {
    // The distinction that matters to a caller: a prompt can resolve one of
    // these and can never resolve the other.
    let info =
        probe_bytes(Library::acquire(), &fixture("unsupported-security.pdf"), None, &clock());
    assert_eq!(info.encryption_state, EncryptionState::Unsupported);
    assert_eq!(info.encryption_requires_password, None);
    assert_eq!(info.pdf_opened_as, None);
}

#[test]
fn an_empty_user_password_is_still_encrypted() {
    // The case most easily mistaken for an unencrypted document: it opens with
    // no password at all, and reporting it as unencrypted would tell a caller
    // the permission word is advisory when it is enforced.
    let info = probe_bytes(
        Library::acquire(),
        &fixture("encrypted-r4-empty-user-password.pdf"),
        None,
        &clock(),
    );
    assert_eq!(info.encryption_state, EncryptionState::Encrypted);
    assert_eq!(info.encryption_requires_password, Some(false));
    assert_eq!(info.pdf_opened_as, Some(OpenedAs::User));
    assert_eq!(info.security_handler_revision, Some(4));
}

#[test]
fn an_encrypted_document_can_still_allow_everything() {
    // So that all-allowed is exercised on the encrypted path and not only on
    // the unencrypted one, where it is true for a different reason.
    let info = probe_bytes(
        Library::acquire(),
        &fixture("encrypted-r4-all-allowed.pdf"),
        Some("user"),
        &clock(),
    );
    assert_eq!(info.pdf_opened_as, Some(OpenedAs::User));
    assert_eq!(info.pdf_permissions_all_allowed, Some(true));
    assert_eq!(info.pdf_permissions_bits, Some(0xffff_fffc));
}

#[test]
fn read_live_reports_declared_permissions_which_is_the_reproduced_defect() {
    // NOT a correctness test. It pins the wrong answer so that fixing it is a
    // deliberate act rather than a silent drift away from the TypeScript.
    //
    // `read_live` calls FPDF_GetDocUserPermissions, which the header says
    // "always returns user permissions, even if the document was unlocked by
    // the owner". `SecurityReader.readLiveSecurityInfo` describes that call as
    // the word in force after an owner unlock, which is what the OTHER getter
    // does. Reproduced for parity; see reader::LIVE_PERMISSIONS_DEFECT.
    assert!(LIVE_PERMISSIONS_DEFECT.contains("owner-unlocked"));

    for (name, _) in ENCRYPTED {
        let document = open(name, Some("owner"));
        let live = read_live(&document, &clock());
        let cold = probe_bytes(Library::acquire(), &fixture(name), Some("owner"), &clock());

        // Both agree the session is an owner one.
        assert_eq!(live.pdf_opened_as, Some(OpenedAs::Owner), "{name}");
        assert_eq!(cold.pdf_opened_as, Some(OpenedAs::Owner), "{name}");

        // And then disagree about what it may do. The cold probe reports the
        // elevated word; the live read reports the restricted declared one.
        assert_eq!(cold.pdf_permissions_all_allowed, Some(true), "{name}");
        assert_eq!(
            live.pdf_permissions_all_allowed,
            Some(false),
            "{name}: if this now reports true, someone fixed the defect; \
             update reader::LIVE_PERMISSIONS_DEFECT and the TypeScript together",
        );
        assert_ne!(live.pdf_permissions_bits, cold.pdf_permissions_bits, "{name}");
    }
}

#[test]
fn read_live_and_the_cold_probe_agree_when_there_is_no_owner_unlock() {
    // The defect is confined to owner unlocks. Everywhere else the two calls
    // return the same word, which is why it went unnoticed.
    for (name, _) in ENCRYPTED {
        let document = open(name, Some("user"));
        let live = read_live(&document, &clock());
        let cold = probe_bytes(Library::acquire(), &fixture(name), Some("user"), &clock());
        assert_eq!(live.pdf_permissions_bits, cold.pdf_permissions_bits, "{name}");
        assert_eq!(live.security_handler_revision, cold.security_handler_revision, "{name}");
    }
}

#[test]
fn checking_a_password_reports_what_it_would_unlock() {
    for (name, revision) in ENCRYPTED {
        let document = open(name, Some("user"));

        let as_user = check_password(&document, "user", PasswordMode::Any, &clock())
            .unwrap_or_else(|e| panic!("{name}: user password rejected: {e}"));
        assert_eq!(as_user.pdf_opened_as, Some(OpenedAs::User), "{name}");
        assert_eq!(as_user.security_handler_revision, Some(*revision), "{name}");

        let as_owner = check_password(&document, "owner", PasswordMode::Any, &clock())
            .unwrap_or_else(|e| panic!("{name}: owner password rejected: {e}"));
        assert_eq!(as_owner.pdf_opened_as, Some(OpenedAs::Owner), "{name}");
    }
}

#[test]
fn checking_a_password_does_not_change_the_session() {
    // The whole reason this exists next to the password argument on open: a
    // prompt asks "would this help?" without committing to it.
    let document = open("encrypted-r4-aes-128.pdf", Some("user"));
    let before = read_live(&document, &clock());
    check_password(&document, "owner", PasswordMode::Any, &clock()).expect("owner password");
    let after = read_live(&document, &clock());
    assert_eq!(before, after, "checking an owner password silently elevated the session");
}

#[test]
fn a_wrong_password_and_a_user_password_in_owner_mode_are_one_error() {
    // Telling a caller "that is the user password, not the owner password"
    // tells an attacker the same thing, so both are DocPasswordIncorrect.
    let document = open("encrypted-r4-aes-128.pdf", Some("user"));

    let wrong = check_password(&document, "nonsense", PasswordMode::Any, &clock())
        .expect_err("a wrong password");
    assert_eq!(wrong.code, EngineErrorCode::DocPasswordIncorrect);

    let user_in_owner_mode = check_password(&document, "user", PasswordMode::Owner, &clock())
        .expect_err("the user password in owner mode");
    assert_eq!(user_in_owner_mode.code, EngineErrorCode::DocPasswordIncorrect);

    check_password(&document, "owner", PasswordMode::Owner, &clock()).expect("the owner password");
}

#[test]
fn a_password_with_an_interior_nul_is_refused_rather_than_truncated() {
    // A truncating check would report a wrong password as correct for its
    // prefix, which is a password check that can be defeated by a byte.
    let document = open("encrypted-r4-aes-128.pdf", Some("user"));
    let refused = check_password(&document, "owner\0ignored", PasswordMode::Any, &clock())
        .expect_err("an interior NUL");
    assert_eq!(refused.code, EngineErrorCode::DocPasswordIncorrect);
}
