//! `packages/engine/core/src/signature/protection.ts`: what the signatures
//! already in a document mean for what comes after. Pure, and pinned to the
//! TypeScript by `tooling/engine-conformance/vectors/signature/protection.json`
//! (`tests/signature_protection.rs`). The reasoning lives in the TypeScript.

use std::collections::BTreeSet;

use crate::auth::scope::DocCapability;

use super::types::{
    Certification, DocumentFieldLock, DocumentProtection, FieldLockAction, FieldLockSource,
    FieldLockSpec, ModificationLevel, SignatureDto,
};

/// `SIGNATURE_POLICY_VERSION`.
pub const SIGNATURE_POLICY_VERSION: u32 = 4;

/// `APPROVAL_BASELINE`.
pub const APPROVAL_BASELINE: ModificationLevel = ModificationLevel::Annotate;

/// `levelFromPermission`.
pub fn level_from_permission(permission: u8) -> ModificationLevel {
    match permission {
        1 => ModificationLevel::Lta,
        2 => ModificationLevel::Fill,
        _ => ModificationLevel::Annotate,
    }
}

/// `minLevel`.
pub fn min_level(a: ModificationLevel, b: ModificationLevel) -> ModificationLevel {
    if a <= b { a } else { b }
}

/// `levelAllows`: `None` is unsigned, everything allowed.
pub fn level_allows(level: Option<ModificationLevel>, needed: ModificationLevel) -> bool {
    level.is_none_or(|level| level >= needed)
}

/// `lockNameCovers`: `group` covers `group.total`.
pub fn lock_name_covers(locked_name: &str, field_name: &str) -> bool {
    field_name == locked_name
        || field_name.strip_prefix(locked_name).is_some_and(|rest| rest.starts_with('.'))
}

/// `lockCovers`.
pub fn lock_covers(spec: &FieldLockSpec, field_name: &str) -> bool {
    match spec.action {
        FieldLockAction::All => true,
        FieldLockAction::Include => spec.fields.iter().any(|n| lock_name_covers(n, field_name)),
        FieldLockAction::Exclude => !spec.fields.iter().any(|n| lock_name_covers(n, field_name)),
    }
}

/// `deriveProtection`.
pub fn derive_protection(signatures: &[SignatureDto]) -> DocumentProtection {
    let mut signed = false;
    let mut enforced: Option<ModificationLevel> = None;
    let mut certification: Option<Certification> = None;
    let mut field_locks = Vec::new();

    for sig in signatures {
        if !sig.signed {
            continue;
        }
        signed = true;
        if sig.catalog_certification
            && let Some(doc_mdp) = sig.doc_mdp
            && certification.is_none()
        {
            certification = Some(Certification { signature_index: sig.index, permission: doc_mdp });
            enforced = Some(min_level(
                enforced.unwrap_or(ModificationLevel::Annotate),
                level_from_permission(doc_mdp),
            ));
        }
        if let Some(spec) = &sig.field_mdp {
            field_locks.push(DocumentFieldLock {
                signature_index: sig.index,
                source: FieldLockSource::Fieldmdp,
                spec: spec.clone(),
            });
        }
        if let Some(spec) = &sig.lock {
            field_locks.push(DocumentFieldLock {
                signature_index: sig.index,
                source: FieldLockSource::Lock,
                spec: spec.clone(),
            });
            if let Some(permission) = spec.permission {
                enforced = Some(min_level(
                    enforced.unwrap_or(ModificationLevel::Annotate),
                    level_from_permission(permission),
                ));
            }
        }
    }

    let judged = signed.then(|| enforced.unwrap_or(APPROVAL_BASELINE));
    DocumentProtection {
        enforced,
        judged,
        certification,
        field_locks,
        policy_version: SIGNATURE_POLICY_VERSION,
    }
}

/// `fieldLockFor`: the lock that freezes `field_name`, if any.
pub fn field_lock_for<'a>(
    protection: &'a DocumentProtection,
    field_name: &str,
) -> Option<&'a DocumentFieldLock> {
    protection.field_locks.iter().find(|lock| lock_covers(&lock.spec, field_name))
}

/// `protectedCapabilities`: what a protection removes from every caller.
pub fn protected_capabilities(protection: Option<&DocumentProtection>) -> BTreeSet<DocCapability> {
    let mut out = BTreeSet::new();
    let Some(protection) = protection else { return out };
    if protection.judged.is_none() {
        return out;
    }
    out.insert(DocCapability::DocDownloadFlattened);
    let Some(enforced) = protection.enforced else { return out };
    out.insert(DocCapability::DocPagesModify);
    out.insert(DocCapability::DocPagesAssemble);
    out.insert(DocCapability::DocRedact);
    out.insert(DocCapability::DocAttachmentsModify);
    out.insert(DocCapability::DocFormsModify);
    if !level_allows(Some(enforced), ModificationLevel::Annotate) {
        out.insert(DocCapability::DocAnnotateModify);
    }
    if !level_allows(Some(enforced), ModificationLevel::Fill) {
        out.insert(DocCapability::DocFormsFill);
    }
    out
}
