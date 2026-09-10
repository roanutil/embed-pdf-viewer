//! G9. Page extraction into a standalone document.
//!
//! Replaces `services/src/features/pages/PagesExtractor.ts` (92). Operation:
//! `pages.extract`.
//!
//! Calls `FPDF_CreateNewDocument`, `FPDF_ImportPagesByIndex`,
//! `EPDF_SaveDocumentToOwnedBuffer` and `FPDF_CloseDocument` directly
//! (`PagesExtractor.ts:56-89`); it does not go through `DocumentSaver`. The
//! scratch document is a transient closed before return. The Rust wrapper for
//! `EPDF_SaveDocumentToOwnedBuffer` lands here and `session::save` reuses it.
