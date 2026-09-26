//! `packages/core/main/src/types.ts`: the document registry the kernel keeps
//! in `core`, and the lifecycle action types it emits.
//!
//! Engine DTOs the kernel only carries (a page layout, a render policy) stay
//! JSON here. The kernel never reads inside a page layout; the plugins that
//! do deserialize it into their own types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::error::Error;

pub const CORE_DOCUMENT_ADDED: &str = "@@core/document-added";
pub const CORE_DOCUMENT_REMOVED: &str = "@@core/document-removed";
pub const CORE_ACTIVE_CHANGED: &str = "@@core/active-changed";
pub const CORE_ORDER_CHANGED: &str = "@@core/order-changed";
pub const CORE_DOCUMENT_PAGES_UPDATED: &str = "@@core/document-pages-updated";
pub const CORE_DOCUMENT_OPENING: &str = "@@core/document-opening";
pub const CORE_DOCUMENT_LOCKED: &str = "@@core/document-locked";
pub const CORE_DOCUMENT_OPEN_FAILED: &str = "@@core/document-open-failed";
/// A tab was renamed (`documents.rename`).
pub const CORE_DOCUMENT_RENAMED: &str = "@@core/document-renamed";

/// `CONTINUOUS_RENDER_POLICY` from `engine-core`.
pub fn continuous_render_policy() -> Value {
    serde_json::json!({ "kind": "continuous" })
}

/// What the kernel knows about a ready document.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    pub id: String,
    /// Unique per open of this id; a reopened id gets a new one.
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub page_count: u32,
    /// `PageLayout[]`, as the engine reported it.
    pub pages: Value,
    /// Bumps each time a mutation event replaces the page registry.
    pub revision: u64,
    pub render_policy: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingStatus {
    Loading,
    Locked,
    Error,
}

/// A tab whose document is not ready: loading, locked or failed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMeta {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub status: PendingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_provided: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "canonical_error")]
    pub error: Option<Error>,
}

fn canonical_error<S: Serializer>(error: &Option<Error>, s: S) -> Result<S::Ok, S::Error> {
    match error {
        Some(error) => error.to_canonical().serialize(s),
        None => s.serialize_none(),
    }
}

/// The document registry. `order` spans ready documents and pending slots.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreState {
    pub documents: BTreeMap<String, DocumentMeta>,
    pub pending: BTreeMap<String, PendingMeta>,
    pub order: Vec<String>,
    pub active_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocStatus {
    Loading,
    Locked,
    Ready,
    Error,
}

/// One tab as `documents.list()` reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub status: DocStatus,
    pub page_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_provided: Option<bool>,
    /// `error` only: why the open failed, as `{code, message, capability}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<PluginErrorInfo>,
}

/// `PluginErrorInfo`: the serializable summary of a `PluginError`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginErrorInfo {
    pub code: String,
    pub message: String,
    pub capability: String,
}

impl DocInfo {
    pub fn from_meta(meta: &DocumentMeta) -> Self {
        DocInfo {
            id: meta.id.clone(),
            name: meta.name.clone(),
            status: DocStatus::Ready,
            page_count: meta.page_count,
            password_provided: None,
            error: None,
        }
    }

    pub fn from_pending(meta: &PendingMeta) -> Self {
        DocInfo {
            id: meta.id.clone(),
            name: meta.name.clone(),
            status: match meta.status {
                PendingStatus::Loading => DocStatus::Loading,
                PendingStatus::Locked => DocStatus::Locked,
                PendingStatus::Error => DocStatus::Error,
            },
            page_count: 0,
            password_provided: meta.password_provided,
            error: match (meta.status, &meta.error) {
                (PendingStatus::Error, Some(error)) => serde_json::from_value(
                    crate::error::to_plugin_error("documents", error).plugin_error_info(),
                )
                .ok(),
                _ => None,
            },
        }
    }
}

/// `docInfoEquals`: field-wise, the one definition every adapter's reactive
/// `docs` read keys on.
pub fn doc_info_equals(a: &DocInfo, b: &DocInfo) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.status == b.status
        && a.page_count == b.page_count
        && a.password_provided == b.password_provided
        && a.error.as_ref().map(|e| &e.code) == b.error.as_ref().map(|e| &e.code)
        && a.error.as_ref().map(|e| &e.message) == b.error.as_ref().map(|e| &e.message)
}

/// `docInfoListEquals`: the same list object is equal without a look.
pub fn doc_info_list_equals(a: &[DocInfo], b: &[DocInfo]) -> bool {
    std::ptr::eq(a, b)
        || (a.len() == b.len() && a.iter().zip(b).all(|(x, y)| doc_info_equals(x, y)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginScope {
    Workspace,
    Document,
}

/// `originOf(event)`: where a confirmed engine event came from, in the plugin
/// vocabulary (`ChangeOrigin`). The transport does not carry the trigger, so
/// it is always `unknown`. An origin without a `sessionId` leaves the member
/// out, as the TypeScript's `undefined` does in canonical JSON.
pub fn origin_of(event: &Value) -> Value {
    let origin = event.get("origin").cloned().unwrap_or(Value::Null);
    let remote = origin.get("kind").and_then(Value::as_str) == Some("remote");
    let mut out = serde_json::json!({
        "locality": if remote { "remote" } else { "local" },
        "trigger": "unknown",
        "actorId": origin.get("sub").cloned().filter(|s| !s.is_null()).unwrap_or(Value::Null),
    });
    if let (Some(session), Some(map)) = (origin.get("sessionId"), out.as_object_mut()) {
        map.insert("sessionId".into(), session.clone());
    }
    out
}
