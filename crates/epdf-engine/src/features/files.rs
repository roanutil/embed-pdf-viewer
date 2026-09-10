//! G8 reads, G14 writes. Attachments and piece info.
//!
//! Replaces `services/src/features/attachments/AttachmentReader.ts` (128) and
//! `AttachmentMutator.ts` (120) over the shared `internal/attachmentPrimitives.ts`
//! (274), `services/src/features/pieceinfo/PieceInfoAccessor.ts` with its object
//! decoders (~150), and `core/src/dto/{Attachment,PieceInfo}.ts` (121).
//!
//! Operations: `attachments.list`, `attachments.readFile`,
//! `annotations.readFile`, `pieceInfo.read`, `pieceInfo.applications`,
//! `attachments.create`, `attachments.delete`, `pieceInfo.update`,
//! `pieceInfo.clear`. Gate: `annotations.readFile` is byte-equal to the
//! TypeScript path.
