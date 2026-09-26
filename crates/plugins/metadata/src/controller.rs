//! `controller.ts`: the metadata controller, the reference for the v3
//! authoring pattern. `publish` is the one place the value changes and
//! `onUpdated` fires; `load` reads, then applies only if nothing newer
//! arrived meanwhile. Lifetime, cancellation and error mapping are the
//! kernel's: `doc()` is the guarded handle, `dispatch` the lease commit.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::types::origin_of;
use epdf_kernel::{Controller, Error, Promise};
use serde_json::{Value, json};

use crate::model::{MetadataAction, MetadataState, changed_keys};
use crate::{
    DocumentMetadata, MetadataPatch, MetadataUpdateResult, MetadataUpdatedEvent, ResourceStatus,
};

const METADATA_MODIFY: &str = "doc.metadata.modify";

/// `SYSTEM_ORIGIN`: a change the plugin made on its own (the seed read).
fn system_origin() -> Value {
    json!({ "locality": "local", "trigger": "system", "sessionId": null, "actorId": null })
}

struct Inner {
    cx: Controller<MetadataState, MetadataAction>,
    updated: EventHookSource<MetadataUpdatedEvent>,
    loading: RefCell<Option<Promise<()>>>,
}

/// `MetadataCapability`: the api `createMetadataController` returns.
#[derive(Clone)]
pub struct MetadataCapability(Rc<Inner>);

impl MetadataCapability {
    fn publish(&self, metadata: DocumentMetadata, origin: Value) {
        let previous = self.0.cx.state().value.clone();
        self.0.cx.dispatch(MetadataAction::Set { metadata: metadata.clone() });
        let changed_keys = changed_keys(previous.as_ref(), &metadata);
        self.0.updated.emit(&MetadataUpdatedEvent { metadata, previous, changed_keys, origin });
    }

    fn load(&self) -> Promise<()> {
        if let Some(loading) = self.0.loading.borrow().clone() {
            return loading;
        }
        let this = self.clone();
        let loading = Promise::spawn(async move {
            let seen = this.0.cx.state().revision;
            this.0.cx.dispatch(MetadataAction::Loading);
            let read = async {
                let doc = this.0.cx.doc()?;
                doc.request::<DocumentMetadata>("metadata.read", None, Vec::new()).await
            };
            let outcome = match read.await {
                Ok(metadata) => {
                    // A confirmed value that landed while we were reading is newer than this read.
                    if this.0.cx.state().revision == seen {
                        this.publish(metadata, system_origin());
                    }
                    Ok(())
                }
                Err(error) if error.is_plugin_error(Some(code::INSTANCE_CLOSED)) => Ok(()),
                Err(error) => {
                    let forbidden = error.is_plugin_error(Some(code::PERMISSION_DENIED));
                    this.0.cx.dispatch(MetadataAction::Error { forbidden });
                    Err(error)
                }
            };
            *this.0.loading.borrow_mut() = None;
            outcome
        });
        // `loading ??= (async () => ...)()`: assigned after the body ran to
        // its first await, as the TypeScript assigns.
        *self.0.loading.borrow_mut() = Some(loading.clone());
        loading
    }

    /// The current metadata, `None` until the first read lands or while
    /// reading is forbidden (see [`get_status`](Self::get_status)).
    pub fn get_snapshot(&self) -> Option<DocumentMetadata> {
        self.0.cx.state().value.clone()
    }

    /// Load state: `idle`, `loading`, `ready`, or `forbidden` / `error`.
    pub fn get_status(&self) -> ResourceStatus {
        self.0.cx.state().status
    }

    /// Whether this caller may edit the Info dict (`doc.metadata.modify`).
    /// The engine enforces the same capability; this is the UI mirror of
    /// that guard. Errs only where the TypeScript's `ctx.doc` throws.
    pub fn can_edit(&self) -> Result<bool, Error> {
        Ok(self.0.cx.doc()?.security().allows(METADATA_MODIFY))
    }

    /// Patch the Info dict: an absent key is left, `null` clears, a value
    /// sets. Resolves with the engine result once the confirmed
    /// `metadata.updated` event has set the snapshot and fired `onUpdated`.
    /// Rejects `permission-denied`, `instance-closed` or
    /// `operation-cancelled` (the `signal`, while waiting for that event).
    pub fn update(
        &self,
        patch: MetadataPatch,
        signal: Option<AbortSignal>,
    ) -> Promise<MetadataUpdateResult> {
        let this = self.clone();
        Promise::spawn(async move {
            if !this.can_edit()? {
                return Err(plugin_error(
                    code::PERMISSION_DENIED,
                    "metadata",
                    format!("metadata.update requires {METADATA_MODIFY}"),
                    None,
                    Some(json!({ "required": METADATA_MODIFY })),
                ));
            }
            let seen = this.0.cx.state().revision;
            let doc = this.0.cx.doc()?;
            let result: MetadataUpdateResult =
                doc.request("metadata.update", None, vec![patch]).await?;
            // The confirmed event (see connect) applies the value and fires
            // onUpdated; resolve only once that has happened.
            let cx = this.0.cx.clone();
            this.0.cx.wait_for(move || Ok(cx.state().revision > seen), signal).await?;
            Ok(result)
        })
    }

    /// Re-read from the engine. Rarely needed: the event stream keeps the
    /// snapshot fresh. Shares a read already in flight.
    pub fn refresh(&self) -> Promise<()> {
        self.load()
    }

    /// `onUpdated`: a confirmed change, whoever caused it. Fires after the
    /// snapshot changed.
    pub fn on_updated(
        &self,
        listener: impl Fn(&MetadataUpdatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.updated.on(listener)
    }

    fn connect(&self) -> Result<(), Error> {
        let doc = self.0.cx.doc()?;
        // Every confirmed change: this plugin, another plugin, a script, a remote session.
        let this = self.clone();
        self.0.cx.listen(doc.subscribe(Rc::new(move |event: &Value| {
            if event.get("type").and_then(Value::as_str) == Some("metadata.updated") {
                let metadata = event.get("metadata").cloned().unwrap_or(Value::Null);
                this.publish(metadata, origin_of(event));
            }
        })));
        // Reported through get_status(); nothing to await at boot.
        self.load().ignore();
        Ok(())
    }
}

/// `createMetadataController(ctx)`: the api and its `connect`.
pub fn create_metadata_controller(
    cx: Controller<MetadataState, MetadataAction>,
) -> Result<(MetadataCapability, Option<ConnectFn>), Error> {
    let updated = cx.event_source::<MetadataUpdatedEvent>();
    let api = MetadataCapability(Rc::new(Inner { cx, updated, loading: RefCell::new(None) }));
    let connecting = api.clone();
    let connect: ConnectFn = Rc::new(move || connecting.connect());
    Ok((api, Some(connect)))
}
