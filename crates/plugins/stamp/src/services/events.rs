//! `services/events.ts`: the capability's events, minted on the instance so
//! they are disposed with it. A listener that throws goes to the kernel's
//! reporter, as every `ctx.events` source's does.

use epdf_kernel::event_hook::EventHookSource;

use crate::contract::*;
use crate::controller::StampContext;

pub struct StampEvents {
    pub library_changed: EventHookSource<StampLibraryChangedEvent>,
    pub library_created: EventHookSource<StampLibraryEvent>,
    pub library_updated: EventHookSource<StampLibraryEvent>,
    pub library_deleted: EventHookSource<StampLibraryEvent>,
    pub asset_created: EventHookSource<StampAssetEvent>,
    pub asset_updated: EventHookSource<StampAssetEvent>,
    pub asset_deleted: EventHookSource<StampAssetEvent>,
    pub arm_changed: EventHookSource<StampArmChangedEvent>,
}

/// `createEvents(ctx)`.
pub fn create_events(cx: &StampContext) -> StampEvents {
    StampEvents {
        library_changed: cx.event_source(),
        library_created: cx.event_source(),
        library_updated: cx.event_source(),
        library_deleted: cx.event_source(),
        asset_created: cx.event_source(),
        asset_updated: cx.event_source(),
        asset_deleted: cx.event_source(),
        arm_changed: cx.event_source(),
    }
}
