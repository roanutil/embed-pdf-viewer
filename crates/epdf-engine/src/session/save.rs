//! G10. Save and mutation finalization (Stage B).
//!
//! Replaces `DocumentSaver.ts` (137), `internal/pdfSaveMode.ts` (18),
//! `core/src/events/DocumentEvent.ts` (179), `core/src/dto/PdfSaveMode.ts`, and
//! `WorkerHost.finishMutation` / `saveLayerArtifact` (`WorkerHost.ts:1380-1414`).
//!
//! Operations: `document.saveBuffer`, `document.saveFile`,
//! `document.saveLayerBuffer`. Rust returns the events each mutation publishes;
//! `services/src/events/EventHub.ts` stays TypeScript and delivers them.
//!
//! `saveStandalone*` reuses the `EPDF_SaveDocumentToOwnedBuffer` wrapper that
//! lands with `features::extract` in G9. Byte equality of saved output is not
//! asserted anywhere; the gates are semantic readback.
