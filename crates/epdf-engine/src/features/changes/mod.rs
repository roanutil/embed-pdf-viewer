//! The change queue: one user action as one transaction.
//!
//! G11. Ports `services/src/features/changes/` (`ChangeApplier.ts`,
//! `ChangeLedger.ts`, `ChangeRecord.ts`, `internal/changeContext.ts`,
//! `internal/metadataChanges.ts`, `internal/attachmentChanges.ts`) and the
//! change types of `core/src/mutation/Change.ts` and `changeOutcome.ts`, for
//! upstream's 2445caba7, b69c40377, 119685ea4 and c2d7a5194. Operations:
//! `document.apply`, `document.applyChanges`, and every single verb that
//! upstream runs as a one-op change (here `metadata.update` and
//! `metadata.updateCustom`; the attachment ops have no single verb of their
//! own, c2d7a5194).
//!
//! A change is its ops in order, all of them or none: one refused op
//! refuses the whole change, and what the ops before it wrote is taken back
//! ([`transaction`]). Each op's places convert from page space right before
//! it runs, and its item back right after ([`applier::ChangeSpace`]). Each op
//! records the steps that reverse it ([`record`]), which is both what takes
//! a refused change back and what `{ undoOf }` runs later. The session keeps
//! every answer under its `opId` ([`ledger`]).
//!
//! The op table ([`op`]) runs the metadata and attachment ops; every other
//! op type answers `NotImplemented` naming the sub-task that registers it
//! ([`op::OP_OWNERS`]).
//!
//! # Events
//!
//! Upstream's worker publishes nothing for a change; main does.
//! `LocalDocumentChanges.apply` (`main/src/document/LocalDocumentChanges.ts`,
//! lines 89-94) publishes the change's events with `publishChange` once the
//! worker answers, all under its `opId`, and nothing for a retry. This host
//! stands in for both, so `document.apply` answers those events beside its
//! result as an `events` array (`{result, replayed, mutationSequence,
//! events}`), each stamped with `origin.tx` and, for an undo, `undoOf`. One
//! envelope `event` could not carry them: a change publishes one fact per
//! op. That is this host's own decision; Task 8.27, which closes the wire
//! contract, confirms or replaces it. `document.applyChanges` publishes
//! nothing, as upstream's doesn't: the server relays a request's changes
//! from its audit log.
//!
//! # Authority
//!
//! Upstream's `document.apply` and each change of `document.applyChanges`
//! require an `authority`. This host takes one when it is sent and
//! otherwise acts for the session's own ([`applier::ChangeAuthority::
//! of_session`]), as `JobEnvelope` reads an absent authority for every kind
//! until Task 8.27: the kernel's calls through `epdf-viewer-host`'s
//! `translate.rs` carry none (the kernel's metadata patch runs as a
//! `document.apply`), and neither do most of `tests/document_apply.rs`'s.

pub mod applier;
pub mod attachments;
pub mod fingerprint;
pub mod item;
pub mod ledger;
pub mod metadata;
pub mod op;
pub mod record;
pub mod transaction;
