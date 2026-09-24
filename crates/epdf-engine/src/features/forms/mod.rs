//! G8 reads, G12 writes. Form fields and widgets.
//!
//! Reads replace `services/src/features/forms/{FormReader,internal/*}.ts` (188),
//! `core/src/forms/{field,snapshot,value,value-entry}.ts` (270), and the form
//! model cache `internal/{formModelCache,readFormSnapshot}.ts` (383) that G7
//! also reaches. Operations: `forms.list`, `forms.export`.
//!
//! Writes replace `{FormMutator,FormsEffectsApplier,internal/authorWidget}.ts`
//! (1,120) and `core/src/forms/{draft,patch,effects,submission,schema}.ts`
//! (527). Operations: `forms.setValue`, `forms.reset`, `forms.applyEffects`,
//! `forms.import`, `forms.repair`, `forms.createField`, `forms.updateField`,
//! `forms.deleteField`, `forms.attachWidget`, `forms.detachWidget`.
//!
//! Form JavaScript is a capability, not a port target: web keeps
//! `packages/core/js-sandbox` and mobile reports unsupported.
//!
//! # What is here
//!
//! [`snapshot`], the read: the detached field tree, its values and its
//! widgets. The export and the writes are not ported.

pub mod snapshot;
