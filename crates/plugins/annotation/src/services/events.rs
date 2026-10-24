//! `services/events.ts`: the plugin's events. Record events (created,
//! updated, deleted, reordered, resynced) fire when the records mirror
//! confirms a change (`sync/confirmed.rs`); selection, draft, editing and
//! hover events are derived from state changes in the store; `writeFailed`
//! fires when the engine refuses a change the user made; the rest fire where
//! their operation completes. `records_changed` is internal. Payloads are
//! the TypeScript's event objects, as JSON.

use epdf_kernel::ControllerContext;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::mirror::MirrorChange;
use serde_json::Value;

pub struct Events {
    pub created: EventHookSource<Value>,
    pub updated: EventHookSource<Value>,
    pub deleted: EventHookSource<Value>,
    pub reordered: EventHookSource<Value>,
    pub resynced: EventHookSource<Value>,
    pub selection_changed: EventHookSource<Value>,
    pub draft_changed: EventHookSource<Value>,
    pub editing_changed: EventHookSource<Value>,
    pub hover_changed: EventHookSource<Value>,
    pub tool_defaults_changed: EventHookSource<Value>,
    pub thread_changed: EventHookSource<Value>,
    pub draft_captured: EventHookSource<Value>,
    pub write_failed: EventHookSource<Value>,
    pub records_changed: EventHookSource<MirrorChange>,
}

impl Events {
    /// `createAnnotationEvents(ctx)`.
    pub fn new(cx: &ControllerContext) -> Events {
        Events {
            created: cx.event_source(),
            updated: cx.event_source(),
            deleted: cx.event_source(),
            reordered: cx.event_source(),
            resynced: cx.event_source(),
            selection_changed: cx.event_source(),
            draft_changed: cx.event_source(),
            editing_changed: cx.event_source(),
            hover_changed: cx.event_source(),
            tool_defaults_changed: cx.event_source(),
            thread_changed: cx.event_source(),
            draft_captured: cx.event_source(),
            write_failed: cx.event_source(),
            records_changed: cx.event_source(),
        }
    }
}

// ── the capability's event hooks and mirror verbs ──────────────────────────

// Each `on_*` is the TypeScript's `EventHook<T>`: the listener gets the
// event object as JSON, an `Err` is reported as a throwing listener is, and
// the return unsubscribes.

macro_rules! hooks {
    ($($(#[$doc:meta])* $name:ident => $source:ident;)*) => {
        impl crate::controller::Core {
            $(
                $(#[$doc])*
                pub fn $name(
                    &self,
                    listener: impl Fn(&Value) -> Result<(), epdf_kernel::Error> + 'static,
                ) -> epdf_kernel::Unsubscribe {
                    self.events.$source.on(listener)
                }
            )*
        }
    };
}

hooks! {
    /// `onCreated`: an annotation was added, once the engine saved it.
    on_created => created;
    /// `onUpdated`: an annotation was changed, once the engine saved it.
    on_updated => updated;
    /// `onDeleted`: annotations were deleted, one plus its popup and replies.
    on_deleted => deleted;
    /// `onReordered`: the drawing order on a page changed.
    on_reordered => reordered;
    /// `onResynced`: annotations were read again, as after loading a document.
    on_resynced => resynced;
    /// `onWriteFailed`: the engine refused a change the user made.
    on_write_failed => write_failed;
    /// `onSelectionChanged`.
    on_selection_changed => selection_changed;
    /// `onDraftChanged`: a polygon or polyline is being drawn, or ended.
    on_draft_changed => draft_changed;
    /// `onEditingChanged`: typing in a text box started or ended.
    on_editing_changed => editing_changed;
    /// `onHoverChanged`: the pointer moved onto an annotation, or off it.
    on_hover_changed => hover_changed;
    /// `tools.onDefaultsChanged`: a tool's defaults changed.
    on_tool_defaults_changed => tool_defaults_changed;
    /// `comments.onThreadChanged`: a comment thread changed.
    on_thread_changed => thread_changed;
    /// `onDraftCaptured` (host lens): a capture tool's line, for the
    /// measurement plugin's calibration.
    on_draft_captured => draft_captured;
}

impl crate::controller::Core {
    /// `getStatus()`: the records mirror's status.
    pub fn get_status(&self) -> epdf_kernel::mirror::ResourceStatus {
        self.records.get_status()
    }

    /// `refresh(options)`: read every annotation again.
    pub fn refresh(
        &self,
        signal: Option<epdf_kernel::signal::AbortSignal>,
    ) -> epdf_kernel::rt::Promise<()> {
        self.cancellable(signal, self.records.refresh())
    }

    /// `whenSynced()` (host lens): every load and page reload in flight has settled.
    pub fn when_synced(&self) -> epdf_kernel::rt::Promise<()> {
        self.records.settled()
    }
}
