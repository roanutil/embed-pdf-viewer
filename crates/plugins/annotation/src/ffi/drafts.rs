use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, PageRef, Point, ViewerError};
use epdf_kernel::bytes::Bytes;
use serde_json::{Map, Value, json};

use super::{
    AnnotationCreationDraft, draft_out, geometry_point, open, page_json, point_json, ref_json,
    ref_out, refs_out,
};
use crate::AnnotationCapability;
use crate::read::chrome::HitView;
use crate::write::stamps::StampInput;

/// Work in progress: creation drafts, text editing, markup from text,
/// stamps and click placement.
#[derive(uniffi::Object)]
pub struct AnnotationDraftsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// An image or single-page PDF to stamp. The engine sniffs the format from
/// the bytes; `mime_type` is advisory.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationStampSource {
    pub bytes: Vec<u8>,
    #[uniffi(default = None)]
    pub mime_type: Option<String>,
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
}

impl AnnotationStampSource {
    /// `StampInput` as JSON: `{ source: {data, mimeType?}, targetWidth?, name?, subject? }`.
    fn input(&self, target_width: Option<f64>) -> StampInput {
        let mut source = json!({ "data": Bytes(self.bytes.clone()).to_json() });
        if let Some(mime) = &self.mime_type {
            source["mimeType"] = mime.clone().into();
        }
        let mut input = Map::new();
        input.insert("source".into(), source);
        if let Some(width) = target_width {
            input.insert("targetWidth".into(), json!(width));
        }
        if let Some(name) = &self.name {
            input.insert("name".into(), json!(name));
        }
        if let Some(subject) = &self.subject {
            input.insert("subject".into(), json!(subject));
        }
        StampInput::read(&Value::Object(input))
    }
}

/// Where a stamp lands: its center, in page space (`StampPlacement`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationStampPlacement {
    pub page: PageRef,
    pub center: Point,
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
}

impl AnnotationStampPlacement {
    fn json(&self) -> Value {
        let mut placement =
            json!({ "page": page_json(self.page), "center": point_json(self.center) });
        if let Some(rotation) = self.rotation {
            placement["rotation"] = json!(rotation);
        }
        placement
    }
}

/// A file to attach: bytes plus the name the PDF stores (`/UF`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationAttachmentFile {
    pub bytes: Vec<u8>,
    pub name: String,
    #[uniffi(default = None)]
    pub mime_type: Option<String>,
    #[uniffi(default = None)]
    pub description: Option<String>,
}

/// The end of a run of selected text, as the selection plugin reads it
/// (`TextEndAnchor`), as JSON: the last glyph's quad and which way the text
/// runs from it. A caret goes there.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationTextEndAnchor {
    pub anchor_json: String,
}

/// One page's text quads (`Quad[]` as JSON) for a markup preview.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPageQuads {
    pub page: PageRef,
    pub quads_json: String,
}

fn json_of(text: &str, what: &str) -> Result<Value, epdf_kernel::Error> {
    serde_json::from_str(text).map_err(|e| super::invalid(format!("{what} is not JSON: {e}")))
}

#[uniffi::export]
impl AnnotationDraftsHandle {
    /// `draft.get()`: the polygon or polyline being drawn.
    pub async fn creation_draft(&self) -> Result<Option<AnnotationCreationDraft>, ViewerError> {
        self.cap.call(|c| draft_out(&open(c)?.draft_get())).await
    }

    /// `draft.finish()`: commit the polygon or polyline being drawn. `None`
    /// when there was nothing to finish.
    pub async fn finish_creation_draft(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.draft_finish(None)),
                |made: Value| match made.get("ref") {
                    None | Some(Value::Null) => Ok(None),
                    Some(r) => ref_out(r).map(Some).map_err(ViewerError::from),
                },
            )
            .await?
    }

    pub async fn finish_ink_draft(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.finish_ink_draft();
                Ok(())
            })
            .await
    }

    /// `draft.cancel()` / `cancel()`: drop every gesture in progress.
    pub async fn cancel(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.cancel();
                Ok(())
            })
            .await
    }

    /// `text.begin(ref)`.
    pub async fn begin_text_edit(&self, annotation_ref: AnnotationRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.text_begin(&ref_json(&annotation_ref)?);
                Ok(())
            })
            .await
    }

    /// Start editing whatever free text is under a page-space point. `scale`,
    /// `rotation` and `zoom` describe the view the point came from, for the
    /// hit slop. `false` when no editable free text is there.
    pub async fn begin_text_edit_at(
        &self,
        page: PageRef,
        point: Point,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| {
                let view = HitView { scale, rotation, zoom };
                Ok(open(c)?.begin_text_edit_at(&page_json(page), geometry_point(point), view))
            })
            .await
    }

    /// `text.end()`: send what was typed and stop editing.
    pub async fn end_text_edit(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.text_end(None)), |()| ()).await
    }

    /// `text.getEditing()`: the annotation being typed into.
    pub async fn editing_ref(&self) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call(|c| match open(c)?.get_editing() {
                Some(dto) => ref_out(dto.get("ref").unwrap_or(&Value::Null)).map(Some),
                None => Ok(None),
            })
            .await
    }

    /// Keystroke-level text, sent once typing pauses or at `end_text_edit`.
    pub async fn draft_contents(
        &self,
        annotation_ref: AnnotationRef,
        text: String,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c)?.draft_contents(&ref_json(&annotation_ref)?, &text)).await
    }

    /// The caret or selection inside the edited text, in UTF-16 code units.
    /// `None` for either end clears it.
    pub async fn set_text_selection(
        &self,
        annotation_ref: AnnotationRef,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let range = start.zip(end).map(|(s, e)| (f64::from(s), f64::from(e)));
                open(c)?.set_text_selection(&ref_json(&annotation_ref)?, range);
                Ok(())
            })
            .await
    }

    /// Highlight, underline, squiggly or strikeout over text quads
    /// (`quads_json`, `Quad[]`). `preset` picks a tool's defaults other than
    /// the subtype's own.
    pub async fn create_markup(
        &self,
        page: PageRef,
        subtype: String,
        quads_json: String,
        preset: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let quads = json_of(&quads_json, "quads")?;
                open(c)?.create_markup(&subtype, &page_json(page), &quads, preset.as_deref())
            })
            .await
    }

    /// An insert caret at the end of a run of text.
    pub async fn create_caret(
        &self,
        page: PageRef,
        anchor: AnnotationTextEndAnchor,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let anchor = json_of(&anchor.anchor_json, "an anchor")?;
                open(c)?.create_caret(&page_json(page), &anchor)
            })
            .await
    }

    /// A strikeout over `quads_json` paired with an insert caret at `anchor`.
    pub async fn create_replace_text(
        &self,
        page: PageRef,
        quads_json: String,
        anchor: AnnotationTextEndAnchor,
        preset: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let quads = json_of(&quads_json, "quads")?;
                let anchor = json_of(&anchor.anchor_json, "an anchor")?;
                open(c)?.create_replace_text(&page_json(page), &quads, &anchor, preset.as_deref())
            })
            .await
    }

    pub async fn preview_markup(
        &self,
        subtype: String,
        pages: Vec<AnnotationPageQuads>,
        preset: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let mut by_page = Map::new();
                for p in pages {
                    by_page.insert(
                        epdf_kernel::js::number_to_string(f64::from(p.page.page_object_number)),
                        json_of(&p.quads_json, "quads")?,
                    );
                }
                open(c)?.preview_markup(&subtype, &Value::Object(by_page), preset.as_deref())
            })
            .await
    }

    pub async fn clear_markup_preview(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.clear_markup_preview();
                Ok(())
            })
            .await
    }

    /// `createFromSelection(toolId, { clear })`: mark the current text
    /// selection (needs the selection plugin) with a markup tool, one
    /// annotation per page. `clear` (default `true`) drops the text
    /// selection afterwards.
    pub async fn create_from_selection(
        &self,
        tool_id: String,
        clear: Option<bool>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_from_selection(&tool_id, clear != Some(false), None)),
                |made: Value| {
                    let refs: Value = made
                        .as_array()
                        .map(|list| list.iter().filter_map(|a| a.get("ref").cloned()).collect())
                        .unwrap_or_default();
                    refs_out(&refs).map_err(ViewerError::from)
                },
            )
            .await?
    }

    /// `stamps.arm(stamp)`: the next click on a page places it.
    pub async fn arm_stamp(
        &self,
        source: AnnotationStampSource,
        target_width: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.stamps_arm(source.input(target_width), None)),
                |()| (),
            )
            .await
    }

    pub async fn disarm_stamp(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.stamps_disarm();
                Ok(())
            })
            .await
    }

    pub async fn has_armed_stamp(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.stamps_is_armed())).await
    }

    /// `stamps.place(stamp, placement)`: place a stamp without arming.
    pub async fn place_stamp(
        &self,
        source: AnnotationStampSource,
        placement: AnnotationStampPlacement,
        target_width: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = source.input(target_width);
                    Ok(open(c)?.stamps_place(input, &placement.json(), None))
                },
                |made: Value| ref_out(&made["annotation"]["ref"]).map_err(ViewerError::from),
            )
            .await?
    }

    /// Place the armed stamp at a point. `false` when none is armed; the
    /// created annotation arrives as a `Created` event.
    pub async fn place_armed_stamp(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.place_armed_stamp(
                    f64::from(page.page_object_number),
                    geometry_point(point),
                    display_rotation,
                ))
            })
            .await
    }

    /// Ask the file-picker port for a file for the active tool and place it
    /// at `point`. `false` when the active tool takes no picked file.
    pub async fn request_stamp_at(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.request_stamp_at(
                    f64::from(page.page_object_number),
                    geometry_point(point),
                    display_rotation,
                ))
            })
            .await
    }

    /// Show `tool_id`'s ghost under a page-space point.
    pub async fn hover_ghost_at(
        &self,
        tool_id: String,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.hover_ghost_at(
                    &tool_id,
                    &page_json(page),
                    geometry_point(point),
                    display_rotation,
                    zoom,
                );
                Ok(())
            })
            .await
    }

    pub async fn clear_ghost(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.clear_ghost();
                Ok(())
            })
            .await
    }

    /// `previewPlacement(toolId, page, from, to)`: a sibling's drag-to-place
    /// gesture with one of this plugin's tools.
    pub async fn preview_placement(
        &self,
        tool_id: String,
        page: PageRef,
        from: Point,
        to: Point,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.preview_placement(
                    &tool_id,
                    &page_json(page),
                    geometry_point(from),
                    geometry_point(to),
                );
                Ok(())
            })
            .await
    }

    pub async fn clear_placement_preview(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.clear_placement_preview();
                Ok(())
            })
            .await
    }

    /// Click-place with the active tool (a note, a stamp). `false` when the
    /// tool places nothing on click.
    pub async fn place_at(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.place_at(
                    f64::from(page.page_object_number),
                    geometry_point(point),
                    display_rotation,
                    zoom,
                ))
            })
            .await
    }

    /// `create(page, { subtype: 'file-attachment', … }, { file })`: an
    /// attachment at a page point.
    pub async fn create_attachment(
        &self,
        page: PageRef,
        point: Point,
        file: AnnotationAttachmentFile,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let mut source =
                        json!({ "data": Bytes(file.bytes).to_json(), "name": file.name });
                    if let Some(mime) = file.mime_type {
                        source["mimeType"] = mime.into();
                    }
                    if let Some(description) = file.description {
                        source["description"] = description.into();
                    }
                    let mut fields = Map::new();
                    fields.insert("subtype".into(), json!("file-attachment"));
                    fields.insert(
                        "box".into(),
                        json!({ "x": point.x, "y": point.y, "width": 24, "height": 24 }),
                    );
                    let resources = json!({ "file": source });
                    Ok(open(c)?.create(
                        &page_json(page),
                        &fields,
                        Some(resources),
                        Default::default(),
                    ))
                },
                |created: Value| ref_out(&created["annotation"]["ref"]).map_err(ViewerError::from),
            )
            .await?
    }

    /// `downloadResource(ref, 'file')`: an attachment's embedded bytes.
    pub async fn read_attachment(
        &self,
        annotation_ref: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.download_resource(&ref_json(&annotation_ref)?, "file", None)),
                |file: Value| {
                    let data =
                        file.get("data").or_else(|| file.get("bytes")).cloned().unwrap_or(file);
                    Bytes::from_json(&data).map(|b| b.0).ok_or_else(|| {
                        ViewerError::from(super::invalid("an attachment read answered no bytes"))
                    })
                },
            )
            .await?
    }
}
