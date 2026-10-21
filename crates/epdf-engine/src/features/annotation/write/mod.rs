//! G11. Annotation writes.
//!
//! Replaces `services/src/features/annotations/AnnotationMutator.ts`,
//! `internal/write/*`, `internal/stackingOrder.ts` and the appearance,
//! normalize, patch and draft bases. Operations:
//! `annotations.create`, `annotations.update`, `annotations.delete`,
//! `annotations.reorder`.
//!
//! # What is here
//!
//! The PURE decision, the piece where a mistake is silent and expensive:
//! [`appearance`], which decides whether an update may keep the
//! `/AP` stream a foreign producer wrote. It is a transcription with
//! vectors and touches no PDFium. Upstream's weak-reference impact went
//! with weak references (bc0057f3a): no write renames an annotation.
//!
//! The mutators themselves are not ported.

pub mod appearance;
pub mod context;
pub mod create;
pub mod delete;
pub mod draft;
pub mod legacy;
pub mod mutator;
pub mod patch;
pub mod prepare;
pub mod primitives;
pub mod reorder;
pub mod resolve;
pub mod shape_for_rect;
pub mod stamp;
pub mod update;
