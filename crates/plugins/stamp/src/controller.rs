//! `controller.ts`: the composition root. It builds the plugin's services
//! once and assembles the capability from the areas' slices.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes their API objects. Here the areas are `impl`
//! blocks on one [`StampCapability`], one per TypeScript file (`read/`,
//! `write/`, `services/`), and their private state sits in [`Core`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::rt::{self, Promise};
use epdf_kernel::scope::Teardown;
use epdf_kernel::{Controller, Error};

use crate::contract::*;
use crate::model::{StampAction, StampState};
use crate::services::binaries::AssetBinary;
use crate::services::events::StampEvents;

/// Ghost renders per size bucket, per asset.
pub(crate) type GhostCache = Rc<RefCell<HashMap<u64, Promise<Option<StampAssetPreview>>>>>;

/// The plugin's private state, by the service or area that owns it.
pub(crate) struct Core {
    pub(crate) cx: Controller<StampState, StampAction>,
    pub(crate) config: StampConfig,
    // services/events
    pub(crate) events: StampEvents,
    // services/binaries
    /// Asset bytes and cached previews, keyed by asset id.
    pub(crate) binaries: RefCell<HashMap<String, AssetBinary>>,
    /// The canonical library PDFs. `None` is a save that answered no bytes.
    pub(crate) library_binaries: RefCell<HashMap<String, Option<Vec<u8>>>>,
    /// Whole-PDF rewrites are serialized per library.
    pub(crate) library_mutation_tails: RefCell<HashMap<String, Promise<()>>>,
    pub(crate) ghost_renders: RefCell<HashMap<String, GhostCache>>,
    // services/asset-engine
    pub(crate) asset_engine_ref: RefCell<Option<AssetEngineRef>>,
    pub(crate) import_supported: Cell<Option<bool>>,
    // write/placement
    pub(crate) armed_by_document: RefCell<HashMap<String, String>>,
}

/// `StampHostCapability`, which equals the public `StampCapability`. Cheap
/// to clone; clones share state.
#[derive(Clone)]
pub struct StampCapability(pub(crate) Rc<Core>);

/// `StampHostCapability`.
pub type StampHostCapability = StampCapability;

/// `createStampController(ctx, config)`.
pub fn create_stamp_controller(
    cx: Controller<StampState, StampAction>,
    config: StampConfig,
) -> StampCapability {
    let events = StampEvents::new(&cx, config.host.console_error.clone());
    let core = Core {
        cx: cx.clone(),
        config,
        events,
        binaries: RefCell::default(),
        library_binaries: RefCell::default(),
        library_mutation_tails: RefCell::default(),
        ghost_renders: RefCell::default(),
        asset_engine_ref: RefCell::new(None),
        import_supported: Cell::new(None),
        armed_by_document: RefCell::default(),
    };
    let api = StampCapability(Rc::new(core));
    // `createBinaries`' cleanup, then `createAssetEngine`'s.
    let inner = api.0.clone();
    let teardown: Teardown = Box::new(move || {
        inner.binaries.borrow_mut().clear();
        inner.ghost_renders.borrow_mut().clear();
        inner.library_binaries.borrow_mut().clear();
        inner.library_mutation_tails.borrow_mut().clear();
        Ok(None)
    });
    cx.defer(teardown);
    let inner = api.0.clone();
    let teardown: Teardown = Box::new(move || {
        let owned = match inner.config.asset_engine {
            Some(AssetEngine::Factory(_)) => inner.asset_engine_ref.borrow().clone(),
            _ => None,
        };
        *inner.asset_engine_ref.borrow_mut() = None;
        if let Some(owned) = owned {
            // `void Promise.resolve(owned).then((e) => e.destroy()).catch(() => {})`
            rt::spawn(async move {
                let engine = match owned {
                    AssetEngineRef::Ready(engine) => {
                        rt::tick().await;
                        engine
                    }
                    AssetEngineRef::Pending(promise) => match promise.await {
                        Ok(engine) => engine,
                        Err(_) => return,
                    },
                };
                engine.destroy().ignore();
            });
        }
        Ok(None)
    });
    cx.defer(teardown);
    api
}

impl StampCapability {
    pub(crate) fn cx(&self) -> &Controller<StampState, StampAction> {
        &self.0.cx
    }

    /// `ctx.getState()`.
    pub(crate) fn state(&self) -> Rc<StampState> {
        self.0.cx.state()
    }

    pub(crate) fn dispatch(&self, action: StampAction) {
        self.0.cx.dispatch(action);
    }

    pub(crate) fn warn(&self, message: &str, error: Option<&Error>) {
        if let Some(sink) = &self.0.config.host.console_warn {
            sink(message, error);
        }
    }

    /// `Math.random()`.
    pub(crate) fn random(&self) -> f64 {
        match &self.0.config.host.random {
            Some(random) => random(),
            None => clock_random(),
        }
    }
}

/// The default `random`: a small xorshift, seeded from the clock. The
/// default host has no better entropy to offer without a dependency.
fn clock_random() -> f64 {
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
    }
    STATE.with(|s| {
        let mut x = s.get() ^ (rt::now() as u64);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::clock_random;

    /// The vectors always answer `random`, so the default is pinned here:
    /// `Math.random`'s contract, draws in [0, 1), spread over the interval.
    #[test]
    fn the_default_random_spreads_over_the_unit_interval() {
        let draws: Vec<f64> = (0..10_000).map(|_| clock_random()).collect();
        assert!(draws.iter().all(|x| (0.0..1.0).contains(x)), "a draw left [0, 1)");
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        assert!((0.45..0.55).contains(&mean), "mean {mean}");
        let mut tenths = [0usize; 10];
        for x in &draws {
            tenths[(x * 10.0) as usize] += 1;
        }
        assert!(tenths.iter().all(|&n| n > 800), "{tenths:?}");
    }
}
