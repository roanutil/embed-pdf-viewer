use std::sync::Arc;

use epdf_annotation_core::js::Record;
use epdf_ffi::{
    AnnotationRef, CancelToken, CreationDraftAnchor, KernelBound, PageRef, Point, Rect, TextQuad,
    ViewerError,
};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::js::number_to_string;
use serde_json::json;

use super::{open, ref_out, refs_out};
use crate::AnnotationCapability;

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
    /// A `BinarySource` as canonical JSON: `{data, mimeType?}`.
    fn source_json(&self) -> serde_json::Value {
        let mut source = json!({ "data": Bytes(self.bytes.clone()).to_json() });
        if let Some(mime) = &self.mime_type {
            source["mimeType"] = mime.clone().into();
        }
        source
    }

    fn input(&self, target_width: Option<f64>) -> crate::StampToolInput {
        crate::StampToolInput {
            source: self.source_json(),
            target_width,
            name: self.name.clone(),
            subject: self.subject.clone(),
            ..Default::default()
        }
    }
}

/// Where a stamp lands: its center, in page space.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationStampPlacement {
    pub page: PageRef,
    pub at: Point,
    #[uniffi(default = None)]
    pub target_width: Option<f64>,
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
}

impl From<AnnotationStampPlacement> for crate::StampPlacement {
    fn from(p: AnnotationStampPlacement) -> Self {
        crate::StampPlacement {
            page: p.page.into(),
            at: p.at.into(),
            target_width: p.target_width,
            rotation: p.rotation,
        }
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

/// The end of a run of selected text: the last glyph's quad, and which way
/// the text runs from it (`1` toward its end, `-1` toward its start). A
/// caret goes there.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationTextEndAnchor {
    pub glyph_quad: TextQuad,
    pub advance: f64,
}

impl From<AnnotationTextEndAnchor> for epdf_annotation_core::TextEndAnchor {
    fn from(a: AnnotationTextEndAnchor) -> Self {
        epdf_annotation_core::TextEndAnchor { glyph_quad: a.glyph_quad.into(), advance: a.advance }
    }
}

/// One page's quads for a markup preview.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPageQuads {
    pub page: PageRef,
    pub quads: Vec<TextQuad>,
}

fn quads_in(quads: Vec<TextQuad>) -> Vec<epdf_annotation_core::TextQuad> {
    quads.into_iter().map(Into::into).collect()
}

#[uniffi::export]
impl AnnotationDraftsHandle {
    pub async fn creation_draft(&self) -> Result<Option<CreationDraftAnchor>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_creation_draft().map(Into::into))).await
    }

    pub async fn has_creation_draft(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.has_creation_draft())).await
    }

    /// Commit the polygon or polyline being drawn. `None` when there was
    /// nothing to finish.
    pub async fn finish_creation_draft(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.finish_creation_draft()),
                |r| r.map(ref_out).transpose().map_err(ViewerError::from),
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

    pub async fn cancel_creation_draft(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.cancel_creation_draft();
                Ok(())
            })
            .await
    }

    /// Drop every gesture in progress: drafts, ghosts, placement previews.
    pub async fn cancel(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.cancel();
                Ok(())
            })
            .await
    }

    pub async fn set_contents(
        &self,
        annotation_ref: AnnotationRef,
        text: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_contents(annotation_ref.into(), text)),
                |()| (),
            )
            .await
    }

    pub async fn begin_text_edit(&self, annotation_ref: AnnotationRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.begin_text_edit(&annotation_ref.into());
                Ok(())
            })
            .await
    }

    /// Start editing whatever free text is under a page-space point. `scale`,
    /// `rotation` and `zoom` describe the view the point came from, for the
    /// hit slop; `None` is the unscaled, unrotated page. `false` when no
    /// free text is there.
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
                Ok(open(c)?.begin_text_edit_at(page.into(), point.into(), scale, rotation, zoom))
            })
            .await
    }

    pub async fn end_text_edit(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.end_text_edit()), |()| ()).await
    }

    pub async fn editing_ref(&self) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap.call(|c| open(c)?.get_editing_ref().map(ref_out).transpose()).await
    }

    pub async fn editing_id(&self) -> Result<Option<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_editing_id())).await
    }

    /// Keystroke-level text, kept as a draft until `end_text_edit`.
    pub async fn draft_contents(
        &self,
        annotation_ref: AnnotationRef,
        text: String,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.draft_contents(annotation_ref.into(), text);
                Ok(())
            })
            .await
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
                open(c)?.set_text_selection(&annotation_ref.into(), range);
                Ok(())
            })
            .await
    }

    /// Highlight, underline, squiggly or strikeout over text quads. The
    /// created annotation arrives as a `Created` event; `preset` picks a
    /// tool's defaults other than the subtype's own.
    pub async fn create_markup(
        &self,
        page: PageRef,
        subtype: String,
        quads: Vec<TextQuad>,
        preset: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.create_markup(&subtype, page.into(), quads_in(quads), preset.as_deref());
                Ok(())
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
                open(c)?.create_caret(page.into(), anchor.into());
                Ok(())
            })
            .await
    }

    /// A strikeout over `quads` paired with an insert caret at `anchor`.
    pub async fn create_replace_text(
        &self,
        page: PageRef,
        quads: Vec<TextQuad>,
        anchor: AnnotationTextEndAnchor,
        preset: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.create_replace_text(
                    page.into(),
                    quads_in(quads),
                    anchor.into(),
                    preset.as_deref(),
                );
                Ok(())
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
                let mut by_page = Record::new();
                for p in pages {
                    by_page.insert(
                        number_to_string(f64::from(p.page.page_object_number)),
                        quads_in(p.quads),
                    );
                }
                open(c)?.preview_markup(&subtype, by_page, preset.as_deref());
                Ok(())
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

    /// Mark the current text selection (needs the selection plugin) with
    /// `subtype`, one annotation per page. Empty when nothing is selected.
    /// `clear` drops the text selection afterwards.
    pub async fn create_from_selection(
        &self,
        subtype: String,
        preset: Option<String>,
        clear: Option<bool>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_from_selection(&subtype, preset, clear)),
                |refs| refs_out(refs).map_err(ViewerError::from),
            )
            .await?
    }

    /// Arm a stamp: the next click on a page places it.
    pub async fn arm_stamp(
        &self,
        source: AnnotationStampSource,
        target_width: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.arm_stamp(source.input(target_width))),
                |()| (),
            )
            .await
    }

    pub async fn disarm_stamp(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.disarm_stamp();
                Ok(())
            })
            .await
    }

    pub async fn has_armed_stamp(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.has_armed_stamp())).await
    }

    /// Place a stamp at a point, without arming.
    pub async fn place_stamp(
        &self,
        source: AnnotationStampSource,
        placement: AnnotationStampPlacement,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = source.input(placement.target_width);
                    Ok(open(c)?.place_stamp(input, placement.into()))
                },
                |r| ref_out(r).map_err(ViewerError::from),
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
                Ok(open(c)?.place_armed_stamp(page.into(), point.into(), display_rotation))
            })
            .await
    }

    /// Ask the file-picker port for a stamp image and place it at `point`.
    /// `false` when the active tool takes no picked file.
    pub async fn request_stamp_at(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.request_stamp_at(page.into(), point.into(), display_rotation))
            })
            .await
    }

    /// Show `tool_id`'s footprint under a page-space point.
    pub async fn hover_ghost_at(
        &self,
        tool_id: String,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.hover_ghost_at(&tool_id, page.into(), point.into(), display_rotation);
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

    pub async fn set_placement_preview(
        &self,
        tool_id: String,
        page: PageRef,
        bounds: Rect,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.set_placement_preview(&tool_id, page.into(), bounds.into());
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
    ) -> Result<bool, ViewerError> {
        self.cap
            .call(move |c| Ok(open(c)?.place_at(page.into(), point.into(), display_rotation)))
            .await
    }

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
                    Ok(open(c)?.create_attachment(page.into(), point.into(), source))
                },
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    /// An attachment's embedded bytes.
    pub async fn read_attachment(
        &self,
        annotation_ref: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.read_attachment(annotation_ref.into())),
                |file| {
                    let data = file.get("data").cloned().unwrap_or(file);
                    Bytes::from_json(&data).map(|b| b.0).ok_or_else(|| {
                        ViewerError::from(super::invalid("an attachment read answered no bytes"))
                    })
                },
            )
            .await?
    }
}
