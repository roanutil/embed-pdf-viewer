//! The interaction plugin through uniffi: the pointer hub as one handle,
//! with a foreign handler trait a Swift gesture source can implement, and
//! the feedback plugin with its foreign provider.
//!
//! Every method hops to the kernel thread through `KernelBound`.
//! `dispatch_pointer` is the hot path: it fires and returns, so a 120 Hz
//! source never waits on the kernel thread. A foreign handler runs on the
//! kernel thread, synchronously, inside the dispatch.
//!
//! A tool's own pointer members and `PointerSample`'s `project` do not
//! cross: a native tool is data (its tags drive handlers), and a sample from
//! a native source carries only its page hit.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    HostError, KernelBound, KernelPost, Modifiers, PageRef, PointerGesture, PointerKind,
    PointerPageHit, PointerPhase, PointerSample, RegistrationHandle, Subscription, ViewerError,
};
use epdf_geometry::PageRotation;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};

use crate::{
    Feedback, ImpactWeight, InteractionCapability, NotifyKind, RegisterHandlerOptions,
    RegisterToolOptions, Tool, ToolTouch,
};

/// Where a foreign callback's failure goes: the kernel's report sink.
pub type Report = Rc<dyn Fn(&Error)>;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionTool {
    pub id: String,
    pub cursor: String,
    /// The cursor over the viewport's page gaps; `None` is the arrow.
    #[uniffi(default = None)]
    pub gap_cursor: Option<String>,
    /// Behaviour tags (`text-select`, `scroll`, ...), each once, in order.
    pub enables: Vec<String>,
    /// What one finger does on a touch screen; `None` reads as `Tap`.
    #[uniffi(default = None)]
    pub touch: Option<InteractionToolTouch>,
}

/// `ToolTouch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InteractionToolTouch {
    Draw,
    Tap,
}

impl From<ToolTouch> for InteractionToolTouch {
    fn from(touch: ToolTouch) -> Self {
        match touch {
            ToolTouch::Draw => InteractionToolTouch::Draw,
            ToolTouch::Tap => InteractionToolTouch::Tap,
        }
    }
}

impl From<InteractionToolTouch> for ToolTouch {
    fn from(touch: InteractionToolTouch) -> Self {
        match touch {
            InteractionToolTouch::Draw => ToolTouch::Draw,
            InteractionToolTouch::Tap => ToolTouch::Tap,
        }
    }
}

impl From<&Tool> for InteractionTool {
    fn from(tool: &Tool) -> Self {
        InteractionTool {
            id: tool.id.clone(),
            cursor: tool.cursor.clone(),
            gap_cursor: tool.gap_cursor.clone(),
            enables: tool.enables.clone(),
            touch: tool.touch.map(Into::into),
        }
    }
}

impl From<InteractionTool> for Tool {
    fn from(tool: InteractionTool) -> Self {
        let mut out = Tool::new(tool.id, tool.cursor, tool.enables.iter().map(String::as_str));
        out.gap_cursor = tool.gap_cursor;
        out.touch = tool.touch.map(Into::into);
        out
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionToolChangedEvent {
    pub tool_id: String,
    pub previous_tool_id: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionGestureEvent {
    /// The tool that was active when the gesture began.
    pub tool_id: String,
    pub page: Option<PageRef>,
    pub pointer_type: PointerKind,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InteractionCursorChangedEvent {
    pub cursor: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum InteractionEvent {
    ToolChanged { event: InteractionToolChangedEvent },
    GestureStarted { event: InteractionGestureEvent },
    GestureEnded { event: InteractionGestureEvent },
    GestureCancelled { event: InteractionGestureEvent },
    CursorChanged { event: InteractionCursorChangedEvent },
}

#[uniffi::export(with_foreign)]
pub trait InteractionEventListener: Send + Sync {
    fn on_event(&self, event: InteractionEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

impl From<PointerPhase> for crate::Phase {
    fn from(phase: PointerPhase) -> Self {
        match phase {
            PointerPhase::Down => crate::Phase::Down,
            PointerPhase::Move => crate::Phase::Move,
            PointerPhase::Up => crate::Phase::Up,
            PointerPhase::Cancel => crate::Phase::Cancel,
        }
    }
}

impl From<crate::Phase> for PointerPhase {
    fn from(phase: crate::Phase) -> Self {
        match phase {
            crate::Phase::Down => PointerPhase::Down,
            crate::Phase::Move => PointerPhase::Move,
            crate::Phase::Up => PointerPhase::Up,
            crate::Phase::Cancel => PointerPhase::Cancel,
        }
    }
}

impl From<PointerKind> for crate::PointerKind {
    fn from(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Mouse => crate::PointerKind::Mouse,
            PointerKind::Pen => crate::PointerKind::Pen,
            PointerKind::Touch => crate::PointerKind::Touch,
        }
    }
}

impl From<crate::PointerKind> for PointerKind {
    fn from(kind: crate::PointerKind) -> Self {
        match kind {
            crate::PointerKind::Mouse => PointerKind::Mouse,
            crate::PointerKind::Pen => PointerKind::Pen,
            crate::PointerKind::Touch => PointerKind::Touch,
        }
    }
}

impl From<PointerGesture> for crate::Gesture {
    fn from(gesture: PointerGesture) -> Self {
        match gesture {
            PointerGesture::LongPress => crate::Gesture::LongPress,
        }
    }
}

impl From<crate::Gesture> for PointerGesture {
    fn from(gesture: crate::Gesture) -> Self {
        match gesture {
            crate::Gesture::LongPress => PointerGesture::LongPress,
        }
    }
}

impl From<Modifiers> for crate::Modifiers {
    fn from(m: Modifiers) -> Self {
        crate::Modifiers { shift: m.shift, alt: m.alt, ctrl: m.ctrl, meta: m.meta }
    }
}

impl From<crate::Modifiers> for Modifiers {
    fn from(m: crate::Modifiers) -> Self {
        Modifiers { shift: m.shift, alt: m.alt, ctrl: m.ctrl, meta: m.meta }
    }
}

impl From<PointerPageHit> for crate::PageHit {
    fn from(hit: PointerPageHit) -> Self {
        crate::PageHit {
            page: hit.page.into(),
            point: hit.point.into(),
            scale: hit.scale,
            rotation: hit.rotation.map(PageRotation),
            zoom: hit.zoom,
        }
    }
}

impl From<crate::PageHit> for PointerPageHit {
    fn from(hit: crate::PageHit) -> Self {
        PointerPageHit {
            page: hit.page.into(),
            point: hit.point.into(),
            scale: hit.scale,
            rotation: hit.rotation.map(|r| r.0),
            zoom: hit.zoom,
        }
    }
}

impl From<PointerSample> for crate::PointerSample {
    fn from(s: PointerSample) -> Self {
        crate::PointerSample {
            phase: s.phase.into(),
            viewport: s.viewport.into(),
            page: s.page.map(Into::into),
            modifiers: s.modifiers.into(),
            click_count: s.click_count.map(f64::from),
            pointer_type: s.pointer_type.map(Into::into),
            gesture: s.gesture.map(Into::into),
            source: s.source,
            project: None,
        }
    }
}

impl From<&crate::PointerSample> for PointerSample {
    fn from(s: &crate::PointerSample) -> Self {
        PointerSample {
            phase: s.phase.into(),
            viewport: s.viewport.into(),
            page: s.page.map(Into::into),
            modifiers: s.modifiers.into(),
            // A click count is 1, 2 or 3; anything else reads as a single.
            click_count: s.click_count.map(|n| if (1.0..=3.0).contains(&n) { n as u32 } else { 1 }),
            pointer_type: s.pointer_type.map(Into::into),
            gesture: s.gesture.map(Into::into),
            source: s.source.clone(),
        }
    }
}

/// A registration whose unregister runs once and then lets go of the
/// remover. The remover holds what it registered (a foreign handler), and
/// the `RegistrationHandle` lives as long as Swift keeps it, so holding on
/// would keep a handler that keeps its own `Registration` alive for good.
fn registration(post: KernelPost, off: crate::Unregister) -> Arc<RegistrationHandle> {
    let off = RefCell::new(Some(off));
    RegistrationHandle::new(post, move || {
        if let Some(off) = off.borrow_mut().take() {
            off.call();
        }
    })
}

fn gesture_event(event: &crate::GestureEvent) -> InteractionGestureEvent {
    InteractionGestureEvent {
        tool_id: event.tool_id.clone(),
        page: event.page.map(Into::into),
        pointer_type: event.pointer_type.into(),
    }
}

/// A pointer handler a foreign gesture source implements. The hub routes
/// each gesture to the first eligible handler, by priority, whose `on_down`
/// returns true; move and up then go to it until the up.
///
/// Called on the kernel thread, synchronously, inside `dispatch_pointer`.
/// Return quickly and never block waiting for the viewer: a call into the
/// viewer from here queues behind this dispatch.
#[uniffi::export(with_foreign)]
pub trait InteractionHandler: Send + Sync {
    /// Usually "does the tool enable my tag".
    fn enabled_for(&self, tool: InteractionTool) -> bool;
    /// True captures the gesture. A thrown error is reported through the
    /// kernel's report sink and the gesture is not captured.
    fn on_down(&self, sample: PointerSample) -> Result<bool, HostError>;
    fn on_move(&self, sample: PointerSample);
    fn on_up(&self, sample: PointerSample);
    /// The gesture was aborted, not completed.
    fn on_cancel(&self, sample: PointerSample);
    /// The pointer moved with no gesture active: cursor feedback only.
    fn on_hover(&self, sample: PointerSample);
    /// Touch consent: claim this contact as a tool gesture. A pure read.
    fn claims_touch(&self, sample: PointerSample) -> bool;
}

/// The hub's handler, backed by a foreign one. The id and priority are
/// fixed at registration, because the hub reads them by reference.
struct ForeignHandler {
    id: String,
    priority: f64,
    inner: Arc<dyn InteractionHandler>,
    report: Report,
}

impl crate::InteractionHandler for ForeignHandler {
    fn id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> f64 {
        self.priority
    }
    fn enabled_for(&self, tool: &Tool) -> bool {
        self.inner.enabled_for(tool.into())
    }
    fn on_down(&self, sample: &crate::PointerSample) -> bool {
        match self.inner.on_down(sample.into()) {
            Ok(captured) => captured,
            Err(error) => {
                (self.report)(&error.into());
                false
            }
        }
    }
    fn on_move(&self, sample: &crate::PointerSample) {
        self.inner.on_move(sample.into());
    }
    fn on_up(&self, sample: &crate::PointerSample) {
        self.inner.on_up(sample.into());
    }
    fn on_cancel(&self, sample: &crate::PointerSample) {
        self.inner.on_cancel(sample.into());
    }
    fn on_hover(&self, sample: &crate::PointerSample) {
        self.inner.on_hover(sample.into());
    }
    fn claims_touch(&self, sample: &crate::PointerSample) -> bool {
        self.inner.claims_touch(sample.into())
    }
}

/// The capability, if its document is still open.
fn open(cap: &InteractionCapability) -> Result<&InteractionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "interaction",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// What lives in the kernel thread's slab for one handle.
struct Bound {
    cap: InteractionCapability,
    report: Report,
}

#[derive(uniffi::Object)]
pub struct InteractionHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl InteractionHandle {
    /// On the kernel thread. `report` receives a foreign handler's errors.
    pub fn new(cap: InteractionCapability, post: KernelPost, report: Report) -> Arc<Self> {
        Arc::new(InteractionHandle {
            bound: KernelBound::new(Bound { cap, report }, post.clone()),
            post,
        })
    }
}

#[uniffi::export]
impl InteractionHandle {
    /// The armed tool. An id nobody registered reads back as a neutral tool
    /// with no tags.
    pub async fn active_tool(&self) -> Result<InteractionTool, ViewerError> {
        self.bound.call(|b| Ok((&*open(&b.cap)?.get_active_tool()).into())).await
    }

    pub async fn active_tool_id(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_active_tool_id())).await
    }

    /// The tool a freshly opened document starts with.
    pub async fn default_tool_id(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_default_tool_id())).await
    }

    pub async fn tools(&self) -> Result<Vec<InteractionTool>, ViewerError> {
        self.bound
            .call(|b| Ok(open(&b.cap)?.list_tools().iter().map(|t| (&**t).into()).collect()))
            .await
    }

    pub async fn tool(&self, id: String) -> Result<Option<InteractionTool>, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.get_tool(&id).map(|t| (&*t).into()))).await
    }

    pub async fn has_tool(&self, id: String) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.has_tool(&id))).await
    }

    /// Whether the active tool enables a behaviour tag such as `text-select`.
    pub async fn active_tool_enables(&self, behavior: String) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.active_tool_enables(&behavior))).await
    }

    /// Rejects `not-found` for an id nobody registered.
    pub async fn activate_tool(&self, id: String) -> Result<(), ViewerError> {
        self.bound.call(move |b| open(&b.cap)?.activate_tool(&id)).await
    }

    pub async fn activate_default_tool(&self) -> Result<(), ViewerError> {
        self.bound.call(|b| open(&b.cap)?.activate_default_tool()).await
    }

    /// Arm a tool for now (hold to pan); `pop_tool` restores the last one.
    pub async fn push_tool(&self, id: String) -> Result<(), ViewerError> {
        self.bound.call(move |b| open(&b.cap)?.push_tool(&id)).await
    }

    /// Nothing when the stack is empty.
    pub async fn pop_tool(&self) -> Result<(), ViewerError> {
        self.bound.call(|b| open(&b.cap)?.pop_tool()).await
    }

    /// Replace a tool's cursors by name; `None` brings the tool's own back.
    pub async fn set_tool_cursor(
        &self,
        id: String,
        cursors: Option<HashMap<String, String>>,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                let cursors = cursors.map(|s| s.into_iter().collect::<BTreeMap<_, _>>());
                open(&b.cap)?.set_tool_cursor(&id, cursors);
                Ok(())
            })
            .await
    }

    /// Rejects `conflict` for an id already registered unless `replace`.
    /// The registration's `unregister` removes only this registration.
    pub async fn register_tool(
        &self,
        tool: InteractionTool,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let off = open(&b.cap)?
                    .register_tool(Tool::from(tool), RegisterToolOptions { replace })?;
                Ok(registration(post, off))
            })
            .await
    }

    /// Add a pointer handler. `priority` is fixed here; higher wins a
    /// gesture. `source` scopes it to one lens (a stage id).
    pub async fn register_handler(
        &self,
        id: String,
        priority: f64,
        source: Option<String>,
        handler: Arc<dyn InteractionHandler>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let foreign = Rc::new(ForeignHandler {
                    id,
                    priority,
                    inner: handler,
                    report: b.report.clone(),
                });
                let off =
                    open(&b.cap)?.register_handler(foreign, RegisterHandlerOptions { source });
                Ok(registration(post, off))
            })
            .await
    }

    /// Push a cursor claim, or clear it with `None`. Higher priority wins.
    pub async fn claim_cursor(
        &self,
        token: String,
        cursor: Option<String>,
        priority: f64,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                open(&b.cap)?.claim_cursor(&token, cursor.as_deref(), priority);
                Ok(())
            })
            .await
    }

    /// The resolved cursor keyword.
    pub async fn cursor(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_cursor())).await
    }

    /// Whether a hover claim holds the cursor.
    pub async fn has_cursor_claim(&self) -> Result<bool, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.has_cursor_claim())).await
    }

    /// Touch pre-flight: would any eligible handler claim this contact.
    pub async fn would_claim_touch(&self, sample: PointerSample) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.would_claim_touch(&sample.into()))).await
    }

    /// Pointer ingress. Returns at once; the sample runs on the kernel
    /// thread in the order it was sent. Dropped after the document closes.
    pub fn dispatch_pointer(&self, sample: PointerSample) {
        self.bound.fire(move |b| {
            if let Ok(cap) = open(&b.cap) {
                cap.dispatch_pointer(&sample.into());
            }
        });
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets `on_end` when the
    /// document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn InteractionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.bound.fire(move |b| {
            let c = &b.cap;
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    macro_rules! relay {
                        ($hook:ident, |$e:ident| $event:expr) => {{
                            let listener = listener.clone();
                            c.$hook(move |$e| {
                                listener.on_event($event);
                                Ok(())
                            })
                        }};
                    }
                    let offs: Vec<Unsubscribe> = vec![
                        relay!(on_tool_changed, |e| InteractionEvent::ToolChanged {
                            event: InteractionToolChangedEvent {
                                tool_id: e.tool_id.clone(),
                                previous_tool_id: e.previous_tool_id.clone(),
                            },
                        }),
                        relay!(on_gesture_started, |e| InteractionEvent::GestureStarted {
                            event: gesture_event(e),
                        }),
                        relay!(on_gesture_ended, |e| InteractionEvent::GestureEnded {
                            event: gesture_event(e),
                        }),
                        relay!(on_gesture_cancelled, |e| InteractionEvent::GestureCancelled {
                            event: gesture_event(e),
                        }),
                        relay!(on_cursor_changed, |e| InteractionEvent::CursorChanged {
                            event: InteractionCursorChangedEvent { cursor: e.cursor.clone() },
                        }),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FeedbackImpactWeight {
    Light,
    Medium,
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FeedbackNotifyKind {
    Success,
    Warning,
    Error,
}

impl From<ImpactWeight> for FeedbackImpactWeight {
    fn from(weight: ImpactWeight) -> Self {
        match weight {
            ImpactWeight::Light => FeedbackImpactWeight::Light,
            ImpactWeight::Medium => FeedbackImpactWeight::Medium,
            ImpactWeight::Heavy => FeedbackImpactWeight::Heavy,
        }
    }
}

impl From<FeedbackImpactWeight> for ImpactWeight {
    fn from(weight: FeedbackImpactWeight) -> Self {
        match weight {
            FeedbackImpactWeight::Light => ImpactWeight::Light,
            FeedbackImpactWeight::Medium => ImpactWeight::Medium,
            FeedbackImpactWeight::Heavy => ImpactWeight::Heavy,
        }
    }
}

impl From<NotifyKind> for FeedbackNotifyKind {
    fn from(kind: NotifyKind) -> Self {
        match kind {
            NotifyKind::Success => FeedbackNotifyKind::Success,
            NotifyKind::Warning => FeedbackNotifyKind::Warning,
            NotifyKind::Error => FeedbackNotifyKind::Error,
        }
    }
}

impl From<FeedbackNotifyKind> for NotifyKind {
    fn from(kind: FeedbackNotifyKind) -> Self {
        match kind {
            FeedbackNotifyKind::Success => NotifyKind::Success,
            FeedbackNotifyKind::Warning => NotifyKind::Warning,
            FeedbackNotifyKind::Error => NotifyKind::Error,
        }
    }
}

/// Haptics, as the platform provides them. Called on the kernel thread:
/// an implementation that needs the main thread (UIKit's feedback
/// generators do) hops there itself.
#[uniffi::export(with_foreign)]
pub trait PlatformFeedback: Send + Sync {
    fn selection(&self);
    fn impact(&self, weight: Option<FeedbackImpactWeight>);
    fn notify(&self, kind: FeedbackNotifyKind);
}

struct ForeignFeedback(Arc<dyn PlatformFeedback>);

impl crate::PlatformFeedback for ForeignFeedback {
    fn selection(&self) {
        self.0.selection();
    }
    fn impact(&self, weight: Option<ImpactWeight>) {
        self.0.impact(weight.map(Into::into));
    }
    fn notify(&self, kind: NotifyKind) {
        self.0.notify(kind.into());
    }
}

/// `feedbackPlugin({ provider })`'s options, as `ViewerConfig` carries them.
///
/// An object, not a record. UniFFI 0.32 takes a record holding a foreign
/// trait object, but a record in another crate that nests it
/// (`ViewerConfig`) can't see the trait object inside: bindgen runs one
/// crate at a time, so it gives that record Swift's synthesized
/// `Equatable` and `Hashable`, which don't compile. A field typed as an
/// object is visible to it, and the outer record goes without them.
#[derive(uniffi::Object)]
pub struct FeedbackConfig {
    provider: Arc<dyn PlatformFeedback>,
}

impl std::fmt::Debug for FeedbackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedbackConfig").field("provider", &"foreign").finish()
    }
}

#[uniffi::export]
impl FeedbackConfig {
    #[uniffi::constructor]
    pub fn new(provider: Arc<dyn PlatformFeedback>) -> Arc<Self> {
        Arc::new(FeedbackConfig { provider })
    }
}

impl FeedbackConfig {
    /// On the kernel thread: the options hold an `Rc`.
    pub fn to_options(&self) -> crate::FeedbackPluginOptions {
        crate::FeedbackPluginOptions {
            provider: Some(Rc::new(ForeignFeedback(self.provider.clone()))),
        }
    }
}

/// The workspace's feedback capability: what plugins call, callable from
/// Swift too (a custom gesture that wants the same haptic vocabulary).
#[derive(uniffi::Object)]
pub struct FeedbackHandle {
    cap: KernelBound<Feedback>,
}

impl FeedbackHandle {
    /// On the kernel thread.
    pub fn new(cap: Feedback, post: KernelPost) -> Arc<Self> {
        Arc::new(FeedbackHandle { cap: KernelBound::new(cap, post) })
    }
}

#[uniffi::export]
impl FeedbackHandle {
    pub async fn selection(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                c.selection();
                Ok(())
            })
            .await
    }

    pub async fn impact(&self, weight: Option<FeedbackImpactWeight>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.impact(weight.map(Into::into));
                Ok(())
            })
            .await
    }

    pub async fn notify(&self, kind: FeedbackNotifyKind) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.notify(kind.into());
                Ok(())
            })
            .await
    }
}
