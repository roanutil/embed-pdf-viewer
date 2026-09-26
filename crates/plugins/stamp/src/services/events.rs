//! `services/events.ts`: the eight change hooks, disposed with the plugin.
//! A failing listener is reported through `console.error`.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::scope::Teardown;
use epdf_kernel::{Controller, Error, Unsubscribe};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::model::{StampAction, StampState};

pub(crate) struct StampEvents {
    pub library_changed: EventHookSource<StampLibraryChange>,
    pub library_created: EventHookSource<StampLibraryEvent>,
    pub library_updated: EventHookSource<StampLibraryEvent>,
    pub library_deleted: EventHookSource<StampLibraryEvent>,
    pub asset_created: EventHookSource<StampAssetEvent>,
    pub asset_updated: EventHookSource<StampAssetEvent>,
    pub asset_deleted: EventHookSource<StampAssetEvent>,
    pub arm_changed: EventHookSource<StampArmChangedEvent>,
}

impl StampEvents {
    pub fn new(
        cx: &Controller<StampState, StampAction>,
        console_error: Option<Rc<dyn Fn(&str, &Error)>>,
    ) -> Self {
        let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
            if let Some(sink) = &console_error {
                sink("[stamp] event listener failed:", error);
            }
        });
        let events = StampEvents {
            library_changed: EventHookSource::new(Some(report.clone())),
            library_created: EventHookSource::new(Some(report.clone())),
            library_updated: EventHookSource::new(Some(report.clone())),
            library_deleted: EventHookSource::new(Some(report.clone())),
            asset_created: EventHookSource::new(Some(report.clone())),
            asset_updated: EventHookSource::new(Some(report.clone())),
            asset_deleted: EventHookSource::new(Some(report.clone())),
            arm_changed: EventHookSource::new(Some(report)),
        };
        let disposers: Vec<Box<dyn Fn()>> = vec![
            dispose(&events.library_changed),
            dispose(&events.library_created),
            dispose(&events.library_updated),
            dispose(&events.library_deleted),
            dispose(&events.asset_created),
            dispose(&events.asset_updated),
            dispose(&events.asset_deleted),
            dispose(&events.arm_changed),
        ];
        let teardown: Teardown = Box::new(move || {
            for d in &disposers {
                d();
            }
            Ok(None)
        });
        cx.defer(teardown);
        events
    }
}

fn dispose<T: 'static>(hook: &EventHookSource<T>) -> Box<dyn Fn()> {
    let hook = hook.clone();
    Box::new(move || hook.dispose())
}

macro_rules! hook {
    ($(#[$doc:meta])* $name:ident, $field:ident, $event:ty) => {
        $(#[$doc])*
        pub fn $name(
            &self,
            listener: impl Fn(&$event) -> Result<(), Error> + 'static,
        ) -> Unsubscribe {
            self.0.events.$field.on(listener)
        }
    };
}

impl StampCapability {
    hook!(
        /// `onLibraryChanged(listener)`: a library's bytes changed.
        on_library_changed, library_changed, StampLibraryChange
    );
    hook!(
        /// `onLibraryCreated(listener)`.
        on_library_created, library_created, StampLibraryEvent
    );
    hook!(
        /// `onLibraryUpdated(listener)`.
        on_library_updated, library_updated, StampLibraryEvent
    );
    hook!(
        /// `onLibraryDeleted(listener)`.
        on_library_deleted, library_deleted, StampLibraryEvent
    );
    hook!(
        /// `onAssetCreated(listener)`.
        on_asset_created, asset_created, StampAssetEvent
    );
    hook!(
        /// `onAssetUpdated(listener)`.
        on_asset_updated, asset_updated, StampAssetEvent
    );
    hook!(
        /// `onAssetDeleted(listener)`.
        on_asset_deleted, asset_deleted, StampAssetEvent
    );
    hook!(
        /// `onArmChanged(listener)`.
        on_arm_changed, arm_changed, StampArmChangedEvent
    );
}
