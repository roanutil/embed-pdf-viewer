//! G6. Actions: what a PDF asks a viewer to do.
//!
//! Replaces `services/src/features/actions/{ActionModelReader,DocumentActionsReader}.ts`
//! and `core/src/dto/{PdfAction,PdfDestination}.ts`. Operation:
//! `actions.read`, plus the page-level trees `pages.list` carries and the
//! annotation-level ones G7 will.
//!
//! An action tree is detached: the reader materializes it out of PDFium and
//! nothing downstream holds a pointer back. That is what makes it safe to
//! cache, and it is why the reader has budgets rather than trusting the
//! document.
//!
//! # What is here
//!
//! [`submit_flags`], the Table 240 decode, which is pure and the piece where a
//! mistake is both silent and serious. The reader that walks PDFium's action
//! model is not ported yet.

pub mod destination;
pub mod document;
pub mod reader;
pub mod submit_flags;
pub mod vocabulary;
pub mod write;
