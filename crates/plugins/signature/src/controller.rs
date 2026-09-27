//! `controller.ts`: the composition root. It builds the plugin's services
//! once and assembles the capability from the areas.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes their API objects. Here the areas are `impl`
//! blocks on one [`SignatureCapability`], one per TypeScript file, and
//! their private state sits in [`Core`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::TimerId;
use epdf_kernel::{Controller, Error};
use epdf_plugin_form::FormCapability;
use serde_json::Value;

use crate::contract::SignatureConfig;
use crate::model::{SignatureAction, SignatureState};
use crate::services::events::SignatureEvents;

/// The plugin's private state, by the service or area that owns it.
pub(crate) struct Core {
    pub(crate) cx: Controller<SignatureState, SignatureAction>,
    pub(crate) config: SignatureConfig,
    // services/events
    pub(crate) events: SignatureEvents,
    // services/siblings: the form plugin, required.
    pub(crate) form: FormCapability,
    // sync/hydration
    pub(crate) revalidate_timer: Cell<Option<TimerId>>,
    // write/sign: two-phase signings this session prepared.
    pub(crate) prepared: RefCell<HashMap<String, Value>>,
}

/// `SignatureCapability`, which the host lens equals today. Cheap to clone;
/// clones share state.
#[derive(Clone)]
pub struct SignatureCapability(pub(crate) Rc<Core>);

/// `SignatureHostCapability`.
pub type SignatureHostCapability = SignatureCapability;

fn build(
    cx: Controller<SignatureState, SignatureAction>,
    config: SignatureConfig,
) -> Result<SignatureCapability, Error> {
    // `createServices`: the siblings first, then the event hooks.
    let form = crate::services::siblings::resolve_form(&cx)?;
    let events = SignatureEvents::new(&cx, config.host.console_error.clone());
    let api = SignatureCapability(Rc::new(Core {
        cx: cx.clone(),
        config,
        events,
        form,
        revalidate_timer: Cell::new(None),
        prepared: RefCell::default(),
    }));
    // `createHydration`'s cleanup.
    let inner = api.0.clone();
    cx.defer(Box::new(move || {
        if let Some(timer) = inner.revalidate_timer.take() {
            epdf_kernel::rt::clear_timer(timer);
        }
        Ok(None)
    }));
    Ok(api)
}

/// `createSignatureController(ctx, config)`: the api and its `connect`,
/// which subscribes to the document's events and reads the snapshot (then
/// judges it, when anything is signed).
pub fn create_signature_controller(
    cx: Controller<SignatureState, SignatureAction>,
    config: SignatureConfig,
) -> Result<(SignatureCapability, ConnectFn), Error> {
    let api = build(cx, config)?;
    let connecting = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        connecting.subscribe_document_events()?;
        connecting.initial_read();
        Ok(())
    });
    Ok((api, connect))
}

/// `createSignatureCapability(ctx, config)`: the capability with its
/// document-event subscription and no initial read.
pub fn create_signature_capability(
    cx: Controller<SignatureState, SignatureAction>,
    config: SignatureConfig,
) -> Result<SignatureCapability, Error> {
    let api = build(cx, config)?;
    api.subscribe_document_events()?;
    Ok(api)
}

impl SignatureCapability {
    pub(crate) fn cx(&self) -> &Controller<SignatureState, SignatureAction> {
        &self.0.cx
    }

    pub(crate) fn config(&self) -> &SignatureConfig {
        &self.0.config
    }

    pub(crate) fn dispatch(&self, action: SignatureAction) {
        self.0.cx.dispatch(action);
    }

    /// `globalThis.console?.error(message, error)`.
    pub(crate) fn console_error(&self, message: &str, error: &Error) {
        if let Some(sink) = &self.0.config.host.console_error {
            sink(message, error);
        }
    }

    /// `void hydration.refresh().then((s) => signed ? hydration.validate() :
    /// null).catch(report)`.
    fn initial_read(&self) {
        let this = self.clone();
        let refresh = self.refresh();
        epdf_kernel::rt::spawn(async move {
            let outcome: Result<(), Error> = async {
                let snapshot = refresh.await?;
                if crate::read::signatures::any_signed(snapshot.as_ref()) {
                    crate::promise::adopt(this.validate(Default::default())).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = outcome {
                epdf_kernel::rt::tick().await;
                this.console_error("[signature] initial read failed:", &error);
            }
        });
    }
}
