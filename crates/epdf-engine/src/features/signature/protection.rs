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

/// `PROTECTABLE_CAPABILITIES`: every capability a protection can remove,
/// the most it ever takes away. A check of any other capability never needs
/// the document's protection.
pub const PROTECTABLE_CAPABILITIES: &[DocCapability] = &[
    DocCapability::DocDownloadFlattened,
    DocCapability::DocPagesModify,
    DocCapability::DocPagesAssemble,
    DocCapability::DocRedact,
    DocCapability::DocAttachmentsModify,
    DocCapability::DocOutlineModify,
    DocCapability::DocFormsModify,
    DocCapability::DocFormsScript,
    DocCapability::DocAnnotateModify,
    DocCapability::DocAnnotateImport,
    DocCapability::DocFormsFill,
    DocCapability::DocFormsImport,
];

/// `isProtectableCapability`: whether a protection can ever remove
/// `capability`.
pub fn is_protectable_capability(capability: DocCapability) -> bool {
    PROTECTABLE_CAPABILITIES.contains(&capability)
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
    out.insert(DocCapability::DocOutlineModify);
    out.insert(DocCapability::DocFormsModify);
    out.insert(DocCapability::DocFormsScript);
    if !level_allows(Some(enforced), ModificationLevel::Annotate) {
        out.insert(DocCapability::DocAnnotateModify);
        out.insert(DocCapability::DocAnnotateImport);
    }
    if !level_allows(Some(enforced), ModificationLevel::Fill) {
        out.insert(DocCapability::DocFormsFill);
        out.insert(DocCapability::DocFormsImport);
    }
    out
}

/// `describeProtection`: why `protection` refuses `capability`, the message
/// of a `ProtectedDocument` refusal.
pub fn describe_protection(capability: DocCapability, protection: &DocumentProtection) -> String {
    let cause = match &protection.certification {
        Some(certification) => format!(
            "certification signature {} (permission {})",
            certification.signature_index, certification.permission
        ),
        None => format!(
            "an existing signature (declared level '{}')",
            protection
                .enforced
                .and_then(|level| serde_json::to_value(level).ok())
                .and_then(|level| level.as_str().map(str::to_owned))
                .unwrap_or_else(|| "none declared".to_owned())
        ),
    };
    format!("the document is signed: {cause} forbids '{capability}'")
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/signature-protection.test.ts`, the cases
    //! the vector does not already pin.

    use super::*;
    use crate::features::forms::snapshot::FormFieldRef;
    use crate::features::signature::types::{SignatureCoverage, SignatureKind, SignatureSigner};

    fn certified(permission: u8) -> SignatureDto {
        SignatureDto {
            index: 0,
            field: FormFieldRef::ObjectNumber { field: 10 },
            field_name: "sig".into(),
            widget: None,
            signed: true,
            kind: SignatureKind::Signature,
            filter: None,
            sub_filter: None,
            byte_range: Some([0, 1, 2, 3]),
            contents_size: 8,
            coverage: Some(SignatureCoverage::WholeRevision),
            revision_index: Some(1),
            signer: SignatureSigner::default(),
            doc_mdp: Some(permission),
            catalog_certification: true,
            field_mdp: None,
            lock: None,
            seed_value: None,
        }
    }

    #[test]
    fn a_declaration_refuses_the_outline_scripts_and_imports_by_level() {
        let has = |p: u8, c: DocCapability| {
            protected_capabilities(Some(&derive_protection(&[certified(p)]))).contains(&c)
        };
        assert!(!has(3, DocCapability::DocAnnotateImport));
        assert!(has(3, DocCapability::DocOutlineModify));
        assert!(has(3, DocCapability::DocFormsScript));
        assert!(has(2, DocCapability::DocAnnotateImport));
        assert!(!has(2, DocCapability::DocFormsImport));
        assert!(has(1, DocCapability::DocFormsImport));
    }

    #[test]
    fn the_protectable_capabilities_are_exactly_what_the_strictest_protection_removes() {
        let strictest = derive_protection(&[certified(1)]);
        let removed: BTreeSet<DocCapability> = protected_capabilities(Some(&strictest));
        let protectable: BTreeSet<DocCapability> =
            PROTECTABLE_CAPABILITIES.iter().copied().collect();
        assert_eq!(removed, protectable);
        assert!(is_protectable_capability(DocCapability::DocFormsFill));
        assert!(!is_protectable_capability(DocCapability::DocFormsRead));
        assert!(!is_protectable_capability(DocCapability::DocSign));
    }

    #[test]
    fn a_refusal_names_the_certification_and_the_capability() {
        let p2 = derive_protection(&[certified(2)]);
        assert_eq!(
            describe_protection(DocCapability::DocAnnotateModify, &p2),
            "the document is signed: certification signature 0 (permission 2) forbids 'doc.annotate.modify'"
        );
    }
}
