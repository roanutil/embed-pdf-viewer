//! G13. Page assembly.
//!
//! Replaces `services/src/features/pages/{PagesMutator,PagesInserter,
//! PagesFlattener}.ts` (552) and the page mutation DTOs. Operations:
//! `pages.rotate`, `pages.move`, `pages.delete`, `pages.insertBlank`,
//! `pages.insert`, `pages.flatten`.
//!
//! Gate: page identity survives reorder, and `pages.insert` keeps its source
//! retained on one owner (`PagesInserter.ts:77`, `session.retainUntilClose`).
