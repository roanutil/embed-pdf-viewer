//! G3, G10. Session core: the owner of a document's bytes and lifetime.
//!
//! Replaces `services/src/document-session/DocumentSession.ts` (354),
//! `pages/{PagePtrPool,PageRecord}.ts` (90),
//! `revisions/RevisionAuthority.ts` (103),
//! `lifecycle/{PdfDocumentOpener,BaseDocumentRegistry}.ts` (285).
//!
//! Owns bytes, lifetime, page identity and leases, revisions, mutation
//! sequence, password state and retained import sources. Close is idempotent:
//! it rejects new work, invalidates handles, and closing an engine closes its
//! documents. Handles carry generation IDs so an engine can be replaced; there
//! is no process-wide singleton with no reset.
//!
//! Operations: `open.fatMem`, `open.layerMemBase`, `open.layerFileBase`,
//! `document.checkPasswordPermissions`, `document.probeSecurityFile`, and the
//! control frames `close`, `layer.close`, `abort`, `shutdown`.
//!
//! Native hosts own the session from G3. Web reaches it at G3w (opt-in) and
//! takes authority at G10.

pub mod pages;
pub mod revisions;
pub mod save;
pub mod scope;
pub mod security;
