//! App-facing document controls, page registry and presentation settings.

use crate::host_table::HostTable;
use crate::{
    DocumentInfo, OpenDocumentOptions, PageInfo, ViewerHost, ViewerSettings, ViewerSettingsField,
    ViewerSettingsPatch,
};
use epdf_ffi::{CancelToken, PageRef, ViewerError};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, OpenOptions, OpenSource, Promise};
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[uniffi::export]
impl ViewerHost {
    /// Restores an editing-layer artifact over the original PDF base. The
    /// engine validates the pair; cancellation closes its loading tab.
    pub async fn open_layer_document(
        &self,
        base: Vec<u8>,
        artifact: Vec<u8>,
        options: OpenDocumentOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<DocumentInfo, ViewerError> {
        self.call_promise_cancellable(cancel, move |s| {
            let signal = AbortSignal::new();
            let OpenDocumentOptions { name, password, activate } = options;
            let mut engine = Map::new();
            if let Some(password) = password {
                engine.insert("password".into(), password.into());
            }
            let documents = s.kernel.documents();
            let opened = documents.open_with_signal(
                byte_source(base, Some(artifact)),
                OpenOptions { name, activate: Some(activate), engine },
                signal.clone(),
            );
            Ok(opened
                .map(move |value| opened_info(&documents, &value))
                .with_abort(move |reason| signal.abort(Error::abort(reason))))
        })
        .await
    }

    /// Opens under a loading tab. Cancelling closes that tab and aborts fetch.
    pub async fn open_url(
        &self,
        url: String,
        options: OpenDocumentOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<DocumentInfo, ViewerError> {
        self.call_promise_cancellable(cancel, move |s| {
            let signal = AbortSignal::new();
            let OpenDocumentOptions { name, password, activate } = options;
            let mut engine = Map::new();
            if let Some(password) = password {
                engine.insert("password".into(), password.into());
            }
            let documents = s.kernel.documents();
            let opened = documents.open_with_signal(
                OpenSource::Input(json!({"kind":"url", "url":url})),
                OpenOptions { name, activate: Some(activate), engine },
                signal.clone(),
            );
            Ok(opened
                .map(move |value| opened_info(&documents, &value))
                .with_abort(move |reason| signal.abort(Error::abort(reason))))
        })
        .await
    }

    /// Re-fetches a failed source in its existing tab without activating it.
    /// Close the loading tab to cancel; retry has no separate caller signal.
    pub async fn retry_document(&self, document_id: String) -> Result<DocumentInfo, ViewerError> {
        self.call_promise(
            move |s| {
                let documents = s.kernel.documents();
                Ok(documents.retry(&document_id).map(move |value| opened_info(&documents, &value)))
            },
            |value| value,
        )
        .await
    }

    pub async fn rename_document(
        &self,
        document_id: String,
        name: String,
    ) -> Result<(), ViewerError> {
        self.call(move |s| {
            s.kernel.documents().rename(&document_id, &name);
            Ok(())
        })
        .await
    }
    pub async fn close_all_documents(&self) -> Result<(), ViewerError> {
        self.call_promise(|s| Ok(s.kernel.documents().close_all()), |()| ()).await
    }
    /// Includes loading, locked and failed tabs. None selects the active tab.
    pub async fn document(
        &self,
        document_id: Option<String>,
    ) -> Result<Option<DocumentInfo>, ViewerError> {
        self.call(move |s| {
            Ok(s.kernel.documents().get_document(document_id.as_deref()).map(Into::into))
        })
        .await
    }
    pub async fn has_document(&self, document_id: String) -> Result<bool, ViewerError> {
        self.call(move |s| Ok(s.kernel.documents().has(&document_id))).await
    }
    pub async fn document_count(&self) -> Result<u64, ViewerError> {
        self.call(|s| Ok(s.kernel.documents().get_count() as u64)).await
    }
    pub async fn active_document(&self) -> Result<Option<DocumentInfo>, ViewerError> {
        self.call(|s| Ok(s.kernel.documents().get_active().map(Into::into))).await
    }
    pub async fn active_document_id(&self) -> Result<Option<String>, ViewerError> {
        self.call(|s| Ok(s.kernel.documents().get_active_id())).await
    }
    pub async fn set_active_document(&self, document_id: String) -> Result<(), ViewerError> {
        self.call(move |s| {
            s.kernel.documents().set_active(&document_id);
            Ok(())
        })
        .await
    }
    pub async fn document_order(&self) -> Result<Vec<String>, ViewerError> {
        self.call(|s| Ok(s.kernel.documents().get_order())).await
    }
    pub async fn set_document_order(&self, document_ids: Vec<String>) -> Result<(), ViewerError> {
        self.call(move |s| s.kernel.documents().set_order(&document_ids)).await
    }
    pub async fn move_document(
        &self,
        document_id: String,
        to_index: i64,
    ) -> Result<(), ViewerError> {
        self.call(move |s| {
            s.kernel.documents().move_to(&document_id, to_index);
            Ok(())
        })
        .await
    }
    pub async fn swap_documents(&self, first: String, second: String) -> Result<(), ViewerError> {
        self.call(move |s| {
            s.kernel.documents().swap(&first, &second);
            Ok(())
        })
        .await
    }
    /// Missing and closed documents/pages answer None, as the kernel does.
    pub async fn page(
        &self,
        page: PageRef,
        document_id: Option<String>,
    ) -> Result<Option<PageInfo>, ViewerError> {
        self.call(move |s| {
            s.kernel
                .documents()
                .get_page(page.into(), document_id.as_deref())
                .as_ref()
                .map(epdf_ffi::page::decode_page)
                .transpose()
        })
        .await
    }
    pub async fn page_at(
        &self,
        index: i64,
        document_id: Option<String>,
    ) -> Result<Option<PageInfo>, ViewerError> {
        self.call(move |s| {
            s.kernel
                .documents()
                .get_page_at(index, document_id.as_deref())
                .as_ref()
                .map(epdf_ffi::page::decode_page)
                .transpose()
        })
        .await
    }
    /// -1 when the page or document is absent.
    pub async fn page_index(
        &self,
        page: PageRef,
        document_id: Option<String>,
    ) -> Result<i64, ViewerError> {
        self.call(move |s| {
            Ok(s.kernel.documents().get_page_index(page.into(), document_id.as_deref()))
        })
        .await
    }
    /// Page-list revision, or -1 when the document is absent.
    pub async fn page_revision(&self, document_id: Option<String>) -> Result<i64, ViewerError> {
        self.call(move |s| Ok(s.kernel.documents().get_revision(document_id.as_deref()))).await
    }
    pub async fn allows(
        &self,
        capability: String,
        document_id: Option<String>,
    ) -> Result<bool, ViewerError> {
        self.call(move |s| Ok(s.kernel.documents().allows(&capability, document_id.as_deref())))
            .await
    }
    pub async fn can_download(&self, document_id: Option<String>) -> Result<bool, ViewerError> {
        self.call(move |s| Ok(s.kernel.documents().can_download(document_id.as_deref()))).await
    }
    /// Saves the editing layer after deferred document work settles.
    pub async fn save_layer(
        &self,
        document_id: String,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.call_promise_cancellable(cancel, move |s| {
            let signal = AbortSignal::new();
            let saved = s
                .kernel
                .documents()
                .save_layer_with_signal(Some(&document_id), Some(signal.clone()));
            Ok(Promise::spawn(async move {
                match saved.await {
                    Ok(value) => value
                        .get("$hostBytes")
                        .and_then(Value::as_u64)
                        .and_then(HostTable::take)
                        .ok_or_else(|| {
                            Error::new(format!("[epdf-viewer-host] a layer save answered {value}"))
                        }),
                    Err(error) => Err(to_plugin_error("documents", &error)),
                }
            })
            .with_abort(move |reason| signal.abort(Error::abort(reason))))
        })
        .await
    }
    /// Presentation values only. Engine scope is fixed when this host is created.
    pub async fn viewer_settings(&self) -> Result<ViewerSettings, ViewerError> {
        self.call(|s| {
            let value = s.kernel.get_settings();
            Ok(ViewerSettings {
                accent: value["accent"].as_str().map(str::to_owned),
                page_background: value["page"]["background"].as_str().map(str::to_owned),
                page_shadow: value["page"]["shadow"].as_str().map(str::to_owned),
            })
        })
        .await
    }
    /// Changes are observed through the viewer's state revision stream.
    pub async fn update_viewer_settings(
        &self,
        patch: ViewerSettingsPatch,
    ) -> Result<(), ViewerError> {
        self.call(move |s| {
            let mut patch = patch;
            let mut accent = patch.accent.take().map(Value::String);
            let mut background = patch.page_background.take().map(Value::String);
            let mut shadow = patch.page_shadow.take().map(Value::String);
            for field in patch.clear {
                let slot = match field {
                    ViewerSettingsField::Accent => &mut accent,
                    ViewerSettingsField::PageBackground => &mut background,
                    ViewerSettingsField::PageShadow => &mut shadow,
                };
                if slot.as_ref().is_some_and(|v| !v.is_null()) {
                    return Err(plugin_error(
                        code::INVALID_INPUT,
                        "settings",
                        "a viewer setting cannot be set and cleared together",
                        None,
                        None,
                    ));
                }
                *slot = Some(Value::Null);
            }
            let mut changes = Map::new();
            let mut page = Map::new();
            if let Some(value) = accent {
                changes.insert("accent".into(), value);
            }
            if let Some(value) = background {
                page.insert("background".into(), value);
            }
            if let Some(value) = shadow {
                page.insert("shadow".into(), value);
            }
            if !page.is_empty() {
                changes.insert("page".into(), page.into());
            }
            s.kernel.update_settings(&Value::Object(changes));
            Ok(())
        })
        .await
    }
    pub async fn reset_viewer_settings(&self) -> Result<(), ViewerError> {
        self.call(|s| {
            s.kernel.reset_settings();
            Ok(())
        })
        .await
    }
}

fn opened_info(documents: &epdf_kernel::Documents, value: &Value) -> Result<DocumentInfo, Error> {
    let id = value["document"]["id"].as_str().unwrap_or_default();
    documents
        .get(id)
        .map(Into::into)
        .ok_or_else(|| Error::new(format!("[epdf-viewer-host] {id} opened but is not listed")))
}

/// The kernel keeps raw source bytes until ready, and invokes the thunk again
/// on retry. Each attempt transfers a fresh buffer; a cancelled produced input
/// is released through the kernel's host hold.
pub(crate) fn byte_source(base: Vec<u8>, artifact: Option<Vec<u8>>) -> OpenSource {
    OpenSource::Thunk(std::rc::Rc::new(move |_| {
        let base = HostTable::put(base.clone());
        let mut input = json!({ "$host": base });
        if let Some(artifact) = &artifact {
            input["layer"] = json!({ "$host": HostTable::put(artifact.clone()) });
        }
        Ok(Promise::resolved(input))
    }))
}
