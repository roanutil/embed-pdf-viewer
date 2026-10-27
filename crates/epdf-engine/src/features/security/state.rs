//! Deriving security state from a probe.
//!
//! G3. A transcription of the pure half of
//! `packages/engine/core/src/engine/document-security-state.ts` and
//! `services/src/features/security/internal/buildSecurityInfo.ts`, plus
//! `services/src/shared/securityPermissions.ts`.
//!
//! Nothing here touches PDFium. The reader that does is [`super::reader`];
//! this module is the shape everything it learns gets folded into, and it is
//! pinned to the TypeScript by
//! `tooling/engine-conformance/vectors/security/state.json`.

use serde::{Deserialize, Serialize};

use crate::auth::scope::{DocCapability, PdfBits, materialize_pdf_permissions};

/// How a document was unlocked.
///
/// Mirrors `EPDF_PASSWORD_PERMISSION_*` in `public/fpdfview.h`:
/// invalid = 0, none = 1, user = 2, owner = 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenedAs {
    None,
    User,
    Owner,
}

impl OpenedAs {
    /// Anything that is not 2 or 3 is `None`, including the invalid code 0.
    ///
    /// Deliberately total rather than returning an error for 0. PDFium only
    /// fills the out-parameter when the call succeeded, and the caller has
    /// already checked that; treating an unexpected code as the least
    /// privileged answer is the safe reading either way.
    pub fn from_code(code: i32) -> Self {
        match code {
            3 => Self::Owner,
            2 => Self::User,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EncryptionState {
    Unknown,
    None,
    Encrypted,
    Unsupported,
}

/// Why a caller has to go somewhere else before it can read the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessReason {
    Password,
    Cdn,
    PermissionsUnknown,
}

/// What a probe learned. The wire's `DocumentSecurityProbeInfo`.
///
/// `permissions_bits` is `u32` where the wire carries a JSON number, because
/// PDFium's word is a 32-bit mask and every consumer masks it. The TypeScript
/// normalizes with `u32()` at each boundary for the same reason; doing it in
/// the type means it cannot be forgotten at one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityProbeInfo {
    pub encryption_state: EncryptionState,
    pub encryption_requires_password: Option<bool>,
    pub security_handler_revision: Option<i32>,
    pub pdf_permissions_bits: Option<u32>,
    pub pdf_permissions_all_allowed: Option<bool>,
    pub pdf_opened_as: Option<OpenedAs>,
    /// Milliseconds since the Unix epoch, as `Date.now()` produces.
    pub security_probed_at: Option<i64>,
}

/// PDF Standard security handlers reserve bits 1 and 2 as zero, and PDFium
/// masks even owner permissions through that convention. So "everything
/// allowed" for an encrypted document is `0xFFFF_FFFC` and not `0xFFFF_FFFF`,
/// and comparing against the latter reports every owner-unlocked document as
/// restricted.
pub const ALL_STANDARD_SECURITY_PERMISSIONS: u32 = 0xffff_fffc;

pub fn has_all_standard_security_permissions(bits: u32) -> bool {
    bits & ALL_STANDARD_SECURITY_PERMISSIONS == ALL_STANDARD_SECURITY_PERMISSIONS
}

/// Coerce PDFium's signed permission word to its unsigned 32-bit value.
///
/// The twin of `u32()` in `services/src/runtime/memory/bits.ts`, and the
/// reason every function below takes `u32`. JavaScript's bitwise operators
/// return a signed 32-bit int, so `0xFFFFFFFC & ~(1 << 3)` evaluates to `-12`
/// there, and the TypeScript launders the word through `u32()` at each
/// boundary. Rust's types make that a conversion the caller performs once, at
/// the FFI edge, instead of a defensive call at each use.
pub fn normalize_u32(bits: i64) -> u32 {
    bits as u32
}

/// The one constructor for the "successfully inspected" shape.
///
/// Every path that resolves a permission word goes through here, so the
/// encrypted-versus-none distinction, the revision-nulling rule and the
/// all-allowed derivation stay defined once. The password-blocked,
/// unsupported and unknown shapes are not built here: only a cold probe can
/// produce them and they carry no permission word.
pub fn build_security_info(
    opened_as: OpenedAs,
    permissions_bits: u32,
    security_handler_revision: Option<i32>,
    probed_at: i64,
) -> SecurityProbeInfo {
    let encrypted = opened_as != OpenedAs::None;
    SecurityProbeInfo {
        encryption_state: if encrypted {
            EncryptionState::Encrypted
        } else {
            EncryptionState::None
        },
        encryption_requires_password: Some(false),
        // An unencrypted document has no security handler, so the revision is
        // nulled rather than reported as whatever PDFium left in the word.
        security_handler_revision: if encrypted { security_handler_revision } else { None },
        pdf_permissions_bits: Some(permissions_bits),
        pdf_permissions_all_allowed: Some(has_all_standard_security_permissions(permissions_bits)),
        pdf_opened_as: Some(opened_as),
        security_probed_at: Some(probed_at),
    }
}

/// The probe shape for a document that needs a password.
pub fn password_blocked(probed_at: i64) -> SecurityProbeInfo {
    SecurityProbeInfo {
        encryption_state: EncryptionState::Encrypted,
        encryption_requires_password: Some(true),
        security_handler_revision: None,
        pdf_permissions_bits: None,
        pdf_permissions_all_allowed: None,
        pdf_opened_as: None,
        security_probed_at: Some(probed_at),
    }
}

/// The probe shape for a security handler PDFium will not open.
pub fn unsupported_security(probed_at: i64) -> SecurityProbeInfo {
    SecurityProbeInfo {
        encryption_state: EncryptionState::Unsupported,
        encryption_requires_password: None,
        security_handler_revision: None,
        pdf_permissions_bits: None,
        pdf_permissions_all_allowed: None,
        pdf_opened_as: None,
        security_probed_at: Some(probed_at),
    }
}

/// The probe shape for a document that failed to open for some other reason.
pub fn unknown_security(probed_at: i64) -> SecurityProbeInfo {
    SecurityProbeInfo {
        encryption_state: EncryptionState::Unknown,
        encryption_requires_password: None,
        security_handler_revision: None,
        pdf_permissions_bits: None,
        pdf_permissions_all_allowed: None,
        pdf_opened_as: None,
        security_probed_at: Some(probed_at),
    }
}

/// Capability-shaped booleans for a UI badge.
///
/// These must mirror the bit combinations
/// [`crate::auth::scope`] uses when expanding `pdf.permissions`, and the
/// conformance runner checks that they do rather than trusting this comment:
/// an advisory that says "you can print in high quality" where the resolver
/// denies `doc.print.high` is a UI that lies about what the document allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAdvisory {
    pub can_print: bool,
    pub can_print_high: bool,
    pub can_copy: bool,
    pub can_annotate: bool,
    pub can_fill_forms: bool,
    pub can_modify_forms: bool,
    pub can_modify_pages: bool,
    pub can_assemble: bool,
}

/// What `pdf.permissions` grants on a file with these bits
/// (`advisoryFromPdfBits`, `document-security-state.ts`): read off
/// [`materialize_pdf_permissions`], so the advisory and the resolver cannot
/// disagree.
pub fn advisory_from_pdf_bits(b: PdfBits) -> PermissionAdvisory {
    let granted = materialize_pdf_permissions(b);
    let has = |c: DocCapability| granted.contains(&c);
    PermissionAdvisory {
        can_print: has(DocCapability::DocPrint),
        can_print_high: has(DocCapability::DocPrintHigh),
        can_copy: has(DocCapability::DocTextCopy),
        can_annotate: has(DocCapability::DocAnnotateModify),
        can_fill_forms: has(DocCapability::DocFormsFill),
        can_modify_forms: has(DocCapability::DocFormsModify),
        can_modify_pages: has(DocCapability::DocPagesModify),
        can_assemble: has(DocCapability::DocPagesAssemble),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionState {
    pub known: bool,
    pub bits: Option<u32>,
    pub all_allowed: Option<bool>,
    pub opened_as: Option<OpenedAs>,
    pub security_handler_revision: Option<i32>,
    pub can_upgrade_to_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionSummary {
    pub state: EncryptionState,
    pub requires_password: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessSummary {
    pub required: bool,
    pub reasons: Vec<AccessReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// The wire's `DocumentSecurityState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityState {
    pub encryption: EncryptionSummary,
    pub permissions: PermissionState,
    pub access: AccessSummary,
}

/// What a caller asks for alongside the probe.
#[derive(Debug, Clone, Default)]
pub struct StateOptions {
    pub access_endpoint: Option<String>,
    pub cdn_required: bool,
}

/// Fold a probe into the state the wire carries.
pub fn security_state_from_probe(info: &SecurityProbeInfo, opts: &StateOptions) -> SecurityState {
    let known = info.pdf_permissions_bits.is_some();

    // The order of the reasons is the order TypeScript pushes them, and it is
    // part of the wire shape rather than an implementation detail: the
    // `/access` response carries the array as written.
    let mut reasons = Vec::new();
    // `password` only when there is no permission word. An encrypted document
    // that opened with the password already supplied requires nothing further.
    if info.encryption_requires_password == Some(true) && !known {
        reasons.push(AccessReason::Password);
    }
    if opts.cdn_required {
        reasons.push(AccessReason::Cdn);
    }
    if info.encryption_state == EncryptionState::Unknown {
        reasons.push(AccessReason::PermissionsUnknown);
    }

    let required = !reasons.is_empty();
    SecurityState {
        encryption: EncryptionSummary {
            state: info.encryption_state,
            requires_password: info.encryption_requires_password,
        },
        permissions: PermissionState {
            known,
            bits: info.pdf_permissions_bits,
            all_allowed: info.pdf_permissions_all_allowed,
            opened_as: info.pdf_opened_as,
            security_handler_revision: info.security_handler_revision,
            // Only an encrypted document opened as something other than owner
            // has an owner to upgrade to.
            can_upgrade_to_owner: info.encryption_state == EncryptionState::Encrypted
                && info.pdf_opened_as != Some(OpenedAs::Owner),
        },
        access: AccessSummary {
            required,
            reasons,
            endpoint: required.then(|| opts.access_endpoint.clone()).flatten(),
        },
    }
}

/// Who a password prompt asks for (`PasswordPrompt`'s `hint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptHint {
    User,
    Owner,
}

/// Whether to ask for a password (`PasswordPrompt`,
/// `packages/engine/core/src/engine/passwordPrompt.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum PasswordPrompt {
    None,
    /// Nothing can be read until a password is given. `hint` is `None`
    /// when the permissions are unknown; `incorrect` when the last
    /// password tried was wrong.
    Required {
        hint: Option<PromptHint>,
        incorrect: bool,
    },
    /// Open and readable; the owner password would allow more.
    Optional {
        hint: PromptHint,
    },
}

/// `passwordPromptFromState(state, incorrect)`: the first rule that
/// matches, top to bottom.
pub fn password_prompt_from_state(state: &SecurityState, incorrect: bool) -> PasswordPrompt {
    let permissions = &state.permissions;
    if state.encryption.state == EncryptionState::None {
        return PasswordPrompt::None;
    }
    if !permissions.known {
        return PasswordPrompt::Required { hint: None, incorrect };
    }
    if state.encryption.requires_password == Some(true)
        && permissions.opened_as == Some(OpenedAs::None)
    {
        return PasswordPrompt::Required { hint: Some(PromptHint::User), incorrect };
    }
    if permissions.opened_as == Some(OpenedAs::Owner) {
        return PasswordPrompt::None;
    }
    if permissions.can_upgrade_to_owner {
        return PasswordPrompt::Optional { hint: PromptHint::Owner };
    }
    PasswordPrompt::None
}

#[cfg(test)]
mod prompt_tests {
    //! `packages/engine/core/test/engine/passwordPrompt.test.ts`'s rules.
    use super::*;
    use serde_json::json;

    fn state(
        encryption: EncryptionState,
        requires_password: Option<bool>,
        known: bool,
        opened_as: Option<OpenedAs>,
        can_upgrade_to_owner: bool,
    ) -> SecurityState {
        SecurityState {
            encryption: EncryptionSummary { state: encryption, requires_password },
            permissions: PermissionState {
                known,
                bits: None,
                all_allowed: None,
                opened_as,
                security_handler_revision: None,
                can_upgrade_to_owner,
            },
            access: AccessSummary { required: false, reasons: Vec::new(), endpoint: None },
        }
    }

    fn prompt(state: &SecurityState, incorrect: bool) -> serde_json::Value {
        serde_json::to_value(password_prompt_from_state(state, incorrect)).unwrap()
    }

    #[test]
    fn each_rule_answers_the_typescripts_prompt() {
        use EncryptionState::{Encrypted, None as Plain};
        use OpenedAs::{None as Anonymous, Owner, User};
        assert_eq!(
            prompt(&state(Plain, Some(false), true, Some(Anonymous), false), true),
            json!({ "state": "none" })
        );
        assert_eq!(
            prompt(&state(Encrypted, Some(true), false, None, true), true),
            json!({ "state": "required", "hint": null, "incorrect": true })
        );
        assert_eq!(
            prompt(&state(Encrypted, Some(true), true, Some(Anonymous), true), false),
            json!({ "state": "required", "hint": "user", "incorrect": false })
        );
        assert_eq!(
            prompt(&state(Encrypted, Some(false), true, Some(Owner), false), false),
            json!({ "state": "none" })
        );
        assert_eq!(
            prompt(&state(Encrypted, Some(false), true, Some(User), true), false),
            json!({ "state": "optional", "hint": "owner" })
        );
        assert_eq!(
            prompt(&state(Encrypted, Some(false), true, Some(User), false), false),
            json!({ "state": "none" })
        );
    }
}
