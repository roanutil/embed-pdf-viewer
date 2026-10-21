//! G13. Page assembly.
//!
//! Replaces `services/src/features/pages/{PagesMutator,PagesInserter,
//! PagesFlattener}.ts` and the page mutation DTOs. The page structure
//! writes, `pages.rotate`, `pages.reorder`, `pages.delete`,
//! `pages.insertBlank` and `pages.insert`, are ops of `document.apply`
//! (`super::changes::pages`, over `super::pages_write`); `pages.setName`,
//! `pages.removeName` and `pages.flatten` are kinds of their own.
//!
//! Gate: page identity survives reorder, and a `pages.insert` op keeps its
//! source retained on one owner, the session (`PagesInserter.ts:88`,
//! `session.retainUntilClose`).
