//! The change queue: one user action as one transaction.
//!
//! G11. Ports `services/src/features/changes/` (`ChangeApplier.ts`,
//! `ChangeLedger.ts`, `ChangeRecord.ts`, `internal/changeContext.ts`,
//! `internal/metadataChanges.ts`) and the change types of
//! `core/src/mutation/Change.ts` and `changeOutcome.ts`, for upstream's
//! 2445caba7, b69c40377 and 119685ea4. Operations: `document.apply`,
//! `document.applyChanges`, and every single verb that upstream runs as a
//! one-op change (here `metadata.update` and `metadata.updateCustom`).
//!
//! A change is its ops in order, all of them or none: one refused op
//! refuses the whole change, and what the ops before it wrote is taken back
//! ([`transaction`]). Each op's places convert from page space right before
//! it runs, and its item back right after ([`applier::ChangeSpace`]). Each op
//! records the steps that reverse it ([`record`]), which is both what takes
//! a refused change back and what `{ undoOf }` runs later. The session keeps
//! every answer under its `opId` ([`ledger`]).
//!
//! The op table ([`op`]) runs the metadata ops; every other op type answers
//! `NotImplemented` naming the sub-task that registers it
//! ([`op::OP_OWNERS`]).

pub mod applier;
pub mod fingerprint;
pub mod item;
pub mod ledger;
pub mod metadata;
pub mod op;
pub mod record;
pub mod transaction;
