//! G7 reads, G11 writes. Annotations.
//!
//! Reads replace `services/src/features/annotations/{AnnotationReader,
//! RawAnnotationReader,AnnotationAppearanceReader}.ts` (368), `internal/read/*`
//! (26 files, 1,922), the flag and enum decoders (~560), and
//! `core/src/annotation/{subtype,primitives,base,registry,
//! AnnotationListSnapshot}.ts` (557) with `kinds/*/dto.ts` (337).
//! Operations: `annotations.listRawAll`, `annotations.listRawPage`,
//! `annotations.listFullPage`, `annotations.renderAppearances`.
//!
//! Writes replace `AnnotationMutator.ts` (784), `internal/write/*` (22 files,
//! 3,496), `computeMutationImpact.ts` (141) and the appearance, normalize,
//! patch and draft bases (1,183). Operations: `annotations.create`,
//! `annotations.update`, `annotations.delete`, `annotations.move`.
//!
//! In Stage A, Rust neither creates nor retains the form model that
//! `internal/read/joinWidgetField.ts:23` needs: TypeScript lends the pointer.
//! The write gate (`runAnnotationMutationConformance`, 2,431 lines) carries a
//! review rule that no `?` past the apply boundary introduces a throw path.
