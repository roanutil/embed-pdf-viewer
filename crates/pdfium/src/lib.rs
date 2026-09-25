//! A safe, thread-confined wrapper over `pdfium-sys`.
//!
//! Group G0 (multiplatform-port-order.md). This layer owns handle lifetimes,
//! checked lengths and every `unsafe` boundary. It decides nothing about what
//! a PDF means; that is `epdf-engine`.
//!
//! Every handle type here is `!Send` by construction. PDFium's globals are
//! per-thread in the fork's native builds
//! (`build-target.sh` sets `embedpdf_thread_local_globals=true` for every
//! target except wasm32), and the engine gives each instance one owner OS
//! thread. `Rc` rather than `Arc` throughout is deliberate: it reinforces the
//! confinement rather than papering over it with atomics that would let a
//! handle cross threads and misbehave later.

pub(crate) mod action;
mod action_write;
mod annot;
mod annot_write;
mod base;
mod bitmap;
mod document;
mod error;
mod extract;
mod file_access;
mod fonts;
mod form;
mod form_write;
mod geom;
mod library;
mod metadata;
mod metadata_write;
mod page;
mod page_list;
mod page_write;
mod piece_info;
mod piece_info_write;
mod save;
mod stamp;
mod text;

pub use action::{
    ActionModel, ActionNodeId, AnnotationActionEvent, DestinationLocation, DestinationView,
    DocumentActionEvent, PageActionEvent, RawDestination,
};
pub use action_write::{Action, Destination, view};
pub use annot::{
    Annotation, AppearanceMode, Attachment, AttachmentExtractError, BorderStyle, DefaultAppearance,
    RectangleDifferences,
};
pub use base::{BaseDocument, LayerDocument, LayerOpenError};
pub use bitmap::Bitmap;
pub use document::{Document, FileBackedDocument, PasswordPermissions};
pub use error::PdfiumError;
pub use file_access::FileAccess;
pub use fonts::{
    FontDescription, FontId, add_fallback_font, clear_fallback_fonts, clear_registered_fonts,
    register_font_file, register_memory_font,
};
pub use form::{FieldActionEvent, FormDataFormat, FormField, FormModel};
pub use form_write::{
    FieldDisplay,
    ChangedWidgets, CreatableFieldFamily, DetachedWidgets, FormImportCounts, FormRepairReport,
};
pub use geom::{MatrixF, PointF, QuadF, RectF};
pub use library::Library;
pub use metadata::Trapped;
pub use page::{Page, PageView, SharedPage};
pub use page_list::PageBoxType;
pub use page_write::{FlattenOutcome, FlattenUsage};
pub use piece_info::{PieceInfoHolder, object_type};
pub use piece_info_write::PieceInfoValue;
pub use save::{LayerOutput, LayerSaveError, PdfSaveMode};
pub use stamp::{ImageObject, StampFit};
pub use text::{CharGeometry, CharGeometryFlags, TextObjectKey, TextPage};
