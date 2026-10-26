//! `controller.ts`: the metadata controller. Both halves of the Info dict
//! are kernel mirrors: each changes only when a load lands or its confirmed
//! document event arrives (`metadata.updated`, `metadata.customUpdated`),
//! whoever caused the edit, and that is also the one place its `onUpdated`
//! and `onResynced` fire. Nothing needs them when a document opens, so they
//! load on demand: the first `ensure_loaded()` reads them.
//!
//! Lifetime, cancellation and error mapping are the kernel's: the mirrors
//! read through the guarded handle and end with the instance, and the verbs
//! race the caller's signal through `ctx.cancellable`.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::mirror::{Fold, Load, Loaded, Mirror, MirrorChange, MirrorSpec, ResourceStatus};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{ControllerContext, Error, Unsubscribe, is_undefined, undefined};
use serde_json::Value;

use crate::contract::*;
use crate::model::changed_keys;

pub(crate) const CAPABILITY: &str = "metadata";
const METADATA_MODIFY: &str = "doc.metadata.modify";

/// `MirrorSpec.fold`.
type FoldFn = Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>>;

/// JavaScript truthiness: `if (!next) return;`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => !is_undefined(value),
    }
}

/// `previous` as the events carry it: `null` (no snapshot yet) is `None`.
fn previous_of(previous: &Rc<Value>) -> Option<Rc<Value>> {
    (!previous.is_null()).then(|| previous.clone())
}

/// `event.<member>`, `undefined` when it is missing.
fn member(event: &Value, name: &str) -> Value {
    event.get(name).cloned().unwrap_or_else(undefined)
}

/// One half of the Info dict as a mirror: `load` reads it through the
/// guarded handle at `path`, a `kind` event replaces it with its `member`.
fn mirror_spec(
    name: &str,
    path: &'static str,
    kind: &'static str,
    member_name: &'static str,
) -> MirrorSpec {
    // `load: async (doc) => ({ value: await doc.<path>() })`
    let load: Load = Rc::new(move |doc, _signal| {
        let call = doc.map(|doc| doc.call(path, None, Vec::new()));
        Promise::spawn(async move {
            let Some(call) = call else {
                return Err(Error::new("[metadata] the mirror has no document"));
            };
            Ok(Loaded { value: call.await?, cursor: None })
        })
    });
    // `fold: (value, event) => (event.type === kind ? event.<member> : value)`
    let fold: FoldFn = Rc::new(move |value, event| {
        Ok(Fold::Value(if event.get("type").and_then(Value::as_str) == Some(kind) {
            Rc::new(member(event, member_name))
        } else {
            value
        }))
    });
    let mut spec = MirrorSpec::new(name, Value::Null, load, fold);
    spec.on_demand = true;
    spec
}

/// `Promise.all([a, b]).then(() => undefined)` over two plain promises:
/// each element reacts one turn after it settles, the last one resolves the
/// whole, and the `.then` takes one turn more. The first rejection wins.
fn join(a: Promise<()>, b: Promise<()>) -> Promise<()> {
    let (all, resolver) = Promise::pending();
    let remaining = Rc::new(Cell::new(2));
    for promise in [a, b] {
        let (remaining, resolver) = (remaining.clone(), resolver.clone());
        rt::spawn(async move {
            match promise.reaction().await {
                Ok(()) => {
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        resolver.resolve(());
                    }
                }
                Err(error) => resolver.reject(error),
            }
        });
    }
    all.map(Ok)
}

pub(crate) struct Core {
    cx: ControllerContext,
    metadata: Mirror,
    custom: Mirror,
    updated: EventHookSource<MetadataUpdatedEvent>,
    resynced: EventHookSource<MetadataResyncedEvent>,
    custom_updated: EventHookSource<CustomMetadataUpdatedEvent>,
    custom_resynced: EventHookSource<CustomMetadataResyncedEvent>,
}

/// `MetadataCapability`: the api `createMetadataController` returns.
#[derive(Clone)]
pub struct MetadataCapability(Rc<Core>);

/// `CustomMetadataCapability`: the Info dict's custom keys, the same reads
/// and verbs as the standard fields. Changing them needs the same
/// permission, so [`MetadataCapability::can_update`] answers for both.
#[derive(Clone)]
pub struct CustomMetadataCapability(Rc<Core>);

impl Core {
    /// `refuseWrite(verb, options)`: both writes refuse the same way before
    /// reaching the engine. The verbs are async, so a refusal rejects, and
    /// nothing changes.
    fn refuse_write(&self, verb: &str, options: &OperationOptions) -> Result<(), Error> {
        self.cx.assert_allowed(METADATA_MODIFY, verb)?;
        if options.signal.as_ref().is_some_and(|signal| signal.aborted()) {
            return Err(plugin_error(
                code::OPERATION_CANCELLED,
                CAPABILITY,
                format!("{verb} was cancelled"),
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `update: async (patch, options) => { refuseWrite(verb, options); const
    /// result = await ctx.cancellable(options?.signal, ctx.doc.<path>(patch));
    /// return { <member>: result.<member> }; }`
    fn write(
        self: &Rc<Self>,
        verb: &'static str,
        path: &'static str,
        member_name: &'static str,
        patch: Value,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse_write(verb, &options)?;
            let doc = this.cx.doc()?;
            let call = doc.call(path, None, vec![patch]);
            let result = cancellable(CAPABILITY, options.signal, call).await?;
            let mut out = serde_json::Map::new();
            out.insert(member_name.into(), member(&result, member_name));
            Ok(Value::Object(out))
        })
    }
}

impl MetadataCapability {
    /// The current standard fields, reference-stable until they change;
    /// `None` until the first read lands or while reading is forbidden (see
    /// [`get_status`](Self::get_status)).
    pub fn get_snapshot(&self) -> Option<Rc<DocumentMetadata>> {
        previous_of(&self.0.metadata.get())
    }

    /// Load state: `idle` until [`ensure_loaded`](Self::ensure_loaded)
    /// asks, then `loading` → `ready`, or `forbidden` / `error`.
    pub fn get_status(&self) -> ResourceStatus {
        self.0.metadata.get_status()
    }

    /// Read the standard fields and the custom keys from the document,
    /// unless they are loaded or loading. Resolves once both have landed
    /// (their `on_resynced` has fired); a later call resolves at once.
    /// Rejects with the first read's failure (`permission-denied` when
    /// reading is refused), `operation-cancelled` or `instance-closed`;
    /// after a failure it reads again.
    pub fn ensure_loaded(&self, options: OperationOptions) -> Promise<()> {
        let joined = join(self.0.metadata.ensure_loaded(), self.0.custom.ensure_loaded());
        cancellable(CAPABILITY, options.signal, joined)
    }

    /// Whether this session may change the Info dict
    /// (`doc.metadata.modify`): [`update`](Self::update) and the custom
    /// keys' `update`. The engine enforces the same permission.
    pub fn can_update(&self) -> bool {
        self.0.cx.allows(METADATA_MODIFY)
    }

    /// Change the standard fields: a field left out stays, `null` clears
    /// it, a value sets it. Resolves `{ metadata }`, the fields after the
    /// change, as the engine answered them; by then the confirmed event has
    /// set the snapshot and fired `on_updated`. Rejects with
    /// `permission-denied`, `instance-closed` or `operation-cancelled`.
    pub fn update(&self, patch: MetadataPatch, options: OperationOptions) -> Promise<Value> {
        self.0.write("metadata.update", "metadata.update", "metadata", patch, options)
    }

    /// Re-read the standard fields from the engine. Rarely needed: the
    /// event stream keeps the snapshot fresh. During a read in flight this
    /// joins it and queues one more read.
    pub fn refresh(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.metadata.refresh())
    }

    /// A confirmed change, whoever caused it. Fires after the snapshot
    /// changed.
    pub fn on_updated(
        &self,
        listener: impl Fn(&MetadataUpdatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.updated.on(listener)
    }

    /// The standard fields were loaded or reloaded from the engine.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&MetadataResyncedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.resynced.on(listener)
    }

    /// The Info dict's other keys.
    pub fn custom(&self) -> CustomMetadataCapability {
        CustomMetadataCapability(self.0.clone())
    }

    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript, where a closed capability is unreachable; a foreign handle
    /// outlives the close, checks this before every call, and ends its event
    /// streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
}

impl CustomMetadataCapability {
    /// The current keys, reference-stable until they change; `None` until
    /// the first read lands.
    pub fn get_snapshot(&self) -> Option<Rc<CustomMetadata>> {
        previous_of(&self.0.custom.get())
    }

    /// Load state: `idle` until `ensure_loaded()` asks, then `loading` →
    /// `ready`, or `forbidden` / `error`.
    pub fn get_status(&self) -> ResourceStatus {
        self.0.custom.get_status()
    }

    /// Set and remove keys: a key left out stays, `null` removes it, a
    /// string sets it. Resolves `{ custom }`, the keys after the change; by
    /// then the snapshot shows it and `on_updated` has fired. Rejects with
    /// `permission-denied`, `invalid-input`, `instance-closed` or
    /// `operation-cancelled`.
    pub fn update(&self, patch: CustomMetadataPatch, options: OperationOptions) -> Promise<Value> {
        self.0.write("metadata.custom.update", "metadata.custom.update", "custom", patch, options)
    }

    /// Re-read the custom keys from the engine.
    pub fn refresh(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.custom.refresh())
    }

    /// A confirmed change, whoever caused it. Fires after the snapshot
    /// changed.
    pub fn on_updated(
        &self,
        listener: impl Fn(&CustomMetadataUpdatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.custom_updated.on(listener)
    }

    /// The custom keys were loaded or reloaded from the engine.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&CustomMetadataResyncedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.custom_resynced.on(listener)
    }
}

/// `changed` for one mirror: nothing while the value is empty, `resynced`
/// after a load, `updated` after a confirmed event that has an origin.
fn on_change(
    resynced: impl Fn(Rc<Value>) + 'static,
    updated: impl Fn(Rc<Value>, Option<Rc<Value>>, Vec<String>, Value) + 'static,
) -> Rc<dyn Fn(MirrorChange) -> Result<(), Error>> {
    Rc::new(move |change| {
        let next = change.next;
        if !truthy(&next) {
            return Ok(());
        }
        if change.cause == "load" {
            resynced(next);
        } else if let Some(origin) = change.event.as_ref().and_then(|e| e.get("origin")) {
            let keys = changed_keys(previous_of(&change.previous).as_deref(), &next);
            updated(next, previous_of(&change.previous), keys, origin.clone());
        }
        Ok(())
    })
}

/// `createMetadataController(ctx)`: the api; there is no `connect`, since
/// nothing is read until someone asks.
pub fn create_metadata_controller(
    cx: ControllerContext,
) -> Result<(MetadataCapability, Option<ConnectFn>), Error> {
    let updated = cx.event_source::<MetadataUpdatedEvent>();
    let resynced = cx.event_source::<MetadataResyncedEvent>();
    let custom_updated = cx.event_source::<CustomMetadataUpdatedEvent>();
    let custom_resynced = cx.event_source::<CustomMetadataResyncedEvent>();

    let mut spec = mirror_spec("metadata", "metadata.get", "metadata.updated", "metadata");
    let (on_resynced, on_updated) = (resynced.clone(), updated.clone());
    spec.changed = Some(on_change(
        move |metadata| on_resynced.emit(&MetadataResyncedEvent { metadata }),
        move |metadata, previous, changed_keys, origin| {
            on_updated.emit(&MetadataUpdatedEvent { metadata, previous, changed_keys, origin })
        },
    ));
    let metadata = cx.mirror(spec)?;

    let mut spec =
        mirror_spec("customMetadata", "metadata.custom.get", "metadata.customUpdated", "custom");
    let (on_resynced, on_updated) = (custom_resynced.clone(), custom_updated.clone());
    spec.changed = Some(on_change(
        move |custom| on_resynced.emit(&CustomMetadataResyncedEvent { custom }),
        move |custom, previous, changed_keys, origin| {
            on_updated.emit(&CustomMetadataUpdatedEvent { custom, previous, changed_keys, origin })
        },
    ));
    let custom = cx.mirror(spec)?;

    let core = Core { cx, metadata, custom, updated, resynced, custom_updated, custom_resynced };
    Ok((MetadataCapability(Rc::new(core)), None))
}
