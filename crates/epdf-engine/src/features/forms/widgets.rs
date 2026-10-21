//! The form as `forms.list` answers it: the fields, and every widget row.
//!
//! G8. Ports `services/src/features/forms/internal/widgetRows.ts`
//! (`readForm`, `readFormWidgets`) and the widget half of
//! `annotations/internal/read/collectPageAnnotations.ts` (its `family:
//! 'widgets'` filter).
//!
//! A form field's widgets come with the form, never with the annotations
//! (1233d1349): the field tree says which widgets a field has, and each
//! page's `/Annots` says where a widget is and how it looks, as it does for
//! every reader.

use pdfium::{Document, FormModel};

use super::snapshot::{FormSnapshot, read_form_fields};
use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::annotation::read::record::{
    WidgetAnnotation, join_widget_field, read_annotation_record_base, read_widget_annotation,
};
use crate::features::annotation::subtype::code;
use crate::identity::PageObjectNumber;

/// The whole form (`readForm`), in the file's coordinates: the fields from
/// `model`, and every widget row from the pages.
pub fn read_form(
    document: &Document,
    model: &FormModel<'_>,
    cancel: &CancelToken,
) -> Result<FormSnapshot, EngineError> {
    let fields = read_form_fields(model, cancel)?;
    let widgets = read_form_widgets(document, model, cancel)?;
    Ok(FormSnapshot {
        form_kind: fields.form_kind,
        needs_appearances: fields.needs_appearances,
        fields: fields.fields,
        widgets,
        calculation_order: fields.calculation_order,
    })
}

/// Every widget the document's pages show, in page order and `/Annots` order
/// within a page (`readFormWidgets`). Only widget rows are built: another
/// annotation's subtype is read, never its row. A page stored inline has no
/// object number, so nothing on it can be named: it is passed over, and the
/// rest of the form still reads.
pub fn read_form_widgets(
    document: &Document,
    model: &FormModel<'_>,
    cancel: &CancelToken,
) -> Result<Vec<WidgetAnnotation>, EngineError> {
    let mut rows = Vec::new();
    for page_index in 0..document.page_count() {
        cancel.check()?;
        let Ok(page) = PageObjectNumber::new(document.page_object_number_by_index(page_index))
        else {
            continue;
        };
        let count = document.raw_annotation_count(page_index).unwrap_or(0);
        // One budget per page, as `collectPageAnnotations` keeps one.
        let mut budget = ActionReadBudgetTracker::default();
        for index in 0..count {
            cancel.check()?;
            let Some(annotation) = document.raw_annotation(page_index, index) else {
                continue;
            };
            if annotation.subtype_code() != code::WIDGET {
                continue;
            }
            let base =
                read_annotation_record_base(document, &annotation, page, index, &mut budget)?;
            let mut row = read_widget_annotation(&annotation, base)?;
            join_widget_field(model, &mut row);
            rows.push(row);
        }
    }
    Ok(rows)
}
