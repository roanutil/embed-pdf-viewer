//! What a change answers (`ChangeItem`, `ChangeResult`, `ChangeAnswer` in
//! `core/src/mutation/Change.ts`).

use serde::{Deserialize, Serialize};

use super::attachments::AttachmentMutationMeta;
use crate::error::EngineError;
use crate::features::annotation::write::create::AnnotationMutationMeta;
use crate::features::files::attachments::EmbeddedFileItem;
use crate::features::forms::write::FormMutationMeta;
use crate::features::metadata_write::{CustomMetadataUpdateResult, MetadataUpdateResult};
use crate::features::outline::OutlineMutationMeta;
use crate::identity::PageRef;
use crate::mutation::MutationMeta;

/// What one op did: its single verb's result with the op's `type`; or, in an
/// undo, an op it left alone entirely (`skipped`). An undo's update items
/// list in `skipped` the entries they left alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChangeItem {
    #[serde(rename = "metadata.update")]
    MetadataUpdate {
        #[serde(flatten)]
        result: MetadataUpdateResult,
        /// The entries an undo left alone.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    #[serde(rename = "metadata.updateCustom")]
    MetadataUpdateCustom {
        #[serde(flatten)]
        result: CustomMetadataUpdateResult,
        /// The keys an undo left alone.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `AttachmentCreateResult`: the new attachment, read back.
    #[serde(rename = "attachments.create")]
    AttachmentsCreate { attachment: EmbeddedFileItem, meta: AttachmentMutationMeta },
    /// `AttachmentDeleteResult`: nothing exists after it, so only `meta`.
    #[serde(rename = "attachments.delete")]
    AttachmentsDelete { meta: AttachmentMutationMeta },
    /// `AttachmentRestoreResult`, only in an undo: the file a delete removed,
    /// back under its key.
    #[serde(rename = "attachments.restore")]
    AttachmentsRestore { attachment: EmbeddedFileItem, meta: AttachmentMutationMeta },
    /// `AnnotationCreateResult`, with the page the create named: the new
    /// annotation, read back as a listing reads it.
    #[serde(rename = "annotations.create")]
    AnnotationsCreate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        annotation: serde_json::Value,
        meta: AnnotationMutationMeta,
    },
    /// `AnnotationUpdateResult`, with its page: the annotation read back,
    /// what happened to its appearance, and in an undo the fields it left
    /// alone.
    #[serde(rename = "annotations.update")]
    AnnotationsUpdate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        annotation: serde_json::Value,
        appearance: crate::features::annotation::write::update::AppearanceOutcome,
        meta: AnnotationMutationMeta,
        /// The fields an undo left alone: they no longer showed what the
        /// change had set.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `AnnotationDeleteResult`, with its page: nothing exists after it, so
    /// only `meta`, whose `changed` names what went, the annotation first.
    #[serde(rename = "annotations.delete")]
    AnnotationsDelete {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        meta: AnnotationMutationMeta,
    },
    /// `AnnotationReorderResult`, with its page: the page's annotations in
    /// their new paint order, bottom to top, widgets excepted.
    #[serde(rename = "annotations.reorder")]
    AnnotationsReorder {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        order: Vec<crate::identity::AnnotationRef>,
        meta: AnnotationMutationMeta,
    },
    /// `AnnotationImportResult`: what an import made, in bundle order, each
    /// read back; each bundle ref and its ref here; and what it left out.
    /// Only the import verb and a server's change answer one.
    #[serde(rename = "annotations.import", rename_all = "camelCase")]
    AnnotationsImport {
        annotations: Vec<serde_json::Value>,
        ref_map: Vec<serde_json::Value>,
        dropped: Vec<serde_json::Value>,
        meta: AnnotationMutationMeta,
    },
    /// Only in an undo: what a delete removed, back, in `/Annots` order,
    /// each read as a listing reads it.
    #[serde(rename = "annotations.restore")]
    AnnotationsRestore {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        annotations: Vec<serde_json::Value>,
        meta: AnnotationMutationMeta,
    },
    /// `PageRotateResult`, with the pages turned and the rotation each has
    /// now.
    #[serde(rename = "pages.rotate")]
    PagesRotate {
        pages: Vec<PageRef>,
        rotation: crate::geom::PdfRotation,
        /// The new `PageListSnapshot`: in the file's coordinates as the op
        /// answers it, in page space once the boundary measured it.
        layout: serde_json::Value,
        meta: MutationMeta,
    },
    /// `PageReorderResult`: the pages that moved, in their new order.
    #[serde(rename = "pages.reorder")]
    PagesReorder { pages: Vec<PageRef>, layout: serde_json::Value, meta: MutationMeta },
    /// `PageDeleteResult`, with the pages that went, in the order they had.
    #[serde(rename = "pages.delete")]
    PagesDelete { pages: Vec<PageRef>, layout: serde_json::Value, meta: MutationMeta },
    /// `PageInsertResult`: the new pages, in the order they landed.
    #[serde(rename = "pages.insertBlank", rename_all = "camelCase")]
    PagesInsertBlank { inserted_pages: Vec<PageRef>, layout: serde_json::Value, meta: MutationMeta },
    #[serde(rename = "pages.insert", rename_all = "camelCase")]
    PagesInsert { inserted_pages: Vec<PageRef>, layout: serde_json::Value, meta: MutationMeta },
    /// `PageRestoreResult`, only in an undo: the pages a delete removed (or
    /// an insert brought, redone), back, in display order.
    #[serde(rename = "pages.restore")]
    PagesRestore { pages: Vec<PageRef>, layout: serde_json::Value, meta: MutationMeta },
    /// `FormSetValueResult`: the field read back, the rows of the widgets it
    /// changed, and its meta. Field and rows stay JSON, so one item holds
    /// them in either space ([`crate::page_space::forms`] converts them).
    #[serde(rename = "forms.setValue")]
    FormsSetValue {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormFieldUpdateResult` of a script's display effect.
    #[serde(rename = "forms.setDisplay")]
    FormsSetDisplay {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormFieldUpdateResult` of a script's formatted text.
    #[serde(rename = "forms.setAppearanceText")]
    FormsSetAppearanceText {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormFieldUpdateResult` of a signature field's drawn look.
    #[serde(rename = "forms.setSignatureAppearance")]
    FormsSetSignatureAppearance {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormResetResult`: the fields that changed, read back, and the rows
    /// of their changed widgets; in an undo, the fields it left alone, by
    /// name.
    #[serde(rename = "forms.reset")]
    FormsReset {
        fields: Vec<serde_json::Value>,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `FormFieldCreateResult`: the new field read back, its widget rows,
    /// and the calculation order when its calculate script changed it.
    #[serde(rename = "forms.create", rename_all = "camelCase")]
    FormsCreate {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        calculation_order: Option<Vec<crate::features::forms::snapshot::FormFieldRef>>,
        meta: FormMutationMeta,
    },
    /// `FormFieldUpdateResult` of a field update; in an undo, the
    /// properties it left alone.
    #[serde(rename = "forms.update", rename_all = "camelCase")]
    FormsUpdate {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        calculation_order: Option<Vec<crate::features::forms::snapshot::FormFieldRef>>,
        meta: FormMutationMeta,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `FormFieldDeleteResult`: nothing exists after it, so only `meta`
    /// (the field in `changedFields`, its widgets in `changedWidgets`), and
    /// the calculation order when the field was in it.
    #[serde(rename = "forms.delete", rename_all = "camelCase")]
    FormsDelete {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        calculation_order: Option<Vec<crate::features::forms::snapshot::FormFieldRef>>,
        meta: FormMutationMeta,
    },
    /// Only in an undo: a field a delete removed, back, with its widget
    /// rows.
    #[serde(rename = "forms.restore")]
    FormsRestore {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormWidgetLinkResult` of `addWidget`: the field read back, and the
    /// new widget's row.
    #[serde(rename = "forms.addWidget")]
    FormsAddWidget {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormWidgetLinkResult` of `removeWidget`: the field read back, and
    /// the widget's row, in no field.
    #[serde(rename = "forms.removeWidget")]
    FormsRemoveWidget {
        field: serde_json::Value,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormWidgetDeleteResult`: the widget gone, its page, and its field
    /// read back (`null` for a widget in no field).
    #[serde(rename = "forms.deleteWidget")]
    FormsDeleteWidget {
        widget: crate::identity::AnnotationRef,
        page: PageRef,
        field: Option<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// Only in an undo: a widget a delete removed, back on its page and in
    /// its field (`FormWidgetRestoreResult`).
    #[serde(rename = "forms.restoreWidget")]
    FormsRestoreWidget {
        field: Option<serde_json::Value>,
        widgets: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `FormWidgetsReorderResult`: every widget of the page, bottom to top.
    #[serde(rename = "forms.reorderWidgets")]
    FormsReorderWidgets {
        page: PageRef,
        order: Vec<crate::identity::AnnotationRef>,
        meta: FormMutationMeta,
    },
    /// `FormCalculationsReorderResult`: the calculation order, whole.
    #[serde(rename = "forms.reorderCalculations", rename_all = "camelCase")]
    FormsReorderCalculations {
        calculation_order: Vec<crate::features::forms::snapshot::FormFieldRef>,
        meta: FormMutationMeta,
    },
    /// `FormWidgetUpdateResult`: the widget's row read back and the
    /// appearance verdict; in an undo, the fields it left alone.
    #[serde(rename = "forms.updateWidget")]
    FormsUpdateWidget {
        widget: serde_json::Value,
        appearance: crate::features::annotation::write::update::AppearanceOutcome,
        meta: FormMutationMeta,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `FormImportResult`: the fields a design import made, in bundle
    /// order, their widget rows, each bundle ref and its ref here, what it
    /// left out, and the calculation order when it changed.
    #[serde(rename = "forms.import", rename_all = "camelCase")]
    FormsImport {
        fields: Vec<serde_json::Value>,
        widgets: Vec<serde_json::Value>,
        ref_map: Vec<serde_json::Value>,
        dropped: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        calculation_order: Option<Vec<crate::features::forms::snapshot::FormFieldRef>>,
        meta: FormMutationMeta,
    },
    /// `FormValuesImportResult`: the fields a values import filled, the
    /// rows of the widgets whose look changed, and what it left out.
    #[serde(rename = "forms.importValues")]
    FormsImportValues {
        fields: Vec<serde_json::Value>,
        widgets: Vec<serde_json::Value>,
        dropped: Vec<serde_json::Value>,
        meta: FormMutationMeta,
    },
    /// `OutlineCreateResult`: the new item read back, and its parent's
    /// children with it. Items stay JSON, so one item holds them in either
    /// space ([`crate::page_space::outline`] converts them).
    #[serde(rename = "outline.create")]
    OutlineCreate {
        item: serde_json::Value,
        order: crate::features::outline::OutlineOrder,
        meta: OutlineMutationMeta,
    },
    /// `OutlineUpdateResult`: the item read back; in an undo, the fields it
    /// left alone.
    #[serde(rename = "outline.update")]
    OutlineUpdate {
        item: serde_json::Value,
        meta: OutlineMutationMeta,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    /// `OutlineMoveResult`: the children of every parent the move changed.
    #[serde(rename = "outline.move")]
    OutlineMove { orders: Vec<crate::features::outline::OutlineOrder>, meta: OutlineMutationMeta },
    /// `OutlineDeleteResult`: in `meta.changed`, what went, the item first.
    #[serde(rename = "outline.delete")]
    OutlineDelete { meta: OutlineMutationMeta },
    /// `OutlineRestoreResult`, only in an undo: what a delete removed, back
    /// under the refs it had, and its parent's children with it.
    #[serde(rename = "outline.restore")]
    OutlineRestore {
        items: Vec<serde_json::Value>,
        order: crate::features::outline::OutlineOrder,
        meta: OutlineMutationMeta,
    },
    /// An op an undo left alone entirely (`SkippedChangeItem`): the document
    /// no longer showed what the change being undone had set, so nothing was
    /// written. `op` is the item it would have been.
    #[serde(rename = "skipped")]
    Skipped { op: String, meta: MutationMeta },
}

impl ChangeItem {
    pub fn meta(&self) -> &MutationMeta {
        match self {
            Self::MetadataUpdate { result, .. } => &result.meta,
            Self::MetadataUpdateCustom { result, .. } => &result.meta,
            Self::AttachmentsCreate { meta, .. }
            | Self::AttachmentsDelete { meta }
            | Self::AttachmentsRestore { meta, .. } => &meta.meta,
            Self::AnnotationsCreate { meta, .. }
            | Self::AnnotationsUpdate { meta, .. }
            | Self::AnnotationsDelete { meta, .. }
            | Self::AnnotationsReorder { meta, .. }
            | Self::AnnotationsImport { meta, .. }
            | Self::AnnotationsRestore { meta, .. } => &meta.meta,
            Self::PagesRotate { meta, .. }
            | Self::PagesReorder { meta, .. }
            | Self::PagesDelete { meta, .. }
            | Self::PagesInsertBlank { meta, .. }
            | Self::PagesInsert { meta, .. }
            | Self::PagesRestore { meta, .. }
            | Self::Skipped { meta, .. } => meta,
            Self::FormsSetValue { meta, .. }
            | Self::FormsSetDisplay { meta, .. }
            | Self::FormsSetAppearanceText { meta, .. }
            | Self::FormsSetSignatureAppearance { meta, .. }
            | Self::FormsReset { meta, .. }
            | Self::FormsCreate { meta, .. }
            | Self::FormsUpdate { meta, .. }
            | Self::FormsDelete { meta, .. }
            | Self::FormsRestore { meta, .. }
            | Self::FormsAddWidget { meta, .. }
            | Self::FormsRemoveWidget { meta, .. }
            | Self::FormsDeleteWidget { meta, .. }
            | Self::FormsRestoreWidget { meta, .. }
            | Self::FormsReorderWidgets { meta, .. }
            | Self::FormsReorderCalculations { meta, .. }
            | Self::FormsImport { meta, .. }
            | Self::FormsImportValues { meta, .. }
            | Self::FormsUpdateWidget { meta, .. } => &meta.meta,
            Self::OutlineCreate { meta, .. }
            | Self::OutlineUpdate { meta, .. }
            | Self::OutlineMove { meta, .. }
            | Self::OutlineDelete { meta }
            | Self::OutlineRestore { meta, .. } => &meta.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut MutationMeta {
        match self {
            Self::MetadataUpdate { result, .. } => &mut result.meta,
            Self::MetadataUpdateCustom { result, .. } => &mut result.meta,
            Self::AttachmentsCreate { meta, .. }
            | Self::AttachmentsDelete { meta }
            | Self::AttachmentsRestore { meta, .. } => &mut meta.meta,
            Self::AnnotationsCreate { meta, .. }
            | Self::AnnotationsUpdate { meta, .. }
            | Self::AnnotationsDelete { meta, .. }
            | Self::AnnotationsReorder { meta, .. }
            | Self::AnnotationsImport { meta, .. }
            | Self::AnnotationsRestore { meta, .. } => &mut meta.meta,
            Self::PagesRotate { meta, .. }
            | Self::PagesReorder { meta, .. }
            | Self::PagesDelete { meta, .. }
            | Self::PagesInsertBlank { meta, .. }
            | Self::PagesInsert { meta, .. }
            | Self::PagesRestore { meta, .. }
            | Self::Skipped { meta, .. } => meta,
            Self::FormsSetValue { meta, .. }
            | Self::FormsSetDisplay { meta, .. }
            | Self::FormsSetAppearanceText { meta, .. }
            | Self::FormsSetSignatureAppearance { meta, .. }
            | Self::FormsReset { meta, .. }
            | Self::FormsCreate { meta, .. }
            | Self::FormsUpdate { meta, .. }
            | Self::FormsDelete { meta, .. }
            | Self::FormsRestore { meta, .. }
            | Self::FormsAddWidget { meta, .. }
            | Self::FormsRemoveWidget { meta, .. }
            | Self::FormsDeleteWidget { meta, .. }
            | Self::FormsRestoreWidget { meta, .. }
            | Self::FormsReorderWidgets { meta, .. }
            | Self::FormsReorderCalculations { meta, .. }
            | Self::FormsImport { meta, .. }
            | Self::FormsImportValues { meta, .. }
            | Self::FormsUpdateWidget { meta, .. } => &mut meta.meta,
            Self::OutlineCreate { meta, .. }
            | Self::OutlineUpdate { meta, .. }
            | Self::OutlineMove { meta, .. }
            | Self::OutlineDelete { meta }
            | Self::OutlineRestore { meta, .. } => &mut meta.meta,
        }
    }

    /// Whether the item wrote to the document (`itemWrote`): not an op an
    /// undo left alone, nor an import that left every row of its bundle
    /// out.
    pub fn wrote(&self) -> bool {
        match self {
            Self::Skipped { .. } => false,
            Self::AnnotationsImport { annotations, .. } => !annotations.is_empty(),
            Self::FormsImport { fields, .. } | Self::FormsImportValues { fields, .. } => {
                !fields.is_empty()
            }
            _ => true,
        }
    }

    /// The events its single verb publishes (`changeEvents`' `itemEvents`):
    /// its result under the event's `type`, without the item's own `type`
    /// and `skipped`. An item an undo left alone publishes nothing.
    pub fn events(&self) -> Result<Vec<serde_json::Value>, EngineError> {
        let (event, result) = match self {
            Self::MetadataUpdate { result, .. } => ("metadata.updated", to_value(result)?),
            Self::MetadataUpdateCustom { result, .. } => {
                ("metadata.customUpdated", to_value(result)?)
            }
            Self::AttachmentsCreate { attachment, meta } => (
                "attachments.created",
                serde_json::json!({ "attachment": to_value(attachment)?, "meta": to_value(meta)? }),
            ),
            // `deleted` is what the delete removed (`deletedAttachmentOf`).
            Self::AttachmentsDelete { meta } => (
                "attachments.deleted",
                serde_json::json!({ "deleted": to_value(&meta.changed.first())?,
                    "meta": to_value(meta)? }),
            ),
            Self::AttachmentsRestore { attachment, meta } => (
                "attachments.restored",
                serde_json::json!({ "attachment": to_value(attachment)?, "meta": to_value(meta)? }),
            ),
            Self::AnnotationsCreate { page, annotation, meta } => (
                "annotations.created",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "annotation": annotation, "meta": to_value(meta)? }),
            ),
            Self::AnnotationsUpdate { page, annotation, appearance, meta, .. } => (
                "annotations.updated",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "annotation": annotation, "appearance": to_value(appearance)?,
                    "meta": to_value(meta)? }),
            ),
            // `deletedAnnotationsOf`: the refs in `meta.changed`.
            Self::AnnotationsDelete { page, meta } => (
                "annotations.deleted",
                serde_json::json!({ "deleted": to_value(&meta.changed)?,
                    "page": crate::identity::PageRef::new(*page), "meta": to_value(meta)? }),
            ),
            Self::AnnotationsReorder { page, order, meta } => (
                "annotations.reordered",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "order": to_value(order)?, "meta": to_value(meta)? }),
            ),
            // One `annotations.created` per annotation (`annotationImportFacts`).
            Self::AnnotationsImport { annotations, meta, .. } => {
                return Ok(crate::features::transfer::plan_annotation::annotation_import_facts(
                    annotations,
                    &to_value(meta)?,
                ));
            }
            // One `forms.created` per field (`formImportFacts`).
            Self::FormsImport { .. } => {
                return Ok(crate::features::transfer::plan_form::form_import_facts(&to_value(
                    self,
                )?));
            }
            // One `forms.valueSet` per field filled (`formValuesImportFacts`).
            Self::FormsImportValues { .. } => {
                return Ok(crate::features::transfer::plan_form::form_values_import_facts(
                    &to_value(self)?,
                ));
            }
            Self::AnnotationsRestore { page, annotations, meta } => (
                "annotations.restored",
                serde_json::json!({ "page": crate::identity::PageRef::new(*page),
                    "annotations": annotations, "meta": to_value(meta)? }),
            ),
            Self::PagesRotate { pages, rotation, layout, meta } => (
                "pages.rotated",
                serde_json::json!({ "pages": pages, "rotation": rotation, "layout": layout,
                    "meta": to_value(meta)? }),
            ),
            Self::PagesReorder { pages, layout, meta } => (
                "pages.reordered",
                serde_json::json!({ "pages": pages, "layout": layout, "meta": to_value(meta)? }),
            ),
            Self::PagesDelete { pages, layout, meta } => (
                "pages.deleted",
                serde_json::json!({ "pages": pages, "layout": layout, "meta": to_value(meta)? }),
            ),
            Self::PagesInsertBlank { inserted_pages, layout, meta }
            | Self::PagesInsert { inserted_pages, layout, meta } => (
                "pages.inserted",
                serde_json::json!({ "insertedPages": inserted_pages, "layout": layout,
                    "meta": to_value(meta)? }),
            ),
            Self::PagesRestore { pages, layout, meta } => (
                "pages.restored",
                serde_json::json!({ "pages": pages, "layout": layout, "meta": to_value(meta)? }),
            ),
            Self::FormsSetValue { field, widgets, meta } => (
                "forms.valueSet",
                serde_json::json!({ "field": field, "widgets": widgets, "meta": to_value(meta)? }),
            ),
            Self::FormsSetDisplay { field, widgets, meta }
            | Self::FormsSetAppearanceText { field, widgets, meta }
            | Self::FormsSetSignatureAppearance { field, widgets, meta } => (
                "forms.updated",
                serde_json::json!({ "field": field, "widgets": widgets, "meta": to_value(meta)? }),
            ),
            // One `forms.valueSet` per field that changed (`formResetFacts`).
            Self::FormsReset { fields, widgets, meta, .. } => {
                return form_reset_facts(fields, widgets, meta)
                    .into_iter()
                    .map(|fact| {
                        let mut event = serde_json::json!({ "type": "forms.valueSet" });
                        if let (Some(object), serde_json::Value::Object(fields)) =
                            (event.as_object_mut(), fact?)
                        {
                            object.extend(fields);
                        }
                        Ok(event)
                    })
                    .collect();
            }
            Self::FormsCreate { .. }
            | Self::FormsUpdate { .. }
            | Self::FormsDelete { .. }
            | Self::FormsRestore { .. }
            | Self::FormsAddWidget { .. }
            | Self::FormsRemoveWidget { .. }
            | Self::FormsDeleteWidget { .. }
            | Self::FormsRestoreWidget { .. }
            | Self::FormsReorderWidgets { .. }
            | Self::FormsReorderCalculations { .. }
            | Self::FormsUpdateWidget { .. } => return self.field_events(),
            Self::OutlineCreate { item, order, meta } => (
                "outline.created",
                serde_json::json!({ "item": item, "order": to_value(order)?,
                    "meta": to_value(meta)? }),
            ),
            Self::OutlineUpdate { item, meta, .. } => {
                ("outline.updated", serde_json::json!({ "item": item, "meta": to_value(meta)? }))
            }
            Self::OutlineMove { orders, meta } => (
                "outline.moved",
                serde_json::json!({ "orders": to_value(orders)?, "meta": to_value(meta)? }),
            ),
            // `deletedOutlineItemsOf`: the refs in `meta.changed`.
            Self::OutlineDelete { meta } => (
                "outline.deleted",
                serde_json::json!({ "deleted": to_value(&meta.changed)?,
                    "meta": to_value(meta)? }),
            ),
            Self::OutlineRestore { items, order, meta } => (
                "outline.restored",
                serde_json::json!({ "items": items, "order": to_value(order)?,
                    "meta": to_value(meta)? }),
            ),
            Self::Skipped { .. } => return Ok(Vec::new()),
        };
        let mut event_value = serde_json::json!({ "type": event });
        if let (Some(object), serde_json::Value::Object(fields)) =
            (event_value.as_object_mut(), result)
        {
            object.extend(fields);
        }
        Ok(vec![event_value])
    }
}

impl ChangeItem {
    /// A field or widget op's one event (`itemEvents`): the item without its
    /// `type` and `skipped`, under the event's type; a delete names the field
    /// it removed (`deletedFieldOf`).
    fn field_events(&self) -> Result<Vec<serde_json::Value>, EngineError> {
        let event = match self {
            Self::FormsCreate { .. } => "forms.created",
            Self::FormsUpdate { .. } => "forms.updated",
            Self::FormsDelete { .. } => "forms.deleted",
            Self::FormsRestore { .. } => "forms.restored",
            Self::FormsAddWidget { .. } => "forms.widgetAdded",
            Self::FormsRemoveWidget { .. } => "forms.widgetRemoved",
            Self::FormsDeleteWidget { .. } => "forms.widgetDeleted",
            Self::FormsRestoreWidget { .. } => "forms.widgetRestored",
            Self::FormsReorderWidgets { .. } => "forms.widgetsReordered",
            Self::FormsReorderCalculations { .. } => "forms.calculationsReordered",
            Self::FormsUpdateWidget { .. } => "forms.widgetUpdated",
            _ => return Ok(Vec::new()),
        };
        let mut value = to_value(self)?;
        let Some(fields) = value.as_object_mut() else {
            return Ok(Vec::new());
        };
        fields.remove("type");
        fields.remove("skipped");
        let mut out = serde_json::Map::new();
        out.insert("type".into(), serde_json::Value::String(event.to_owned()));
        if let Self::FormsDelete { meta, .. } = self {
            out.insert("deleted".into(), to_value(&meta.changed_fields.first())?);
        }
        out.extend(std::mem::take(fields));
        Ok(vec![serde_json::Value::Object(out)])
    }
}

/// A reset as one value write per changed field (`formResetFacts`): each
/// field with the part of the meta that is its own.
fn form_reset_facts(
    fields: &[serde_json::Value],
    widgets: &[serde_json::Value],
    meta: &FormMutationMeta,
) -> Vec<Result<serde_json::Value, EngineError>> {
    fields
        .iter()
        .map(|field| {
            let own: Vec<u64> = field["widgets"]
                .as_array()
                .map(|w| w.iter().filter_map(|w| w["objectNumber"].as_u64()).collect())
                .unwrap_or_default();
            let mine = |widget: &crate::features::forms::write::MetaWidget| {
                own.contains(&u64::from(widget.widget().annot_object_number))
            };
            let changed_widgets: Vec<_> =
                meta.changed_widgets.iter().filter(|w| mine(w)).cloned().collect();
            let rows: Vec<&serde_json::Value> = widgets
                .iter()
                .filter(|row| {
                    row["ref"]["kind"] == "objectNumber"
                        && row["ref"]["objectNumber"].as_u64().is_some_and(|n| own.contains(&n))
                })
                .collect();
            let pages: Vec<u32> = changed_widgets
                .iter()
                .map(|w| w.widget().page_object_number)
                .filter(|page| *page > 0)
                .collect();
            let fact_meta = FormMutationMeta {
                meta: MutationMeta {
                    affected_pages: meta
                        .meta
                        .affected_pages
                        .iter()
                        .filter(|page| pages.contains(&page.object_number().get()))
                        .copied()
                        .collect(),
                    cache_delta: None,
                    op_id: meta.meta.op_id.clone(),
                    undoable: meta.meta.undoable,
                },
                changed_fields: vec![serde_json::from_value(field["ref"].clone()).map_err(
                    |e| {
                        EngineError::with_message(
                            crate::error::EngineErrorCode::Unknown,
                            format!("a reset's field has no ref: {e}"),
                        )
                    },
                )?],
                redrawn_widgets: meta.redrawn_widgets.iter().filter(|w| mine(w)).cloned().collect(),
                changed_widgets,
            };
            Ok(serde_json::json!({ "field": field, "widgets": rows,
                "meta": to_value(&fact_meta)? }))
        })
        .collect()
}

/// What `document.apply` answers (`ChangeResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResult {
    /// One per op, in op order; for an undo, one per step of the record.
    pub items: Vec<ChangeItem>,
    /// Every page the change touched, the change's `opId` and whether it
    /// can be undone.
    pub meta: MutationMeta,
}

impl ChangeResult {
    /// Whether any item wrote: a change that wrote nothing needs no save.
    pub fn wrote(&self) -> bool {
        self.items.iter().any(ChangeItem::wrote)
    }
}

/// Every page the items touched, once each, in order (`pagesOf`).
pub fn pages_of(items: &[ChangeItem]) -> Vec<PageRef> {
    let mut pages: Vec<PageRef> = Vec::new();
    for item in items {
        for page in &item.meta().affected_pages {
            if !pages.contains(page) {
                pages.push(*page);
            }
        }
    }
    pages
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, EngineError> {
    serde_json::to_value(value).map_err(|e| {
        EngineError::with_message(
            crate::error::EngineErrorCode::WireFormat,
            format!("a change item could not be serialized: {e}"),
        )
    })
}
