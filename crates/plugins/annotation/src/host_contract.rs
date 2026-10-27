//! `host-contract.ts` and `priorities.ts`: the host lens's extension seams
//! and constants. The host capability itself is
//! [`crate::AnnotationCapability`], the same object as the public one.

use std::rc::Rc;

use epdf_annotation_core::types::{Authority, Effect, Model, Painter};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::Value;

use crate::js::Object;
use crate::services::lending::LentRecords;

/// The interaction-hub priorities of the annotation plugin's handlers.
pub const ANNOTATION_EDIT_PRIORITY: f64 = 100.0;
pub const ANNOTATION_PLACE_PRIORITY: f64 = 95.0;
pub const ANNOTATION_DRAW_PRIORITY: f64 = 55.0;
pub const ANNOTATION_MARQUEE_PRIORITY: f64 = 50.0;

/// `previewBucket(devicePixelWidth, maxWidth)`: powers of two from 128 px
/// up to `max_width`.
pub fn preview_bucket(device_pixel_width: f64, max_width: f64) -> f64 {
    let px = epdf_geometry::js::max(128.0, device_pixel_width.ceil());
    epdf_geometry::js::min(max_width, 2f64.powf(px.log2().ceil()))
}

/// `AnnotationLender`: a sibling plugin that lends this plugin records it
/// owns (the form plugin's widgets), so they are selected, moved, resized,
/// restyled and deleted with the annotation tools. Records, refs, ops and
/// pictures are the engine's JSON.
pub trait AnnotationLender {
    /// Who owns the records (`'form'`).
    fn owner(&self) -> String;
    /// Its records as confirmed and as they show; `None` while it lends none.
    fn records(&self) -> Option<LentRecords>;
    /// The ops that write a message's effects on its records.
    fn ops_of(&self, effects: &[Effect], model: &Model, before: &Model) -> Vec<Value>;
    /// What undoing these ops looks like.
    fn undo_of(&self, ops: &[Value]) -> Vec<Value>;
    /// The ops that move these records of one page to `position`.
    fn reorder_ops(&self, page: &Value, refs: &[Value], position: &Value) -> Vec<Value>;
    /// Whether this session may change and delete the record.
    fn authority(&self, record: &Object) -> Authority;
    /// What its records' pictures on a page depend on.
    fn picture_epoch(&self, page: &Value) -> String;
    /// A record's confirmed picture version.
    fn picture_version(&self, reference: &Value) -> f64;
    /// The record's look isn't its confirmed look yet.
    fn pending(&self, reference: &Value) -> bool;
    /// How the lender draws the record live, or none.
    fn painter(&self, record: &Rc<Object>) -> Painter;
    /// Its records' pictures on a page, at rest, in every state.
    fn render_appearances(
        &self,
        page: &Value,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>>;
}

/// The lender's drawing members, as the view reads them.
pub struct LenderDrawing<'a>(pub &'a dyn AnnotationLender);

impl crate::read::view::LentDrawing for LenderDrawing<'_> {
    fn picture_version(&self, reference: &Value) -> f64 {
        self.0.picture_version(reference)
    }
    fn authority(&self, record: &Object) -> Authority {
        self.0.authority(record)
    }
    fn painter(&self, record: &Rc<Object>) -> Painter {
        self.0.painter(record)
    }
    fn pending(&self, reference: &Value) -> bool {
        self.0.pending(reference)
    }
}

/// `Behavior`: a sibling plugin (forms, links) or an interactive renderer
/// marks some annotations as interactive; while engaged they are not
/// geometry-editable.
pub trait Behavior {
    fn id(&self) -> String;
    /// Whether this behavior is about the annotation at all.
    fn matches(&self, annotation: &Object) -> bool;
    /// Whether it owns the annotation now.
    fn engaged(&self, annotation: &Object) -> bool;
}

/// `FilePickerProvider`: given a click's request, the file to place, or
/// `None` to cancel. The request and the file are JSON (`FilePromptRequest`,
/// `AttachmentFileSource`).
pub type FilePickerProvider = Rc<dyn Fn(Value) -> Promise<Option<Value>>>;

/// `StampPreviewProvider`: the stamp at a device pixel width, or none
/// (`ArmedStampPreview` as JSON: `{ bytes, mimeType? }`).
pub type StampPreviewProvider = Rc<dyn Fn(f64) -> Promise<Option<Value>>>;
