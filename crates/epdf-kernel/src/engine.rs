//! The plugins' engine: `engine-core`'s `Engine` and `DocumentHandle`
//! contract, as one JSON envelope per operation.
//!
//! A call names its service path the way the TypeScript spells the member
//! chain: `metadata.read`, `pages.list`, `render.policy`, and for a page
//! handle `page.render.render` with the page's object number alongside.
//! Arguments and results are JSON; binary members travel as
//! `{"$bytes": "<base64>"}`, which [`crate::bytes::Bytes`] reads and writes.
//!
//! This is the same move `crates/epdf-engine/src/host.rs` made for the worker
//! wire, and for the same reason: three implementations (the local engine,
//! the cloud engine, the conformance fake) sit behind one contract, and a
//! typed trait per service would be sixty signatures checked against nothing.
//! Each plugin wraps the calls it makes in typed functions over its own DTOs.

use std::rc::Rc;

use serde_json::Value;

use crate::error::Error;
use crate::rt::Promise;
use crate::store::Unsubscribe;

/// An engine call in flight: an `AbortablePromise`.
pub type EngineCall<T> = Promise<T>;

pub trait Engine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>>;
    fn destroy(&self) -> EngineCall<()>;
}

pub trait DocumentSecurity {
    fn allows(&self, capability: &str) -> bool;
    fn allows_annotation_create(&self) -> bool;
    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool;
    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool;
    fn effective_scope(&self) -> Vec<String>;
    fn identity(&self) -> Value;
    /// `DocumentSecurityState`.
    fn current(&self) -> Value;
    /// `PasswordPrompt`: `{state: 'none' | 'required' | ...}`.
    fn password_prompt(&self) -> Value;
    fn unlock(&self, input: Value) -> EngineCall<Value>;
}

pub trait DocumentHandle {
    fn id(&self) -> String;
    /// `DocumentCapabilities`.
    fn capabilities(&self) -> Value;
    fn security(&self) -> Rc<dyn DocumentSecurity>;
    /// Whether an optional member exists: `render`, `actions`,
    /// `attachments`, `pieceInfo`, `redaction`, `signatures`, `fonts`,
    /// `version`, `downloadLayer`, `page.pieceInfo`, `page.measure`, and
    /// the members every shipped engine has but the
    /// contract leaves optional (`forms.applyEffects`, `pages.flatten`,
    /// `pages.extract`). The TypeScript feature-detects with
    /// `doc.render !== undefined`.
    fn has(&self, member: &str) -> bool;
    /// An asynchronous service call on the document, or on one of its pages
    /// when `pon` is given.
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value>;
    /// `page(ref)` for the page whose `PageRef` names object number `pon`:
    /// the page's display index, or `EngineError(NotFound)`.
    fn page_index(&self, pon: u32) -> Result<u32, Error>;
    /// `events.subscribe(listener)`.
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe;
    /// `events.lastServerId()`.
    fn last_server_id(&self) -> Option<String> {
        None
    }
}

impl dyn DocumentHandle {
    /// A typed call, awaited: `await doc.metadata.read()`. Costs the one turn
    /// the TypeScript `await` does; deserializing is synchronous.
    pub fn request<R: serde::de::DeserializeOwned + 'static>(
        &self,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
    ) -> impl std::future::Future<Output = Result<R, Error>> + 'static {
        let call = self.call(path, pon, args);
        async move { decode(call.await?) }
    }
}

/// Read an engine answer into a plugin's DTO.
pub fn decode<R: serde::de::DeserializeOwned>(value: Value) -> Result<R, Error> {
    serde_json::from_value(value).map_err(|e| Error::new(format!("[engine] malformed answer: {e}")))
}
