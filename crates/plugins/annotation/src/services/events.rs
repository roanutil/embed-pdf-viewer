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
