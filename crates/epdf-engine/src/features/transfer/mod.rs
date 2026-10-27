//! Annotations and form fields taken out of one document to go into this
//! one or another: bundles.
//!
//! Task 8.22. Ports `core/src/transfer/` (`bundle.ts`, `bundleLimits.ts`,
//! `codec.ts`, `AnnotationBundle.ts`, `FormBundle.ts`, `AnnotationTransfer.ts`,
//! `FormTransfer.ts`, `pageRefs.ts`, `exportSelection.ts`, `formExport.ts`,
//! `planAnnotationImport.ts`, `planFormImport.ts`, the facts of
//! `annotationImport.ts` and `formImport.ts`) and
//! `services/src/features/transfer/` (`bundlePages.ts`, `BundleResources.ts`,
//! `checkWireBundle.ts`), for upstream's 7f0b73a3c, 83f6598d2, 843deedb4 and
//! 73f05157f. Operations: `annotations.export`, `annotations.import`,
//! `forms.export`, `forms.import`, `forms.importValues`; the imports are
//! also change ops (`RecordedOp`), which only their own verbs and a server's
//! `document.applyChanges` run.
//!
//! A bundle is page space, whichever family it carries: each row is what a
//! read returns, measured from the top-left of its page, and an import
//! measures each create on the page it goes to. Rows stay the JSON objects
//! a read serializes, as upstream's stay objects: the checks here read
//! them the way upstream's do, field by field.
//!
//! A bundle past a limit is refused `PayloadTooLarge`, with upstream's
//! message and `details` ([`limits::past_limit`]).

pub mod annotation_bundle;
pub mod bundle;
pub mod codec;
pub mod export_selection;
pub mod form_bundle;
pub mod form_export;
pub mod limits;
pub mod page_refs;
pub mod plan_annotation;
pub mod plan_form;
pub mod resources;

pub use bundle::{Bundle, BundleImportPages, BundleImportTarget, BundleKind, BundlePage};
pub use limits::BundleLimits;
