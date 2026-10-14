//! `packages/engine/core/src/signature/analysis/restrictions.ts`: the
//! restrictions a window is judged under, read from the sealed revision.

use crate::features::signature::protection::{APPROVAL_BASELINE, level_from_permission, min_level};
use crate::features::signature::types::{
    DocumentFieldLock, FieldLockSource, ModificationLevel, SignatureDto,
};

use super::types::{RestrictionAnchor, RestrictionSource, RevisionStructure};

/// What `restrictionsFor` answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Restrictions {
    pub anchors: Vec<RestrictionAnchor>,
    pub level: ModificationLevel,
    pub locks: Vec<DocumentFieldLock>,
}

/// `restrictionsFor`.
pub fn restrictions_for(before: &RevisionStructure, judged: Option<&SignatureDto>) -> Restrictions {
    let mut anchors = Vec::new();
    let mut locks = Vec::new();
    let mut enforced: Option<ModificationLevel> = None;
    let mut signed = false;
    for sig in &before.signatures {
        let Some(revision_index) = sig.revision_index.filter(|_| sig.signed) else { continue };
        signed = true;
        let own = judged.is_some_and(|j| sig.index == j.index);
        if sig.catalog_certification
            && let Some(doc_mdp) = sig.doc_mdp
        {
            anchors.push(RestrictionAnchor {
                signature_index: sig.index,
                revision_index,
                source: RestrictionSource::Docmdp,
                own,
                permission: Some(doc_mdp),
                fields: None,
            });
            enforced = Some(min_level(
                enforced.unwrap_or(ModificationLevel::Annotate),
                level_from_permission(doc_mdp),
            ));
        }
        if let Some(permission) = sig.lock.as_ref().and_then(|l| l.permission) {
            enforced = Some(min_level(
                enforced.unwrap_or(ModificationLevel::Annotate),
                level_from_permission(permission),
            ));
        }
        if judged.is_some() && !own {
            continue;
        }
        if let Some(spec) = &sig.field_mdp {
            anchors.push(RestrictionAnchor {
                signature_index: sig.index,
                revision_index,
                source: RestrictionSource::Fieldmdp,
                own,
                permission: None,
                fields: Some(spec.clone()),
            });
            locks.push(DocumentFieldLock {
                signature_index: sig.index,
                source: FieldLockSource::Fieldmdp,
                spec: spec.clone(),
            });
        }
        if let Some(spec) = &sig.lock {
            anchors.push(RestrictionAnchor {
                signature_index: sig.index,
                revision_index,
                source: RestrictionSource::Lock,
                own,
                permission: None,
                fields: Some(spec.clone()),
            });
            locks.push(DocumentFieldLock {
                signature_index: sig.index,
                source: FieldLockSource::Lock,
                spec: spec.clone(),
            });
        }
    }
    let level =
        if signed { enforced.unwrap_or(APPROVAL_BASELINE) } else { ModificationLevel::Annotate };
    Restrictions { anchors, level, locks }
}
