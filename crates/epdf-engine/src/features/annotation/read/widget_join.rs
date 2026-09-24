//! Stamping a widget with the field that owns it.
//!
//! G8, reached from G7. Ports
//! `services/src/features/annotations/internal/read/joinWidgetField.ts`.
//!
//! A widget annotation's `/Parent` points at a FIELD dictionary, not at
//! another annotation, so no annotation-plane accessor can follow it. The
//! reconciled form model can, and it is also the only thing that knows about
//! a field recovered from a page's `/Annots` when the `/AcroForm` tree failed
//! to list it.
//!
//! The model is built ONCE per listing and every widget is stamped from it.
//! Building one per widget would walk the whole field tree and sweep every
//! page's `/Annots` per widget, which on a form-heavy document is the
//! difference between one pass and hundreds.
//!
//! A widget with no durable address is skipped. The join key is the widget's
//! object number, and a widget that has none cannot be looked up: it reports
//! the unattached values, which is what it would report anyway.

use pdfium::Document;

use super::dto::{AnnotationDto, AnnotationKind};
use crate::features::forms::snapshot::FormFieldFamily;
use crate::identity::AnnotationRef;

/// Fill in `field_object_number` and `field_family` on every widget.
///
/// Does nothing, cheaply, for a listing with no widgets in it: the model is
/// not built at all. Most pages have none.
pub fn join_widget_fields(document: &Document, annotations: &mut [AnnotationDto]) {
    if !annotations.iter().any(|a| matches!(a.kind, AnnotationKind::Widget { .. })) {
        return;
    }
    // A document PDFium will not build a model for leaves every widget
    // unattached rather than failing the listing. The widgets are still
    // annotations with geometry and appearance, and a form tree that cannot
    // be walked is not a reason to report nothing about the page.
    let Some(model) = document.form_model() else {
        return;
    };

    for annotation in annotations {
        let AnnotationKind::Widget { fields } = &mut annotation.kind else {
            continue;
        };
        let AnnotationRef::ObjectNumber { annot, .. } = &annotation.base.annotation_ref else {
            continue;
        };
        let Some(field_index) = model.field_index_for_widget(*annot) else {
            continue;
        };
        let Some(field) = model.field(field_index) else {
            continue;
        };
        fields.field_object_number = field.object_number();
        fields.field_family = FormFieldFamily::from_code(field.family_code());
    }
}
