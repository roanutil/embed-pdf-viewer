use std::sync::Arc;

use epdf_ffi::{
    AnnotationFlagsPatch, AnnotationPropKey, AnnotationProps, AnnotationPropsPatch, AnnotationRef,
    CancelToken, KernelBound, PageRef, Rect, ViewerError,
};

use super::tools::AnnotationPropSpec;
use super::{AnnotationBatchResult, decode, open, refs_out};
use crate::AnnotationCapability;

/// The selected annotations: what to select, and what to do to all of them.
#[derive(uniffi::Object)]
pub struct AnnotationSelectionHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// What a property panel shows for the selection: the shared values, the
/// properties it can edit, and which keys differ across the selected
/// annotations.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSelectionProps {
    /// `None` when nothing is selected.
    pub values: Option<AnnotationProps>,
    pub editable: Vec<AnnotationPropSpec>,
    pub mixed: Vec<AnnotationPropKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextFormat {
    Bold,
    Italic,
    Underline,
}

impl From<AnnotationTextFormat> for crate::TextFormat {
    fn from(f: AnnotationTextFormat) -> Self {
        match f {
            AnnotationTextFormat::Bold => crate::TextFormat::Bold,
            AnnotationTextFormat::Italic => crate::TextFormat::Italic,
            AnnotationTextFormat::Underline => crate::TextFormat::Underline,
        }
    }
}

/// `rotateSelectionBy`'s `90 | -90`: no other angle can be asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationQuarterTurn {
    /// `90`.
    Clockwise,
    /// `-90`.
    CounterClockwise,
}

impl From<AnnotationQuarterTurn> for crate::QuarterTurn {
    fn from(t: AnnotationQuarterTurn) -> Self {
        match t {
            AnnotationQuarterTurn::Clockwise => crate::QuarterTurn::Plus90,
            AnnotationQuarterTurn::CounterClockwise => crate::QuarterTurn::Minus90,
        }
    }
}

#[uniffi::export]
impl AnnotationSelectionHandle {
    /// `add` keeps the current selection and adds to it.
    pub async fn select(
        &self,
        refs: Vec<AnnotationRef>,
        add: Option<bool>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let refs: Vec<epdf_annotation_core::AnnotationRef> =
                    refs.into_iter().map(Into::into).collect();
                open(c)?.select(&refs, add);
                Ok(())
            })
            .await
    }

    pub async fn select_all(&self, page: Option<PageRef>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.select_all(page.map(Into::into));
                Ok(())
            })
            .await
    }

    pub async fn select_in_rect(
        &self,
        page: PageRef,
        rect: Rect,
        add: Option<bool>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.select_in_rect(page.into(), rect.into(), add);
                Ok(())
            })
            .await
    }

    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.clear_selection();
                Ok(())
            })
            .await
    }

    pub async fn refs(&self) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap.call(|c| refs_out(open(c)?.get_selection())).await
    }

    pub async fn props(&self) -> Result<AnnotationSelectionProps, ViewerError> {
        self.cap
            .call(|c| {
                let read = open(c)?.get_selection_props()?;
                // `values` is `Partial<AnnotationProps>`, as a read's props.
                let values = if read.values.is_empty() {
                    None
                } else {
                    let props: epdf_annotation_core::AnnotationPropsPatch =
                        decode(serde_json::Value::Object(read.values))?;
                    Some(props.into())
                };
                Ok(AnnotationSelectionProps {
                    values,
                    editable: read.specs.into_iter().map(Into::into).collect(),
                    mixed: read.mixed.into_iter().map(Into::into).collect(),
                })
            })
            .await
    }

    /// Patch every selected annotation's props.
    pub async fn update(
        &self,
        patch: AnnotationPropsPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update_selection(patch.try_into()?)),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn update_flags(
        &self,
        patch: AnnotationFlagsPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update_selection_flags(patch.into())),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
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
                |c| Ok(open(c)?.delete_selection()),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn rotate_by(
        &self,
        turn: AnnotationQuarterTurn,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.rotate_selection_by(turn.into())),
                |()| (),
            )
            .await
    }

    pub async fn reset_rotation(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.reset_selection_rotation()), |()| ())
            .await
    }

    pub async fn toggle_text_format(
        &self,
        format: AnnotationTextFormat,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.toggle_text_format(format.into())),
                |()| (),
            )
            .await
    }

    pub async fn group(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.group()), |()| ()).await
    }

    pub async fn ungroup(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.ungroup()), |()| ()).await
    }

    pub async fn can_group(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_group())).await
    }

    pub async fn can_ungroup(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_ungroup())).await
    }
}
