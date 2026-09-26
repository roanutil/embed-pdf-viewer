//! `controller.ts`: the composition root. It builds the plugin's services
//! once and holds them in one [`RedactionCapability`]; every verb and read
//! lives in `read/`, `write/` or `sync/` as an `impl` block on it.

use std::rc::Rc;

use epdf_kernel::{Controller, Error, Unsubscribe};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_search::{SEARCH, SearchCapability};
use epdf_plugin_selection::{SELECTION, SelectionCapability};

use crate::contract::{RedactionAppliedEvent, RedactionPendingChangedEvent};
use crate::contract::{RedactionApplyResult, RedactionConfig};
use crate::model::{RedactionAction, RedactionState};
use crate::services::events::{RedactionEvents, create_events};

pub(crate) struct Core {
    pub cx: Controller<RedactionState, RedactionAction>,
    pub config: RedactionConfig,
    pub events: RedactionEvents,
    /// The required sibling, resolved once (`siblings.annotation`).
    pub annotation: AnnotationCapability,
}

/// `RedactionCapability` (and `RedactionHostCapability`). Cheap to clone;
/// clones share state.
#[derive(Clone)]
pub struct RedactionCapability(pub(crate) Rc<Core>);

/// `createRedactionController(ctx, config)`: the capability; the plugin's
/// `connect` then calls [`RedactionCapability::subscribe_changes`].
pub fn create_redaction_controller(
    cx: Controller<RedactionState, RedactionAction>,
    config: RedactionConfig,
) -> Result<RedactionCapability, Error> {
    let events = create_events(&cx, &config.host);
    let annotation = cx.get(&ANNOTATION)?;
    Ok(RedactionCapability(Rc::new(Core { cx, config, events, annotation })))
}

impl RedactionCapability {
    // ── siblings (`services/siblings.ts`) ─────────────────────────────────────

    pub(crate) fn annotation(&self) -> &AnnotationCapability {
        &self.0.annotation
    }

    pub(crate) fn selection(&self) -> Result<Option<SelectionCapability>, Error> {
        self.0.cx.try_get(&SELECTION)
    }

    pub(crate) fn search(&self) -> Result<Option<SearchCapability>, Error> {
        self.0.cx.try_get(&SEARCH)
    }

    // ── the controller's own members ──────────────────────────────────────────

    /// Marks are annotations: marking authority is annotation create
    /// authority (the annotation plugin's own twin, not a re-derivation).
    pub fn can_mark(&self) -> bool {
        self.annotation().can_create()
    }

    /// An apply is in flight.
    pub fn is_applying(&self) -> bool {
        self.state().applying
    }

    /// The last apply result seen (own or remote).
    pub fn get_last_result(&self) -> Option<RedactionApplyResult> {
        self.state().last_result.clone()
    }

    /// After any confirmed apply, own or a remote collaborator's.
    pub fn on_applied(
        &self,
        listener: impl Fn(&RedactionAppliedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.applied.on(listener)
    }

    /// Marks were created, changed or removed.
    pub fn on_pending_changed(
        &self,
        listener: impl Fn(&RedactionPendingChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.pending_changed.on(listener)
    }
}
