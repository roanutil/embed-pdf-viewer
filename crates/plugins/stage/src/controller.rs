//! `controller.ts`: the composition root. It builds the plugin's services
//! once (the events, the host's clock, the placement latch, the scene
//! model) and assembles the host capability from the areas.
//!
//! The TypeScript builds one closure object per area and spreads their
//! `api` slices into one capability with `composeApi`. The areas call each
//! other in a cycle (arrivals re-place after settings changes, settings
//! re-anchor through arrivals), so here they are `impl StageCapability`
//! blocks over one shared state, one file per TypeScript area:
//! `services/{events,scene}`, `read/pages`, `camera/{write,animation,
//! gestures}`, `navigation/{arrive,reveal,zoom}`, `settings/area` (the
//! TypeScript's `settings/settings.ts`) and `view/lifecycle`.
//!
//! The pure spatial math is `epdf-stage-core`; this is the impure shell. It
//! writes state, caches the scene, and drives motion, and time enters only
//! through the host's clock (`ctx.clock`), whose frames and timers the
//! kernel cancels when the document closes.
//!
//! Every camera move is defined by what it holds fixed: gestures hold the
//! pointer, navigation lands at the arrivalAlign point, positioned reveals
//! and restored viewpoints hold what the call specifies, pointer-less zooms
//! hold the zoomAlign point, reframes hold the anchorAlign point, and an
//! axis the camera cannot travel rests at fitAlign.
//!
//! The order of reads, state writes and clock calls follows the TypeScript
//! line by line, because the vectors record every state change. No
//! `RefCell` borrow is held across a state write: a write runs the store's
//! watchers and the change hooks, and both read back into this capability.

use epdf_kernel::Controller;
use epdf_kernel::clock::{Cancel, PluginClock};
use epdf_kernel::event_hook::EventHookSource;
use serde_json::Value;

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

pub(crate) struct Grouping {
    pub(crate) grouping: Rc<Vec<Vec<usize>>>,
    pub(crate) first_pages: Rc<Vec<usize>>,
}

type Provider = Rc<dyn Fn() -> Option<StageViewPatch>>;

/// The six events (`services/events.ts`).
pub(crate) struct Events {
    pub(crate) page_changed: EventHookSource<StagePageChangedEvent>,
    pub(crate) zoom_changed: EventHookSource<StageZoomChangedEvent>,
    pub(crate) camera_changed: EventHookSource<StageCameraChangedEvent>,
    pub(crate) motion_ended: EventHookSource<StageMotionEndedEvent>,
    pub(crate) settings_changed: EventHookSource<StageSettingsChangedEvent>,
    pub(crate) viewport_changed: EventHookSource<StageViewportChangedEvent>,
}

/// `services/placement-latch.ts`: flips when initial placement starts,
/// while `state.placed` flips only after it finishes. `pending` is the last
/// navigation asked for before placement; placement applies it last.
pub(crate) struct PlacementLatch {
    pub(crate) started: Cell<bool>,
    pub(crate) pending: RefCell<Option<Box<dyn FnOnce(&StageCapability)>>>,
}

/// `camera/write.ts`'s gesture bracket.
#[derive(Default)]
pub(crate) struct Gesture {
    pub(crate) depth: Cell<u32>,
    pub(crate) zoomed: Cell<bool>,
    pub(crate) elastic: Cell<bool>,
    /// The elastic gesture's unclamped, finger-integrated camera.
    pub(crate) raw: Cell<Option<Camera>>,
}

/// The visible pages' memo inputs (`read/pages.ts`), compared with
/// `Object.is` element by element.
#[derive(Clone, PartialEq)]
pub(crate) struct VisibleInputs {
    pub(crate) scene_key: String,
    pub(crate) flow: FlowMode,
    pub(crate) numbers: [JsNumber; 6],
    pub(crate) resting: bool,
}

/// A number compared as `Object.is` compares it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct JsNumber(pub(crate) f64);

impl PartialEq for JsNumber {
    fn eq(&self, other: &Self) -> bool {
        crate::model::object_is(self.0, other.0)
    }
}

pub(crate) struct Inner {
    pub(crate) events: Events,
    /// `ctx.clock`: frames and timers, cancelled when the document closes.
    pub(crate) clock: PluginClock,
    pub(crate) placement: PlacementLatch,
    // services/scene
    pub(crate) grouping_memo: RefCell<Option<((u32, sc::SpreadMode), Rc<Grouping>)>>,
    pub(crate) scene_cache: RefCell<Option<(String, Rc<Scene>)>>,
    /// One entry per registry row, `None` for a row that does not parse,
    /// so display indexes stay aligned with the document's.
    pub(crate) registry_cache: RefCell<Option<(Rc<Value>, Rc<Vec<Option<PageRow>>>)>>,
    // camera/write
    pub(crate) gesture: Gesture,
    pub(crate) cancel_rest: RefCell<Option<Cancel>>,
    // camera/animation: the frame the running motion waits for
    pub(crate) cancel_frame: RefCell<Option<Cancel>>,
    // read/pages
    pub(crate) last_metrics: Cell<Option<ScrollMetrics>>,
    pub(crate) visible_memo: RefCell<Option<(VisibleInputs, Rc<Vec<VisiblePage>>)>>,
    /// `noVisiblePages`: the one empty list read before placement.
    pub(crate) no_visible_pages: Rc<Vec<VisiblePage>>,
    // settings/settings
    pub(crate) base: RefCell<StageSettings>,
    pub(crate) initial_base: StageSettings,
    pub(crate) rules: RefCell<Rc<Vec<ResponsiveRule>>>,
    pub(crate) last_effective: RefCell<StageSettings>,
    // view/lifecycle: registration id, priority and provider
    pub(crate) providers: RefCell<Vec<(u64, f64, Provider)>>,
    pub(crate) next_provider: Cell<u64>,
}

/// `StageHostCapability`: the stage's public contract and its host lens,
/// one object as in the TypeScript (the host token is the public one,
/// typed wider). Cheap to clone; clones are the same lens.
#[derive(Clone)]
pub struct StageCapability {
    pub(crate) ctx: Controller<StageState, Value>,
    pub(crate) inner: Rc<Inner>,
}

/// `createStageController(ctx, config)`: the capability. `connect` (see
/// [`crate::connect`]) runs once every dependency exists.
pub(crate) fn create_stage_controller(
    ctx: Controller<StageState, Value>,
    config: StageConfig,
) -> StageCapability {
    let base = ctx.state().settings.clone();
    let rules = config.responsive.unwrap_or_else(default_responsive);
    let events = Events {
        page_changed: ctx.event_source(),
        zoom_changed: ctx.event_source(),
        camera_changed: ctx.event_source(),
        motion_ended: ctx.event_source(),
        settings_changed: ctx.event_source(),
        viewport_changed: ctx.event_source(),
    };
    let stage = StageCapability {
        inner: Rc::new(Inner {
            events,
            clock: ctx.clock(),
            placement: PlacementLatch { started: Cell::new(false), pending: RefCell::new(None) },
            grouping_memo: RefCell::new(None),
            scene_cache: RefCell::new(None),
            registry_cache: RefCell::new(None),
            gesture: Gesture::default(),
            cancel_rest: RefCell::new(None),
            cancel_frame: RefCell::new(None),
            last_metrics: Cell::new(None),
            visible_memo: RefCell::new(None),
            no_visible_pages: Rc::new(Vec::new()),
            last_effective: RefCell::new(base.clone()),
            initial_base: base.clone(),
            base: RefCell::new(base),
            rules: RefCell::new(Rc::new(rules)),
            providers: RefCell::new(Vec::new()),
            next_provider: Cell::new(0),
        }),
        ctx,
    };
    stage.connect_events();
    let inner = Rc::downgrade(&stage.inner);
    stage.ctx.cleanup_fn(move || {
        if let Some(inner) = inner.upgrade() {
            // Rust only: an initial-view provider, a rule predicate or a
            // pending navigation can hold this capability, and so this
            // `Inner`. Let go of them at close, as a garbage collector
            // would reclaim the cycle (the interaction hub does the same
            // with its handlers). The clock's frames and timers the kernel
            // cancels with the instance lifetime.
            inner.providers.borrow_mut().clear();
            *inner.rules.borrow_mut() = Rc::new(Vec::new());
            inner.placement.pending.borrow_mut().take();
            inner.cancel_frame.borrow_mut().take();
            inner.cancel_rest.borrow_mut().take();
        }
    });
    stage
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

    /// `ctx.state.get()`.
    pub(crate) fn st(&self) -> Rc<StageState> {
        self.ctx.state()
    }

    /// `ctx.state.update(transition)`.
    pub(crate) fn update(&self, transition: impl FnOnce(&Rc<StageState>) -> Rc<StageState>) {
        self.ctx.update_state(|state| transition(&state));
    }
}
