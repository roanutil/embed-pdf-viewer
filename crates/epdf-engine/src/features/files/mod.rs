//! G8 reads, G14 writes. Attachments and piece info.
//!
//! Replaces `services/src/features/attachments/AttachmentReader.ts` (128) and
//! `AttachmentMutator.ts` (120) over the shared `internal/attachmentPrimitives.ts`
//! (274), `services/src/features/pieceinfo/PieceInfoAccessor.ts` with its object
//! decoders (~150), and `core/src/dto/{Attachment,PieceInfo}.ts` (121).
//!
//! Operations: `attachments.list`, `attachments.readFile`,
//! `annotations.readFile`, `pieceInfo.read`, `pieceInfo.applications`,
//! `pieceInfo.update`, `pieceInfo.delete`, and the `attachments.create` and
//! `attachments.delete` change ops (`features::changes::attachments`). Gate:
//! `annotations.readFile` is byte-equal to the TypeScript path.
//!
//! # What is here
//!
//! [`attachments`]: the catalog's `/EmbeddedFiles` name tree, the bytes-out
//! call and the writes, over the same metadata projection G7's
//! file-attachment annotation already uses; and [`piece_info`], /PieceInfo
//! on both of the holders that can carry it.

pub mod attachments;
pub mod piece_info;
