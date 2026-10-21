//! G11. Annotation writes.
//!
//! Replaces `services/src/features/annotations/AnnotationMutator.ts`,
//! `internal/write/*`, `internal/mutations/computeMutationImpact.ts` and the
//! appearance, normalize, patch and draft bases. Operations:
//! `annotations.create`, `annotations.update`, `annotations.delete`,
//! `annotations.move`.
//!
//! # What is here
//!
//! The two PURE decisions, which are the pieces where a mistake is silent and
//! expensive: [`appearance`], which decides whether an update may keep the
//! `/AP` stream a foreign producer wrote, and [`impact`], which decides
//! whether a mutation just invalidated every index-addressed reference a
//! client is holding. Both are transcriptions with vectors; neither touches
//! PDFium.
//!
//! The mutators themselves are not ported.

pub mod appearance;
pub mod context;
pub mod create;
pub mod delete;
pub mod draft;
pub mod impact;
pub mod legacy;
pub mod move_;
pub mod mutator;
pub mod patch;
pub mod prepare;
pub mod primitives;
pub mod resolve;
pub mod shape_for_rect;
pub mod stamp;
pub mod update;
