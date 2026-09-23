//! Reading document security from PDFium.
//!
//! G3. Replaces `services/src/features/security/SecurityReader.ts`. The pure
//! derivation these functions feed is [`super::state`].
//!
//! Three entry points, as in TypeScript. [`probe_bytes`] is the doc-less one:
//! it opens, inspects and closes its own document to answer "what is this
//! file's security?" before any session exists. [`read_live`] and
//! [`check_password`] inspect a document that is already open.
//!
//! # The two permission calls, and a reproduced defect
//!
//! PDFium has two permission getters and they are easy to swap. From
//! `public/fpdfview.h`:
//!
//! - `FPDF_GetDocPermissions`: "If the document is not protected **or was
//!   unlocked by the owner**, 0xffffffff will be returned." The effective
//!   word. An owner unlock elevates it.
//! - `FPDF_GetDocUserPermissions`: "**Always returns user permissions, even if
//!   the document was unlocked by the owner.**" The declared word, whatever
//!   password opened the document.
//!
//! `SecurityReader.readLiveSecurityInfo` calls the second one and describes it
//! as "the permission word in force right now (e.g. after an owner unlock)",
//! which is what the first one does. So a live session unlocked with the owner
//! password reports the restricted user permissions, and the unlock has no
//! visible effect.
//!
//! [`read_live`] reproduces that, deliberately, and [`LIVE_PERMISSIONS_DEFECT`]
//! records it. This was a decision rather than an oversight: the conformance
//! corpus makes TypeScript the oracle, native is the first caller either
//! implementation will ever have, and having the two agree was judged worth
//! more than having one of them right. `tests/security_reader.rs` pins the
//! wrong answer, so changing it is a change somebody makes on purpose.
//!
//! The evidence, if you are the one deciding to fix it:
//! `tooling/engine-conformance/fixtures/encrypted-r2-rc4-40.pdf` declares
//! `/P` with printing, copying and high-quality printing denied. Opened with
//! the owner password `owner`, `FPDF_GetDocPermissions` reports `0xFFFFFFFC`
//! and `FPDF_GetDocUserPermissions` reports `0xFFFFF7E8`. The fix is one call
//! on each side plus a regenerated vector; the reason it is not done here is
//! that it changes shipping web behaviour for a path with no callers yet.

use pdfium::{Document, Library, PdfiumError};
use std::rc::Rc;

use super::state::{
    OpenedAs, SecurityProbeInfo, build_security_info, normalize_u32, password_blocked,
    unknown_security, unsupported_security,
};
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};

/// The one behaviour here that is knowingly wrong, carried for parity.
///
/// Kept as a constant rather than only a comment so that a reader grepping for
/// why native and web agree on something surprising finds the reason, and so
/// that deleting the note requires deleting a symbol a test names.
pub const LIVE_PERMISSIONS_DEFECT: &str = "\
read_live reports FPDF_GetDocUserPermissions, which always returns the declared \
user permissions, while describing it as the word in force. An owner-unlocked \
session therefore reports itself restricted. Reproduced from \
SecurityReader.readLiveSecurityInfo for parity; see the module docs.";

/// Which unlock a password check will accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordMode {
    /// Any password that opens the document.
    Any,
    /// Only the owner password.
    Owner,
}

/// Inspect a file's security without keeping it open.
///
/// The intentional exception to the engine's "operate on an open session"
/// shape: it answers what a file's security is before any session exists, so
/// it owns the document for the duration of the call and closes it.
///
/// Takes bytes where TypeScript's `probeFile` takes a path. The TypeScript
/// path exists because its runtime can hand PDFium a `FPDF_FILEACCESS` over a
/// Node file descriptor and avoid reading the whole file; this crate has no
/// such reader yet, and inventing one to preserve a signature would be the
/// wrong order of work. The answers are identical either way.
pub fn probe_bytes(
    library: Rc<Library>,
    bytes: &[u8],
    password: Option<&str>,
    clock: &dyn Clock,
) -> SecurityProbeInfo {
    let now = clock.now().epoch_millis;
    let document = match Document::from_bytes(library, bytes, password) {
        Ok(document) => document,
        // A password would resolve this one.
        Err(PdfiumError::Password) => return password_blocked(now),
        // And could never resolve this one, which is why the two are separate
        // variants rather than one load failure.
        Err(PdfiumError::Security) => return unsupported_security(now),
        Err(_) => return unknown_security(now),
    };

    describe(&document, now)
    // `document` drops here, closing the handle.
}

/// The cold-probe shape for a document that opened.
///
/// Shared by [`probe_bytes`] and [`probe_file`] so the two cannot drift: they
/// differ in where the bytes come from and in nothing else.
///
/// Reads the effective permission word. For a document opened with no password
/// or a user password that is also the declared word; opened with an owner
/// password it is the elevated one, which is the answer a caller asking "what
/// may I do with this file" wants.
fn describe(document: &Document, now: i64) -> SecurityProbeInfo {
    let encrypted = document.is_encrypted();
    let bits = normalize_u32(document.permissions() as i64);
    let opened_as = if !encrypted {
        OpenedAs::None
    } else if document.is_owner_unlocked() {
        OpenedAs::Owner
    } else {
        OpenedAs::User
    };
    build_security_info(
        opened_as,
        bits,
        encrypted.then(|| document.security_handler_revision()),
        now,
    )
}

/// Inspect a file's security without reading it into memory.
///
/// What `SecurityReader.probeFile` does, and now by the same means: PDFium
/// reads the ranges it needs through a [`pdfium::FileAccess`] rather than
/// being handed a copy. For a large document the difference is the whole file
/// against the few pages a security probe touches.
///
/// A file that cannot be opened at all reports the unknown shape rather than
/// an error, because that is what the shape is for: the probe answers "what do
/// we know about this file's security", and "nothing" is an answer.
pub fn probe_file(
    library: Rc<Library>,
    path: &std::path::Path,
    password: Option<&str>,
    clock: &dyn Clock,
) -> SecurityProbeInfo {
    let now = clock.now().epoch_millis;
    let Ok(access) = pdfium::FileAccess::from_file(path) else {
        return unknown_security(now);
    };
    match Document::from_file_access(library, &access, password) {
        Ok(document) => describe(&document, now),
        Err(PdfiumError::Password) => password_blocked(now),
        Err(PdfiumError::Security) => unsupported_security(now),
        Err(_) => unknown_security(now),
    }
}

/// Snapshot an already-open document's security.
///
/// Reports the DECLARED user permissions, not the effective ones. See
/// [`LIVE_PERMISSIONS_DEFECT`] and the module documentation: this is a
/// reproduction of the TypeScript's behaviour and not the intended meaning of
/// the function.
pub fn read_live(document: &Document, clock: &dyn Clock) -> SecurityProbeInfo {
    let encrypted = document.is_encrypted();
    // `user_permissions()` is `FPDF_GetDocUserPermissions`. The call that
    // would match this function's name is `document.permissions()`. Changing
    // it is a deliberate behaviour change on both platforms at once.
    let bits = normalize_u32(document.user_permissions() as i64);
    let opened_as = if !encrypted {
        OpenedAs::None
    } else if document.is_owner_unlocked() {
        OpenedAs::Owner
    } else {
        OpenedAs::User
    };
    build_security_info(
        opened_as,
        bits,
        encrypted.then(|| document.security_handler_revision()),
        clock.now().epoch_millis,
    )
}

/// Check a password against an open document and report what it unlocks.
///
/// Does not change the session: this answers "would this password help?",
/// which is what a prompt needs before it reopens anything.
///
/// Reports `DocPasswordIncorrect` when the password is rejected, and also when
/// [`PasswordMode::Owner`] was asked for and the password unlocks only as a
/// user. Those are one error on purpose, because telling a caller "that is the
/// user password, not the owner password" tells an attacker the same thing.
pub fn check_password(
    document: &Document,
    password: &str,
    mode: PasswordMode,
    clock: &dyn Clock,
) -> Result<SecurityProbeInfo, EngineError> {
    let rejected = || {
        EngineError::with_message(
            EngineErrorCode::DocPasswordIncorrect,
            "incorrect document password",
        )
    };

    let checked = document.check_password_permissions(password).ok_or_else(rejected)?;
    let opened_as = OpenedAs::from_code(checked.kind);
    if mode == PasswordMode::Owner && opened_as != OpenedAs::Owner {
        return Err(EngineError::with_message(
            EngineErrorCode::DocPasswordIncorrect,
            "owner password required",
        ));
    }
    Ok(build_security_info(
        opened_as,
        normalize_u32(checked.effective_permissions as i64),
        Some(checked.security_handler_revision),
        clock.now().epoch_millis,
    ))
}
