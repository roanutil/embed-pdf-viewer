//! G8 reads, G12 writes. Form fields and widgets.
//!
//! Reads replace `services/src/features/forms/{FormReader,internal/*}.ts` (188),
//! `core/src/forms/{field,snapshot,value,value-entry}.ts` (270), and the form
//! model cache `internal/{formModelCache,readFormSnapshot}.ts` (383) that G7
//! also reaches. Operations: `forms.list`, `forms.export`.
//!
//! Writes replace `{FormMutator,FormsEffectsApplier,internal/authorWidget}.ts`
//! (1,120) and `core/src/forms/{draft,patch,effects,submission,schema}.ts`
//! (527). Operations: `forms.setValue`, `forms.reset`, `forms.repair`,
//! `forms.import`, `forms.createField`, `forms.updateField`,
//! `forms.deleteField`, `forms.attachWidget`, `forms.detachWidget`.
//!
//! Form JavaScript is a capability, not a port target: web keeps
//! `packages/core/js-sandbox` and mobile reports unsupported.
//!
//! # What is here
//!
//! [`snapshot`], the read: the detached field tree, its values and its
//! widgets. [`widgets`], the form as `forms.list` answers it: the fields and
//! every widget's row. [`value`], what a value write is and what it leaves
//! a field holding; [`write`], the writes themselves, which the change ops
//! of [`crate::features::changes::forms`] run (`forms.applyEffects` went in
//! 98e5c270c: a script's effects are one `document.apply` change).

pub mod field;
pub mod snapshot;
pub mod value;
pub mod widgets;
pub mod write;
