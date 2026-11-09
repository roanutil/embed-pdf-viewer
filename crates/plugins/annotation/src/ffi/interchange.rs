use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, PageRef, ViewerError};
use serde_json::{Value, json};

use super::{
    AnnotationHandle, AnnotationRecord, object_of, open, page_json, records_out, ref_out, refs_json,
};

/// Bridge native cancellation into the capability's signal, including chained promises.
pub(super) fn cancellable_operation<T: Clone + 'static>(
    run: impl FnOnce(
        epdf_kernel::signal::AbortSignal,
    ) -> Result<epdf_kernel::Promise<T>, epdf_kernel::Error>,
) -> Result<epdf_kernel::Promise<T>, epdf_kernel::Error> {
    let signal = epdf_kernel::signal::AbortSignal::new();
    let task = run(signal.clone())?;
    Ok(epdf_kernel::cancellable::cancellable("annotation", Some(signal.clone()), task)
        .with_abort_member(move |reason| signal.abort(epdf_kernel::Error::abort(reason))))
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationRefMapping {
    pub from: AnnotationRef,
    pub to: AnnotationRef,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationImportResult {
    pub annotations: Vec<AnnotationRecord>,
    pub ref_map: Vec<AnnotationRefMapping>,
    /// The engine's open dropped-item diagnostics.
    pub dropped_json: String,
}

#[uniffi::export]
impl AnnotationHandle {
    /// A self-contained AnnotationBundle: resource bytes use the existing `$bytes` encoding.
    /// Omitted filters select all; an empty ref or page list selects none.
    pub async fn export(
        &self,
        refs: Option<Vec<AnnotationRef>>,
        pages: Option<Vec<PageRef>>,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let mut selection = json!({});
                    if let Some(refs) = refs {
                        selection["refs"] = Value::Array(refs_json(&refs)?);
                    }
                    if let Some(pages) = pages {
                        selection["pages"] = pages.into_iter().map(page_json).collect();
                    }
                    cancellable_operation(|signal| Ok(open(c)?.export(&selection, Some(signal))))
                },
                |bundle| bundle.to_string(),
            )
            .await
    }

    /// Copy an AnnotationBundle using the engine's AnnotationImportOptions vocabulary.
    pub async fn import(
        &self,
        bundle_json: String,
        options_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationImportResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let c = open(c)?.clone();
                    let bundle = Value::Object(object_of(&bundle_json)?);
                    let options = Value::Object(object_of(&options_json)?);
                    cancellable_operation(|signal| {
                        Ok(c.import(bundle, options, Some(signal)).convert(move |result| {
                            let annotations =
                                result["annotations"].as_array().ok_or_else(|| {
                                    super::invalid("an import result has no annotations")
                                })?;
                            let pairs = result["refMap"]
                                .as_array()
                                .ok_or_else(|| super::invalid("an import result has no refMap"))?;
                            Ok(AnnotationImportResult {
                                annotations: records_out(&c, annotations)?,
                                ref_map: pairs
                                    .iter()
                                    .map(|p| {
                                        Ok(AnnotationRefMapping {
                                            from: ref_out(&p["from"])?,
                                            to: ref_out(&p["to"])?,
                                        })
                                    })
                                    .collect::<Result<_, epdf_kernel::Error>>()?,
                                dropped_json: result["dropped"].to_string(),
                            })
                        }))
                    })
                },
                |result| result,
            )
            .await
    }
}
