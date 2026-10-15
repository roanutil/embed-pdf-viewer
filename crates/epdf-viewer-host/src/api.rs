//! The records, events and callback traits `ViewerHost` exports, and their
//! decoding from the kernel's JSON.
//!
//! The kernel speaks TypeScript shapes: `DocInfo` and `PageLayout` in
//! camelCase, lifecycle payloads as `crates/epdf-kernel/src/kernel.rs` emits
//! them. A shape the kernel did not send is an error, never a guess.

use std::sync::Arc;

use epdf_ffi::{PluginErrorInfo, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::types::{DocInfo, DocStatus};
use epdf_plugin_actions::ffi::ActionsConfig;
use epdf_plugin_i18n::ffi::I18nConfig;
use epdf_plugin_interaction::ffi::FeedbackConfig;
use epdf_plugin_render::ffi::RenderConfig;
use epdf_plugin_search::ffi::SearchConfig;
use epdf_plugin_stage::ffi::StageConfig;
use serde_json::Value;

pub use epdf_ffi::PageInfo;
pub(crate) use epdf_ffi::page::decode_pages;

/// One field per plugin as each plugin lands.
#[derive(Debug, Default, uniffi::Record)]
pub struct ViewerConfig {
    /// The capability scope every document opens with, as the engine
    /// parses it (`doc.open`, `doc.annotate.modify`, `pdf.permissions`,
    /// ...). `None` is `["*"]`: everything.
    #[uniffi(default = None)]
    pub scope: Option<Vec<String>>,
    /// Install `metadataPlugin()`: the document Info dict, read and edited.
    #[uniffi(default = false)]
    pub metadata: bool,
    /// Install `searchPlugin(config)`: text search, a session per document.
    #[uniffi(default = None)]
    pub search: Option<SearchConfig>,
    /// Install `renderPlugin(config)`: page rasters as owned pixels.
    #[uniffi(default = None)]
    pub render: Option<RenderConfig>,
    /// Install `interactionPlugin()`: the pointer hub, one per document.
    /// Selection, link, annotation, form and measurement require it.
    #[uniffi(default = false)]
    pub interaction: bool,
    /// Install `feedbackPlugin({ provider })`: haptics for the plugins that
    /// use them. `None` leaves it out, and plugins skip feedback.
    #[uniffi(default = None)]
    pub feedback: Option<Arc<FeedbackConfig>>,
    /// Install `stagePlugin({ config })`: the viewport lens, one per
    /// document. Search reveals hits through it when both are installed.
    #[uniffi(default = None)]
    pub stage: Option<StageConfig>,
    /// Install `selectionPlugin()`: text selection and extraction, one per
    /// document. Requires `interaction`; without it the viewer fails to
    /// build.
    #[uniffi(default = false)]
    pub selection: bool,
    /// Install `linkPlugin()`: link areas and activation, one per document.
    /// Requires `interaction`. With `stage`, a `goto` link reveals its page.
    #[uniffi(default = false)]
    pub link: bool,
    /// Install `annotationPlugin(config)`: read, create, edit and comment
    /// on annotations, one per document. Requires `interaction`; uses
    /// `selection` for markup from selected text and `actions` for the
    /// hover feed when they are installed.
    #[uniffi(default = None)]
    pub annotation: Option<epdf_plugin_annotation::ffi::AnnotationConfig>,
    /// Install `formPlugin(config)`: read, fill and design AcroForm fields.
    /// Needs `interaction`.
    #[uniffi(default = None)]
    pub form: Option<epdf_plugin_form::ffi::FormConfig>,
    /// Install `redactionPlugin(config)`: mark regions, then apply them.
    /// Needs `annotation` (and so `interaction`); marks from search hits
    /// need `search`, marks from the text selection need `selection`.
    #[uniffi(default = None)]
    pub redaction: Option<epdf_plugin_redaction::ffi::RedactionConfig>,
    /// Install `measurementPlugin(config)`: page scale, calibration and
    /// readouts. Needs `interaction` and `annotation`.
    #[uniffi(default = None)]
    pub measurement: Option<epdf_plugin_measurement::ffi::MeasurementConfig>,
    /// Install `stampPlugin(config)`: workspace stamp libraries, opened on
    /// their own asset engine thread. Placing needs `annotation`.
    #[uniffi(default = None)]
    pub stamp: Option<epdf_plugin_stamp::ffi::StampConfig>,
    /// Install `pageEditPlugin()`: rotate, move, delete, insert and extract
    /// pages, addressed by `PageRef`.
    #[uniffi(default = false)]
    pub page_edit: bool,
    /// Install `viewManagerPlugin()`: workspace panes that partition the
    /// open documents (split views and tab strips).
    #[uniffi(default = false)]
    pub view_manager: bool,
    /// Install `shellPlugin()`: which named surfaces and menus are open, per
    /// document. The app still owns their UI.
    #[uniffi(default = false)]
    pub shell: bool,
    /// Install `i18nPlugin(config)`: workspace locale packs and `t()`.
    #[uniffi(default = None)]
    pub i18n: Option<I18nConfig>,
    /// Install `commandsPlugin()`: the workspace command registry. Register
    /// definitions through `ViewerHost::commands()`.
    #[uniffi(default = false)]
    pub commands: bool,
    /// Install `actionsPlugin(config)`: the PDF action engine, a dispatcher
    /// per document.
    #[uniffi(default = None)]
    pub actions: Option<ActionsConfig>,
    /// Install `signaturePlugin(config)`: signature fields, validation and
    /// signing. Needs `form` (and so `interaction`). The signer and the
    /// trust anchors are set with `ViewerHost::set_signature_signer` and
    /// `set_signature_trust`.
    #[uniffi(default = None)]
    pub signature: Option<epdf_plugin_signature::ffi::SignatureConfig>,
}

#[derive(Debug, uniffi::Record)]
pub struct OpenDocumentOptions {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub password: Option<String>,
    #[uniffi(default = true)]
    pub activate: bool,
}

/// How `ViewerHost::save` writes the file: TypeScript's `PdfSaveMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SaveMode {
    /// Append the edits as a new revision after the original bytes, which
    /// stay untouched. Keeps every signature valid.
    Incremental,
    /// Write a fresh file. Smaller, and drops the revision history; a signed
    /// document refuses it.
    Rewrite,
}

impl SaveMode {
    /// The kernel's `{mode}` value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SaveMode::Incremental => "incremental",
            SaveMode::Rewrite => "rewrite",
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentInfo {
    pub id: String,
    pub name: Option<String>,
    pub page_count: u32,
    pub status: DocumentStatus,
    /// `Failed` only: why the open failed, as the kernel's
    /// `toPluginErrorInfo` reports it. A wrong password leads the message
    /// with the engine's `DocPasswordIncorrect`.
    pub error: Option<PluginErrorInfo>,
}

/// Where a tab is, as the kernel's `DocInfo.status` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocumentStatus {
    Loading,
    Locked,
    Ready,
    /// The kernel's `error`: the open failed, and `error` says why.
    Failed,
}

impl From<DocStatus> for DocumentStatus {
    fn from(status: DocStatus) -> Self {
        match status {
            DocStatus::Loading => DocumentStatus::Loading,
            DocStatus::Locked => DocumentStatus::Locked,
            DocStatus::Ready => DocumentStatus::Ready,
            DocStatus::Error => DocumentStatus::Failed,
        }
    }
}

/// `OpenFailed.error` is the kernel's `toPluginError` of the failure, the
/// same record the failed tab's `DocumentInfo.error` holds.
/// `Locked.password_provided` says whether the open carried a password:
/// false means the document wants one, true that the one given did not
/// unlock it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum DocumentEvent {
    Opened { document: DocumentInfo },
    OpenFailed { document_id: String, error: PluginErrorInfo },
    Locked { document_id: String, password_provided: bool },
    Closed { document_id: String },
    ActiveChanged { document_id: Option<String> },
    PagesChanged { document_id: String },
}

#[uniffi::export(with_foreign)]
pub trait DocumentEventListener: Send + Sync {
    fn on_event(&self, event: DocumentEvent);
}

#[uniffi::export(with_foreign)]
pub trait ViewerDelegate: Send + Sync {
    fn state_changed(&self, revision: u64);
    fn report(&self, error: ViewerError);
}

impl From<DocInfo> for DocumentInfo {
    fn from(info: DocInfo) -> Self {
        DocumentInfo {
            id: info.id,
            name: info.name,
            page_count: info.page_count,
            status: info.status.into(),
            error: info.error.map(|e| PluginErrorInfo {
                code: e.code,
                message: e.message,
                capability: e.capability,
            }),
        }
    }
}

fn malformed(what: String) -> Error {
    Error::new(format!("[epdf-viewer-host] the kernel sent {what}"))
}

fn string(value: &Value, field: &str, what: &str) -> Result<String, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| malformed(format!("{what} without \"{field}\": {value}")))
}

/// The six lifecycle hooks, as `Documents::events` names them.
#[derive(Clone, Copy)]
pub(crate) enum Lifecycle {
    Opened,
    OpenFailed,
    Locked,
    Closed,
    ActiveChanged,
    PagesChanged,
}

impl Lifecycle {
    /// Read the payload `kernel.rs` emits on this hook.
    pub(crate) fn decode(self, payload: &Value) -> Result<DocumentEvent, Error> {
        let id = || string(payload, "documentId", "a lifecycle event");
        Ok(match self {
            // `{documentId, info: DocInfo}`
            Lifecycle::Opened => {
                let info = payload.get("info").cloned().unwrap_or(Value::Null);
                let info: DocInfo = serde_json::from_value(info)
                    .map_err(|e| malformed(format!("an opened event without its info ({e})")))?;
                DocumentEvent::Opened { document: info.into() }
            }
            // `{documentId, error: {code, message, capability}}`
            Lifecycle::OpenFailed => {
                let error = payload.get("error").ok_or_else(|| {
                    malformed(format!("an open failure without \"error\": {payload}"))
                })?;
                DocumentEvent::OpenFailed {
                    document_id: id()?,
                    error: PluginErrorInfo {
                        code: string(error, "code", "an open failure")?,
                        message: string(error, "message", "an open failure")?,
                        capability: string(error, "capability", "an open failure")?,
                    },
                }
            }
            // `{documentId, passwordProvided}`
            Lifecycle::Locked => DocumentEvent::Locked {
                document_id: id()?,
                password_provided: payload
                    .get("passwordProvided")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| {
                        malformed(format!("a locked event without \"passwordProvided\": {payload}"))
                    })?,
            },
            // `{documentId}`
            Lifecycle::Closed => DocumentEvent::Closed { document_id: id()? },
            // `{documentId: string | null, previousDocumentId}`
            Lifecycle::ActiveChanged => match payload.get("documentId") {
                Some(Value::Null) => DocumentEvent::ActiveChanged { document_id: None },
                Some(_) => DocumentEvent::ActiveChanged { document_id: Some(id()?) },
                None => {
                    return Err(malformed(format!(
                        "an active change without \"documentId\": {payload}"
                    )));
                }
            },
            // `{documentId, revision, pages}`
            Lifecycle::PagesChanged => DocumentEvent::PagesChanged { document_id: id()? },
        })
    }
}
