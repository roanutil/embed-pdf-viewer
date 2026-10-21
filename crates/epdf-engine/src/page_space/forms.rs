//! The form in page space (`core/src/pageSpace/forms.ts`).
//!
//! A field holds no place of its own: only its actions' destinations move,
//! each measured on the page it goes to. A widget row's `rect` and `box`
//! are measured on the widget's page, and its actions' destinations on
//! theirs (`pageAnnotationOf` over the widget's field spaces).

use super::destinations::page_destination_of;
use super::{PageCoordinates, PdfCoordinates, VisibleBoxOf, page_rect_of};
use crate::error::EngineError;
use crate::features::annotation::read::record::{AnnotationRecordBase, WidgetAnnotation};
use crate::features::forms::snapshot::{FormFieldBase, FormFieldDto, FormSnapshot};

/// A form field in page space: each destination of its actions on the page
/// it goes to (`pageFormFieldOf`).
pub fn page_form_field_of(
    field: FormFieldDto<PdfCoordinates>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<FormFieldDto<PageCoordinates>, EngineError> {
    let FormFieldDto { base, kind } = field;
    let actions = base
        .actions
        .map(|actions| {
            actions.try_map_destinations(&mut |destination| {
                page_destination_of(destination, &mut *box_of)
            })
        })
        .transpose()?;
    Ok(FormFieldDto {
        base: FormFieldBase::<PageCoordinates> {
            field_ref: base.field_ref,
            name: base.name,
            family: base.family,
            origin: base.origin,
            read_only: base.read_only,
            required: base.required,
            no_export: base.no_export,
            alternate_name: base.alternate_name,
            mapping_name: base.mapping_name,
            value_entry: base.value_entry,
            default_value_entry: base.default_value_entry,
            actions,
            group_id: base.group_id,
            attribution: base.attribution,
        },
        kind,
    })
}

/// A widget row in page space, measured on its page (`pageWidgetOf`).
pub fn page_widget_of(
    widget: WidgetAnnotation<PdfCoordinates>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<WidgetAnnotation<PageCoordinates>, EngineError> {
    let visible = box_of(widget.base.page)?;
    let base = widget.base;
    let actions = base
        .actions
        .map(|actions| {
            actions.try_map_destinations(&mut |destination| {
                page_destination_of(destination, &mut *box_of)
            })
        })
        .transpose()?;
    Ok(WidgetAnnotation {
        base: AnnotationRecordBase::<PageCoordinates> {
            annotation_ref: base.annotation_ref,
            page: base.page,
            has_appearance: base.has_appearance,
            appearance_state: base.appearance_state,
            nm: base.nm,
            flags: base.flags,
            rect: page_rect_of(base.rect, visible),
            contents: base.contents,
            subject: base.subject,
            author: base.author,
            created_at: base.created_at,
            modified_at: base.modified_at,
            blend_mode: base.blend_mode,
            reply: base.reply,
            popup: base.popup,
            group_id: base.group_id,
            user_id: base.user_id,
            created_by: base.created_by,
            modified_by: base.modified_by,
            imported_by: base.imported_by,
            actions,
        },
        content_box: page_rect_of(widget.content_box, visible),
        rotation: widget.rotation,
        subtype: widget.subtype,
        color: widget.color,
        interior_color: widget.interior_color,
        stroke_width: widget.stroke_width,
        border_style: widget.border_style,
        dash_array: widget.dash_array,
        font_family: widget.font_family,
        font_size: widget.font_size,
        font_color: widget.font_color,
        text_align: widget.text_align,
        caption: widget.caption,
        check_style: widget.check_style,
        field: widget.field,
        field_family: widget.field_family,
    })
}

/// A form in page space: its fields, and each widget row on its page
/// (`pageFormSnapshotOf`).
pub fn page_form_snapshot_of(
    snapshot: FormSnapshot<PdfCoordinates>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<FormSnapshot<PageCoordinates>, EngineError> {
    Ok(FormSnapshot {
        form_kind: snapshot.form_kind,
        needs_appearances: snapshot.needs_appearances,
        fields: snapshot
            .fields
            .into_iter()
            .map(|field| page_form_field_of(field, &mut *box_of))
            .collect::<Result<_, _>>()?,
        widgets: snapshot
            .widgets
            .into_iter()
            .map(|widget| page_widget_of(widget, &mut *box_of))
            .collect::<Result<_, _>>()?,
        calculation_order: snapshot.calculation_order,
    })
}

#[cfg(test)]
mod tests {
    //! The read half of `describe('render, form and measure values')` in
    //! `packages/engine/core/test/pageSpace/features.test.ts`.

    use serde_json::{Value, json};

    use super::*;
    use crate::geom::{PageRect, PdfRect};
    use crate::identity::PageObjectNumber;

    /// `crop` in the upstream test, the visible box of page 4.
    fn box_of(page: PageObjectNumber) -> Result<PdfRect, EngineError> {
        assert_eq!(page.get(), 4, "only page 4 is in this document");
        Ok(PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 })
    }

    fn page_ref(n: u32) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": n })
    }

    fn pushbutton(name: &str, actions: Option<Value>) -> Value {
        let mut field = json!({
            "ref": { "kind": "objectNumber", "objectNumber": 9 }, "name": name,
            "family": "pushbutton", "origin": "acroform", "readOnly": false,
            "required": false, "noExport": false, "alternateName": null, "mappingName": null,
            "valueEntry": { "kind": "none" }, "defaultValueEntry": { "kind": "none" },
            "groupId": null, "createdBy": null, "createdAt": null, "filledBy": null,
            "filledByName": null, "filledAt": null, "importedBy": null, "widgets": []
        });
        if let Some(actions) = actions {
            field["actions"] = actions;
        }
        field
    }

    fn calculate(destination: Value) -> Value {
        json!({ "calculate": { "incomplete": false, "warningFlags": 0, "warnings": [],
            "root": { "subtype": "GoTo", "type": "goto", "destination": destination,
                "next": [] } } })
    }

    #[test]
    fn a_fields_actions_go_to_spots_on_the_pages_they_go_to_and_the_rest_stays() {
        let field = pushbutton(
            "go",
            Some(calculate(json!({ "kind": "fitH", "page": page_ref(4), "top": 700 }))),
        );
        let read: FormFieldDto = serde_json::from_value(field.clone()).expect("a field");
        let page = serde_json::to_value(page_form_field_of(read, &mut box_of).unwrap()).unwrap();
        let mut expected = field;
        expected["actions"] = calculate(json!({ "kind": "fitH", "page": page_ref(4), "y": 32.0 }));
        assert_eq!(page, expected);

        let plain = pushbutton("plain", None);
        let read: FormFieldDto = serde_json::from_value(plain.clone()).expect("a field");
        let page = serde_json::to_value(page_form_field_of(read, &mut box_of).unwrap()).unwrap();
        assert_eq!(page, plain);
    }

    #[test]
    fn the_forms_widget_rows_are_measured_on_their_pages() {
        let rect = json!({ "left": 100, "bottom": 600, "right": 150, "top": 650 });
        let mut row = json!({
            "ref": { "kind": "objectNumber", "page": page_ref(4), "objectNumber": 8 },
            "page": page_ref(4), "hasAppearance": false, "appearanceState": null, "nm": null,
            "rect": rect, "box": rect, "rotation": null, "subtype": "widget",
            "field": null, "fieldFamily": "unknown", "actions": null
        });
        let nulls = [
            "contents",
            "subject",
            "author",
            "createdAt",
            "modifiedAt",
            "reply",
            "popup",
            "groupId",
            "userId",
            "createdBy",
            "modifiedBy",
            "importedBy",
            "color",
            "interiorColor",
            "dashArray",
            "fontFamily",
            "fontSize",
            "fontColor",
            "caption",
            "checkStyle",
        ];
        for key in nulls {
            row[key] = Value::Null;
        }
        let flags = [
            "invisible",
            "hidden",
            "print",
            "noZoom",
            "noRotate",
            "noView",
            "readOnly",
            "locked",
            "toggleNoView",
            "lockedContents",
        ];
        for key in flags {
            row[key] = json!(key == "print");
        }
        row["blendMode"] = json!("normal");
        row["strokeWidth"] = json!(1);
        row["borderStyle"] = json!("solid");
        row["textAlign"] = json!("left");
        let row: WidgetAnnotation = serde_json::from_value(row).expect("a widget row");
        let page = page_widget_of(row, &mut box_of).expect("converts");
        let expected = PageRect { x: 50.0, y: 82.0, width: 50.0, height: 50.0 };
        assert_eq!(page.base.rect, expected);
        assert_eq!(page.content_box, expected);
    }
}
