//! `controller.ts`: the composition root. It builds the plugin's services
//! once (the diffing dispatch and its change hooks, the timing seam, the
//! scene model) and assembles the capability from the areas.
//!
//! The TypeScript builds one closure object per area and spreads their
//! `api` slices into one capability with `composeApi`. The areas call each
//! other in a cycle (arrivals re-place after settings changes, settings
//! re-anchor through arrivals), so here they are `impl StageCapability`
//! blocks over one shared state, one file per TypeScript area:
//! `services/{scheduler,events,scene}`, `read/pages`,
//! `camera/{write,animation,gestures}`, `navigation/{arrive,reveal,zoom}`,
//! `settings/area` (the TypeScript's `settings/settings.ts`) and
//! `view/lifecycle`.
//!
//! The pure spatial math is `epdf-stage-core`; this is the impure shell. It
//! dispatches, caches the scene, and owns the camera tween, the rest
//! detector and the momentum physics, whose frames come through the
//! [`Scheduler`] the host passes (see the README's host inputs).
//!
//! Every camera move is defined by what it holds fixed: gestures hold the
//! pointer, pointer-less zooms the `zoomAlign` point, reframes the
//! `anchorAlign` point, arrivals nothing prior (a fresh landing at
//! `arrivalAlign`), explicit reveals whatever the call says. `fitAlign` is
//! the clamp's rest constraint on an axis with no travel.
//!
//! The order of reads, dispatches and scheduler calls follows the
//! TypeScript line by line, because the vectors record every dispatch. No
//! `RefCell` borrow is held across a dispatch: a dispatch runs the effects'
//! watches and the change hooks, and both read back into this capability.

use epdf_kernel::Controller;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;

use crate::prelude::*;

/// A `TypeError` for a member read on `undefined`, with V8's message.
pub(crate) fn read_of_undefined(member: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of undefined (reading '{member}')"))
}

/// A stage-core failure as the error the TypeScript throws.
pub(crate) fn stage_error(error: sc::StageError) -> Error {
    Error::named(error.name(), error.to_string())
}

/// JavaScript falsiness of a number: `0`, `-0` and `NaN`.
pub(crate) fn falsy(x: f64) -> bool {
    x == 0.0 || x.is_nan()
}

pub(crate) fn num(x: f64) -> String {
    js::number_to_string(x)
}

/// A unit enum's JSON string (`'vertical'`, `'odd'`, ...).
pub(crate) fn serde_str<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub(crate) const REST_MS: f64 = 150.0;
pub(crate) const TWEEN_MS: f64 = 240.0;

pub(crate) struct Grouping {
    pub(crate) key: (usize, sc::SpreadMode),
    pub(crate) grouping: Rc<Vec<Vec<usize>>>,
    pub(crate) first_pages: Rc<Vec<usize>>,
}

type Provider = Rc<dyn Fn() -> Option<StageViewState>>;

/// The six change hooks (`services/events.ts`).
pub(crate) struct Events {
    pub(crate) page_changed: EventHookSource<StagePageChangedEvent>,
    pub(crate) zoom_changed: EventHookSource<StageZoomChangedEvent>,
    pub(crate) camera_changed: EventHookSource<StageCameraChangedEvent>,
    pub(crate) motion_ended: EventHookSource<StageMotionEndedEvent>,
    pub(crate) settings_changed: EventHookSource<StageSettingsChangedEvent>,
    pub(crate) viewport_changed: EventHookSource<StageViewportChangedEvent>,
}

pub(crate) struct Inner {
    pub(crate) scheduler: Option<Rc<dyn Scheduler>>,
    pub(crate) events: Events,
    /// Set for the one CAMERA write whose camera is the object already in
    /// state (`setCam(cam())` on an unbounded stage, where `clampCamera`
    /// returns its input): the TypeScript's `before.camera !== after.camera`
    /// is false then, and no camera event fires.
    pub(crate) camera_kept: Cell<bool>,
    pub(crate) grouping_cache: RefCell<Option<Rc<Grouping>>>,
    pub(crate) scene_cache: RefCell<Option<(String, Rc<Scene>)>>,
    /// One entry per registry row, `None` for a row that does not parse,
    /// so display indexes stay aligned with the document's.
    pub(crate) registry_cache: RefCell<Option<((u32, u64), Rc<Vec<Option<PageLayout>>>)>>,
    /// `services/placement-latch.ts`: flips when initial placement starts,
    /// while `state.placed` flips only after it finishes.
    pub(crate) placement_started: Cell<bool>,
    pub(crate) gesture_depth: Cell<u32>,
    pub(crate) gesture_zoomed: Cell<bool>,
    pub(crate) gesture_elastic: Cell<bool>,
    pub(crate) gesture_raw: Cell<Option<Camera>>,
    pub(crate) rest_raf: Cell<u64>,
    pub(crate) raf: Cell<u64>,
    pub(crate) scroll_memo: Cell<Option<ScrollMetrics>>,
    pub(crate) vis: RefCell<(String, Rc<Vec<VisiblePage>>)>,
    pub(crate) base: RefCell<StageSettings>,
    pub(crate) initial_base: StageSettings,
    pub(crate) rules: RefCell<Rc<Vec<ResponsiveRule>>>,
    pub(crate) last_effective: RefCell<StageSettings>,
    /// Registration id, priority and provider.
    pub(crate) providers: RefCell<Vec<(u64, f64, Provider)>>,
    pub(crate) next_provider: Cell<u64>,
}

/// `StageHostCapability`: the stage's public contract and its host lens,
/// one object as in the TypeScript (the host token is the public one,
/// typed wider). Cheap to clone; clones are the same lens.
#[derive(Clone)]
pub struct StageCapability {
    pub(crate) ctx: Controller<StageState, StageAction>,
    pub(crate) inner: Rc<Inner>,
}

/// `createStageController(ctx, config)`: the capability and the connect
/// half. `connect` registers the navigation executors (main lens only).
pub fn create_stage_controller(
    ctx: Controller<StageState, StageAction>,
    config: StageConfig,
) -> StageCapability {
    let base = ctx.state().settings.clone();
    let rules = config.responsive.unwrap_or_else(default_responsive);
    let events = Events {
        page_changed: EventHookSource::new(None),
        zoom_changed: EventHookSource::new(None),
        camera_changed: EventHookSource::new(None),
        motion_ended: EventHookSource::new(None),
        settings_changed: EventHookSource::new(None),
        viewport_changed: EventHookSource::new(None),
    };
    let stage = StageCapability {
        inner: Rc::new(Inner {
            scheduler: config.scheduler,
            events,
            camera_kept: Cell::new(false),
            grouping_cache: RefCell::new(None),
            scene_cache: RefCell::new(None),
            registry_cache: RefCell::new(None),
            placement_started: Cell::new(false),
            gesture_depth: Cell::new(0),
            gesture_zoomed: Cell::new(false),
            gesture_elastic: Cell::new(false),
            gesture_raw: Cell::new(None),
            rest_raf: Cell::new(0),
            raf: Cell::new(0),
            scroll_memo: Cell::new(None),
            vis: RefCell::new((String::new(), Rc::new(Vec::new()))),
            last_effective: RefCell::new(base.clone()),
            initial_base: base.clone(),
            base: RefCell::new(base),
            rules: RefCell::new(Rc::new(rules)),
            providers: RefCell::new(Vec::new()),
            next_provider: Cell::new(0),
        }),
        ctx,
    };
    let inner = Rc::downgrade(&stage.inner);
    stage.ctx.cleanup_fn(move || {
        if let Some(inner) = inner.upgrade() {
            let e = &inner.events;
            e.page_changed.dispose();
            e.zoom_changed.dispose();
            e.camera_changed.dispose();
            e.motion_ended.dispose();
            e.settings_changed.dispose();
            e.viewport_changed.dispose();
            // Rust only: an initial-view provider or a rule predicate can
            // hold this capability, and so this `Inner`. Let go of them at
            // close, as a garbage collector would reclaim the cycle (the
            // interaction hub does the same with its handlers).
            inner.providers.borrow_mut().clear();
            *inner.rules.borrow_mut() = Rc::new(Vec::new());
        }
    });
    stage
}

/// `create(ctx)`'s return for [`crate::stage_plugin`]: the api, and a
/// connect that runs `register`.
pub(crate) fn created(
    stage: StageCapability,
    register: impl Fn() -> Result<(), Error> + 'static,
) -> (StageCapability, Option<ConnectFn>) {
    (stage, Some(Rc::new(register)))
}

impl StageCapability {
    /// Whether two handles are the same lens.
    pub fn same(&self, other: &StageCapability) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.ctx.lifetime()
    }
}
