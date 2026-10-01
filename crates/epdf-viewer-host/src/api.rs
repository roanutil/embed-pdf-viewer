//! The records, events and callback traits `ViewerHost` exports, and their
//! decoding from the kernel's JSON.
//!
//! The kernel speaks TypeScript shapes: `DocInfo` and `PageLayout` in
//! camelCase, lifecycle payloads as `crates/epdf-kernel/src/kernel.rs` emits
//! them. A shape the kernel did not send is an error, never a guess.

use epdf_ffi::{PageRef, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::types::DocInfo;
use serde_json::Value;

/// One field per plugin as each plugin lands.
#[derive(Debug, Default, uniffi::Record)]
pub struct ViewerConfig {
    /// Install `metadataPlugin()`: the document Info dict, read and edited.
    #[uniffi(default = false)]
    pub metadata: bool,
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

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentInfo {
    pub id: String,
    pub name: Option<String>,
    pub page_count: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageInfo {
    pub page: PageRef,
    pub index: u32,
    pub width: f64,
    pub height: f64,
    /// Degrees clockwise: 0, 90, 180 or 270.
    pub rotation: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum DocumentEvent {
    Opened { document: DocumentInfo },
    OpenFailed { document_id: String, error: String },
    Locked { document_id: String },
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
        DocumentInfo { id: info.id, name: info.name, page_count: info.page_count }
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

fn number(value: &Value, field: &str, what: &str) -> Result<f64, Error> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| malformed(format!("{what} without \"{field}\": {value}")))
}

fn whole(value: &Value, field: &str, what: &str) -> Result<u32, Error> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| malformed(format!("{what} without \"{field}\": {value}")))
}

/// A `PageLayout`: `pageObjectNumber`, `index`, `size` and `rotation`.
pub(crate) fn decode_page(page: &Value) -> Result<PageInfo, Error> {
    let size =
        page.get("size").ok_or_else(|| malformed(format!("a page without \"size\": {page}")))?;
    Ok(PageInfo {
        page: PageRef { page_object_number: whole(page, "pageObjectNumber", "a page")? },
        index: whole(page, "index", "a page")?,
        width: number(size, "width", "a page size")?,
        height: number(size, "height", "a page size")?,
        rotation: whole(page, "rotation", "a page")?,
    })
}

pub(crate) fn decode_pages(pages: &[Value]) -> Result<Vec<PageInfo>, Error> {
    pages.iter().map(decode_page).collect()
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
            Lifecycle::OpenFailed => DocumentEvent::OpenFailed {
                document_id: id()?,
                error: payload
                    .get("error")
                    .map(|error| string(error, "message", "an open failure"))
                    .transpose()?
                    .ok_or_else(|| {
                        malformed(format!("an open failure without \"error\": {payload}"))
                    })?,
            },
            // `{documentId, passwordProvided}`
            Lifecycle::Locked => DocumentEvent::Locked { document_id: id()? },
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
