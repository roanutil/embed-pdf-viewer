//! What this session's caller is allowed to do.
//!
//! G3. The other half of [`crate::auth::scope`]: that module is the
//! vocabulary and the rules, this is the thing that holds one caller's answer
//! for the life of one document and refuses work that is outside it.
//!
//! `multiplatform-port-order.md` G3 puts it plainly: "Native permission
//! enforcement has no other owner." On web the resolver lives in
//! `engine/core` and its callers are route guards in `engine/main`; a native
//! host has no routes, so if the session does not enforce, nothing does.
//!
//! # Why the session must enforce
//!
//! `PdfDocumentOpener.setRuntimeOwnerPermissionsIfEncrypted` calls
//! `EPDF_SetRuntimeOwnerPermissions(doc, true)` on every encrypted document,
//! which makes PDFium's own subsystems behave as though the owner password was
//! supplied. That is deliberate: PDFium's forms code consults the permission
//! bits itself and would otherwise refuse work the application has already
//! authorized. The header says "Callers must enforce JWT/PDF permission policy
//! outside PDFium."
//!
//! So after open, PDFium is not enforcing anything. This is what is.

use crate::auth::scope::{
    Capabilities, CollabAction, CollabTarget, DocCapability, IdentityClaims, InvalidScope,
    ParsedScope, PdfBits, check_capability, check_collab, check_set_group, expanded_capabilities,
    parse_all,
};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::signature::protection::protected_capabilities;
use crate::features::signature::types::DocumentProtection;

/// One caller's authority over one open document.
#[derive(Debug, Clone)]
pub struct SessionAuthority {
    /// Parsed once at open rather than per check. A scope array is fixed for
    /// the life of a session, and re-parsing it on every read would put a
    /// string parse on the hot path for an answer that cannot change.
    scope: Vec<ParsedScope>,
    identity: IdentityClaims,
    /// The document's permission word, as the security probe read it.
    ///
    /// Only consulted when `pdf.permissions` is in scope. A document that has
    /// not been probed leaves this at its default, which is every bit false,
    /// so an unprobed document grants nothing through the virtual scope rather
    /// than everything.
    pdf_bits: PdfBits,
    /// What the document's signatures forbid (`deriveProtection`), which no
    /// scope grants, the wildcard included (`resolver.ts` `checkCapability`
    /// under `signedDocumentPolicy: 'protect'`, the default). `None` for an
    /// unsigned document or one not read yet.
    protection: Option<DocumentProtection>,
}

impl SessionAuthority {
    /// Parse a scope array and hold it.
    ///
    /// Reports [`InvalidScope`] rather than dropping the entry, because a
    /// scope string nobody recognizes is a configuration mistake and silently
    /// ignoring it grants less than the caller asked for without saying so.
    pub fn new(
        scope: &[String],
        identity: IdentityClaims,
        pdf_bits: PdfBits,
    ) -> Result<Self, InvalidScope> {
        Ok(Self { scope: parse_all(scope)?, identity, pdf_bits, protection: None })
    }

    /// Full authority, for a host with no notion of scopes.
    ///
    /// The native equivalent of opening with `['*']`, which is what the
    /// conformance harness and the smoke apps do. Named rather than spelled
    /// `["*"]` at each call site so that grepping for it finds every place
    /// that opted out of enforcement.
    pub fn unrestricted() -> Self {
        Self {
            scope: vec![ParsedScope::Wildcard],
            identity: IdentityClaims::default(),
            pdf_bits: PdfBits::default(),
            protection: None,
        }
    }

    pub fn identity(&self) -> &IdentityClaims {
        &self.identity
    }

    pub fn pdf_bits(&self) -> PdfBits {
        self.pdf_bits
    }

    /// Replace the permission word once the document has been probed.
    ///
    /// Open reads the bits and open is where this is called. Kept separate
    /// from [`SessionAuthority::new`] because the scope arrives with the
    /// request and the bits arrive with the document, and pretending they
    /// arrive together would force a caller to probe before it can build an
    /// authority.
    pub fn set_pdf_bits(&mut self, pdf_bits: PdfBits) {
        self.pdf_bits = pdf_bits;
    }

    /// Adopt the document's signature protection (`ScopeGuard.setProtection`).
    pub fn set_protection(&mut self, protection: Option<DocumentProtection>) {
        self.protection = protection;
    }

    pub fn protection(&self) -> Option<&DocumentProtection> {
        self.protection.as_ref()
    }

    /// What the protection removes from every caller.
    fn protected(&self) -> Capabilities {
        protected_capabilities(self.protection.as_ref())
    }

    pub fn scope(&self) -> &[ParsedScope] {
        &self.scope
    }

    pub fn allows(&self, capability: DocCapability) -> bool {
        !self.protected().contains(&capability)
            && check_capability(capability, &self.scope, self.pdf_bits)
    }

    pub fn allows_any(&self, capabilities: &[DocCapability]) -> bool {
        capabilities.iter().any(|c| self.allows(*c))
    }

    /// `ScopeGuard.canCollab`: the collab action against a record's owner.
    pub fn allows_collab(&self, action: CollabAction, target: &CollabTarget) -> bool {
        // A declared signature constraint outranks the collab authority.
        !self.protected().contains(&DocCapability::DocAnnotateModify)
            && check_collab(action, target, &self.scope, &self.identity, self.pdf_bits)
    }

    /// `ScopeGuard.canSetGroup`: assigning `group_id` to an annotation.
    pub fn allows_set_group(&self, group_id: &str) -> bool {
        check_set_group(group_id, self.identity.group_id.as_deref(), &self.scope)
    }

    /// Everything this caller may do, for an `/access`-style report.
    pub fn expanded(&self) -> Capabilities {
        let protected = self.protected();
        expanded_capabilities(&self.scope, self.pdf_bits)
            .into_iter()
            .filter(|c| !protected.contains(c))
            .collect()
    }

    /// Refuse unless the capability is granted.
    ///
    /// The error names the capability and nothing else. A message that also
    /// said what the caller does hold would be a scope oracle, and the caller
    /// already knows its own token.
    pub fn require(&self, capability: DocCapability) -> Result<(), EngineError> {
        if self.allows(capability) {
            return Ok(());
        }
        Err(EngineError::with_message(
            EngineErrorCode::Forbidden,
            format!("permission denied: {capability}"),
        ))
    }

    /// Refuse unless at least one of the capabilities is granted.
    ///
    /// For routes that accept either of two, such as a geometry read under
    /// `doc.text.select` or `doc.text.search`.
    pub fn require_any(&self, capabilities: &[DocCapability]) -> Result<(), EngineError> {
        if self.allows_any(capabilities) {
            return Ok(());
        }
        let names: Vec<&str> = capabilities.iter().map(|c| c.as_str()).collect();
        Err(EngineError::with_message(
            EngineErrorCode::Forbidden,
            format!("permission denied: one of {}", names.join(", ")),
        ))
    }
}

impl Default for SessionAuthority {
    /// Grants nothing.
    ///
    /// Deliberately the opposite of [`SessionAuthority::unrestricted`]: a
    /// default that appears by accident, through a `..Default::default()` or a
    /// derived container, must fail closed.
    fn default() -> Self {
        Self {
            scope: Vec::new(),
            identity: IdentityClaims::default(),
            pdf_bits: PdfBits::default(),
            protection: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_default_grants_nothing() {
        // The failure mode this exists to prevent: an authority that appeared
        // without anyone choosing it must not be permissive.
        let authority = SessionAuthority::default();
        for capability in DocCapability::ALL {
            assert!(!authority.allows(capability), "{capability} was granted by default");
        }
    }

    #[test]
    fn unrestricted_grants_everything() {
        let authority = SessionAuthority::unrestricted();
        for capability in DocCapability::ALL {
            authority.require(capability).expect("wildcard");
        }
    }

    #[test]
    fn a_denied_capability_reports_forbidden_and_names_itself() {
        let authority = SessionAuthority::new(
            &scope(&["doc.open"]),
            IdentityClaims::default(),
            PdfBits::default(),
        )
        .expect("parse");
        authority.require(DocCapability::DocOpen).expect("granted");

        let refused = authority.require(DocCapability::DocRender).expect_err("denied");
        assert_eq!(refused.code, EngineErrorCode::Forbidden);
        assert!(refused.message.contains("doc.render"), "{}", refused.message);
        // And says nothing about what the caller does hold.
        assert!(!refused.message.contains("doc.open"), "{}", refused.message);
    }

    #[test]
    fn an_unknown_scope_string_is_refused_rather_than_ignored() {
        // Dropping it would grant less than the caller asked for without
        // saying so, which reads as an engine bug rather than a bad token.
        let error = SessionAuthority::new(
            &scope(&["doc.open", "doc.not-a-real-capability"]),
            IdentityClaims::default(),
            PdfBits::default(),
        )
        .expect_err("unknown scope");
        assert_eq!(error.scope, "doc.not-a-real-capability");
    }

    #[test]
    fn pdf_permissions_expands_only_once_the_bits_are_known() {
        // An unprobed document grants nothing through the virtual scope. That
        // is the restrictive reading, and the one that matters: the permissive
        // one would hand out print and copy for a document nobody has looked
        // at.
        let mut authority = SessionAuthority::new(
            &scope(&["pdf.permissions"]),
            IdentityClaims::default(),
            PdfBits::default(),
        )
        .expect("parse");
        assert!(!authority.allows(DocCapability::DocPrint));

        authority.set_pdf_bits(PdfBits { bit3: true, ..PdfBits::default() });
        assert!(authority.allows(DocCapability::DocPrint));
        // Bit 12 refines bit 3 rather than standing alone.
        assert!(!authority.allows(DocCapability::DocPrintHigh));
    }

    #[test]
    fn require_any_accepts_either() {
        let authority = SessionAuthority::new(
            &scope(&["doc.text.search"]),
            IdentityClaims::default(),
            PdfBits::default(),
        )
        .expect("parse");

        authority
            .require_any(&[DocCapability::DocTextSelect, DocCapability::DocTextSearch])
            .expect("one of the two");
        let refused = authority
            .require_any(&[DocCapability::DocPrint, DocCapability::DocDownload])
            .expect_err("neither");
        assert_eq!(refused.code, EngineErrorCode::Forbidden);
    }
}
