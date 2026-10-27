use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, PageRef, Rect, ViewerError};
use serde_json::Value;

use super::tools::{AnnotationProperty, properties_out};
use super::{AnnotationBatchResult, batch_out, object_of, open, page_json, refs_json, refs_out};
use crate::AnnotationCapability;
use crate::write::selection::SelectionPatch;

/// The selected annotations: what to select, and what to do to all of them.
#[derive(uniffi::Object)]
pub struct AnnotationSelectionHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// What a property panel shows for the selection
/// (`selection.getProperties()`): the properties it can edit, their shared
/// values as the engine's field bag, and which keys differ across it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSelectionProps {
    pub properties: Vec<AnnotationProperty>,
    pub values_json: String,
    pub mixed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextFormat {
    Bold,
    Italic,
    Underline,
}

impl AnnotationTextFormat {
    fn as_str(self) -> &'static str {
        match self {
            AnnotationTextFormat::Bold => "bold",
            AnnotationTextFormat::Italic => "italic",
            AnnotationTextFormat::Underline => "underline",
        }
    }
}

#[uniffi::export]
impl AnnotationSelectionHandle {
    /// `selection.set(refs)`, or `selection.add(refs)` with `add`.
    pub async fn select(
        &self,
        refs: Vec<AnnotationRef>,
        add: Option<bool>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let refs = refs_json(&refs)?;
                let c = open(c)?;
                if add == Some(true) {
                    c.selection_add(&refs);
                } else {
                    c.selection_set(&refs);
                }
                Ok(())
            })
            .await
    }

    pub async fn select_all(&self, page: Option<PageRef>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.selection_select_all(page.map(page_json).as_ref());
                Ok(())
            })
            .await
    }

    pub async fn select_in_rect(&self, page: PageRef, rect: Rect) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let rect = epdf_geometry::Rect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                };
                open(c)?.selection_select_in_rect(&page_json(page), rect)
            })
            .await
    }

    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.selection_clear();
                Ok(())
            })
            .await
    }

    pub async fn refs(&self) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call(|c| {
                let selected = open(c)?.list_selected();
                let refs: Value = selected.iter().filter_map(|a| a.get("ref").cloned()).collect();
                refs_out(&refs)
            })
            .await
    }

    pub async fn props(&self) -> Result<AnnotationSelectionProps, ViewerError> {
        self.cap
            .call(|c| {
                let read = open(c)?.selection_properties();
                Ok(AnnotationSelectionProps {
                    properties: properties_out(read.get("properties").unwrap_or(&Value::Null)),
                    values_json: read.get("values").cloned().unwrap_or(Value::Null).to_string(),
                    mixed: read
                        .get("mixed")
                        .and_then(Value::as_array)
                        .map(|m| m.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default(),
                })
            })
            .await
    }

    /// `selection.update(patch)`: the engine's field bag, per member the
    /// fields its kind has.
    pub async fn update(
        &self,
        patch_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let patch = SelectionPatch::Fields(object_of(&patch_json)?);
                    Ok(open(c)?.selection_update(patch, None))
                },
                |b: Value| batch_out(&b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn delete(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.selection_delete(None)),
                |b: Value| batch_out(&b).map_err(ViewerError::from),
            )
            .await?
    }

    /// `selection.rotateBy(degrees)`.
    pub async fn rotate_by(
        &self,
        degrees: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.selection_rotate_by(degrees, None)),
                |()| (),
            )
            .await
    }

    pub async fn reset_rotation(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.selection_reset_rotation(None)),
                |()| (),
            )
            .await
    }

    /// `text.toggleFormat(format)`.
    pub async fn toggle_text_format(
        &self,
        format: AnnotationTextFormat,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.toggle_format(format.as_str(), None)),
                |()| (),
            )
            .await
    }

    pub async fn group(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.selection_group(None)), |()| ())
            .await
    }

    pub async fn ungroup(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.selection_ungroup(None)), |()| ())
            .await
    }

    pub async fn can_group(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.selection_can_group())).await
    }

    pub async fn can_ungroup(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.selection_can_ungroup())).await
    }
}
