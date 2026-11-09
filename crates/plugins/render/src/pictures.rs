//! `pictures.ts`: whether each page's picture is on screen, the reports the
//! page layers keep, folded into one state per page and view.
//!
//! A layer that paints a page opens a report on it when it mounts
//! (`reportPicture`), and the page is `loading` in that view from then on.
//! It turns `ready` once a report says `shown()`, and stays so: a later
//! picture of the page replaces the one on screen only once it's there. A
//! failure before that makes it `forbidden` or `error`. `slow` turns on
//! when it has been loading for `slowAfterMs`, on the plugin's clock, and
//! the page goes back to `idle` once every report on it closed (its layers
//! unmounted).
//!
//! Like the tile plans, the states live here and not in the plugin's state:
//! they follow the layers, which unmount while the document closes, after
//! the plugin's state stopped taking writes. A change wakes the readers with
//! `ctx.notify()`, which does nothing by then.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use epdf_kernel::clock::Cancel;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::mirror::ResourceStatus;
use indexmap::IndexMap;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::contract::ser_page;

/// `PictureState`: whether a picture of a page is on screen in a view.
/// Only the first picture counts: tiles arriving as the view zooms in, and
/// a redraw after a change, keep the old picture on screen, so the page
/// stays `ready`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureState {
    /// `Idle`: the view doesn't show the page. `Loading`: it does, and no
    /// picture of the page is on screen yet. `Ready`: one is. `Forbidden`:
    /// the user may not see it (no `doc.render`). `Error`: it couldn't be
    /// drawn.
    pub status: ResourceStatus,
    /// Loading for longer than `slowAfterMs`: the moment to show an
    /// indicator.
    pub slow: bool,
}

impl PictureState {
    pub const IDLE: PictureState = PictureState { status: ResourceStatus::Idle, slow: false };

    fn of(status: ResourceStatus, slow: bool) -> PictureState {
        PictureState { status, slow: status == ResourceStatus::Loading && slow }
    }
}

impl Serialize for PictureState {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_struct("PictureState", 2)?;
        out.serialize_field("status", self.status.as_str())?;
        out.serialize_field("slow", &self.slow)?;
        out.end()
    }
}

/// `PictureStateChangedEvent`: a page's picture came on screen, failed,
/// went slow, or the page left the view.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureStateChangedEvent {
    pub page: PageRef,
    pub view: String,
    pub state: PictureState,
}

impl Serialize for PictureStateChangedEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        struct Page<'a>(&'a PageRef);
        impl Serialize for Page<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                ser_page(self.0, s)
            }
        }
        let mut out = s.serialize_struct("PictureStateChangedEvent", 4)?;
        out.serialize_field("page", &Page(&self.page))?;
        out.serialize_field("view", &self.view)?;
        out.serialize_field("status", self.state.status.as_str())?;
        out.serialize_field("slow", &self.state.slow)?;
        out.end()
    }
}

/// `PictureReport`: what paints a page's picture tells the render plugin,
/// so the page's [`PictureState`] knows.
pub trait PictureReport {
    /// A picture of the page is on screen now: it has been presented.
    fn shown(&self);
    /// The picture can't be painted, and why: a `permission-denied` error
    /// (a `PluginError`'s canonical `{"$error": ...}`, or a plain object with
    /// that `code`) makes the page `forbidden`, any other `error`. Ignored
    /// once a picture showed, which stays on screen.
    fn failed(&self, error: &Value);
}

/// A refusal for want of a permission, as a `PluginError` or its plain info.
fn is_refusal(error: &Value) -> bool {
    let code =
        |value: &Value| value.get("code").and_then(Value::as_str) == Some("permission-denied");
    error.get("$error").is_some_and(code) || code(error)
}

/// What one report said: its picture showed, or why it can't.
struct Report {
    shown: Cell<bool>,
    failure: Cell<Option<ResourceStatus>>,
}

/// A page some view shows, and the reports on it.
struct ShownPage {
    page: PageRef,
    reports: RefCell<Vec<Rc<Report>>>,
    slow: Cell<bool>,
    /// The pending turn to `slow`, while it waits.
    cancel_slow: RefCell<Option<Cancel>>,
    /// What it reads now.
    state: Cell<PictureState>,
}

/// One picture shown is enough; otherwise a failure, or still loading.
fn status_of(shown: &ShownPage) -> ResourceStatus {
    let mut failure = None;
    for report in shown.reports.borrow().iter() {
        if report.shown.get() {
            return ResourceStatus::Ready;
        }
        let this = report.failure.get();
        if this == Some(ResourceStatus::Forbidden) || (this.is_some() && failure.is_none()) {
            failure = this;
        }
    }
    failure.unwrap_or(ResourceStatus::Loading)
}

/// What the reports need of the plugin.
pub(crate) struct ReportsDeps {
    /// `ctx.notify()`.
    pub notify: Rc<dyn Fn()>,
    /// The `onPictureStateChanged` source's `emit`.
    pub changed: Rc<dyn Fn(&PictureStateChangedEvent)>,
    /// `ctx.settings().get().slowAfterMs`.
    pub slow_after_ms: Rc<dyn Fn() -> f64>,
    /// `ctx.clock.after`.
    pub after: Rc<dyn Fn(f64, Box<dyn FnOnce()>) -> Cancel>,
}

struct Inner {
    views: RefCell<HashMap<String, IndexMap<u32, Rc<ShownPage>>>>,
    /// Layers unmount after the document closed: their last reports change
    /// nothing then.
    closed: Cell<bool>,
    deps: ReportsDeps,
}

/// `createPictureReports(ctx, changed)`.
#[derive(Clone)]
pub(crate) struct PictureReports(Rc<Inner>);

impl PictureReports {
    pub(crate) fn new(deps: ReportsDeps) -> PictureReports {
        PictureReports(Rc::new(Inner {
            views: RefCell::new(HashMap::new()),
            closed: Cell::new(false),
            deps,
        }))
    }

    /// The document closed (`ctx.cleanup`): every pending turn to `slow`
    /// stops, and later reports change nothing.
    pub(crate) fn close(&self) {
        self.0.closed.set(true);
        let views = std::mem::take(&mut *self.0.views.borrow_mut());
        for pages in views.values() {
            for shown in pages.values() {
                if let Some(cancel) = shown.cancel_slow.borrow_mut().take() {
                    cancel.call();
                }
            }
        }
    }

    /// A page's state in a view: idle when the view doesn't show it.
    pub(crate) fn state_in(&self, view: &str, page_object_number: u32) -> PictureState {
        self.0
            .views
            .borrow()
            .get(view)
            .and_then(|pages| pages.get(&page_object_number))
            .map_or(PictureState::IDLE, |shown| shown.state.get())
    }

    /// `open(view, page)`: a report on a page's picture in a view.
    pub(crate) fn open(&self, view: &str, page: PageRef) -> OpenPictureReport {
        let report = Rc::new(Report { shown: Cell::new(false), failure: Cell::new(None) });
        if self.0.closed.get() {
            return OpenPictureReport { reports: None, view: view.into(), target: None, report };
        }
        let target = {
            let mut views = self.0.views.borrow_mut();
            let pages = views.entry(view.to_string()).or_default();
            pages
                .entry(page.page_object_number)
                .or_insert_with(|| {
                    Rc::new(ShownPage {
                        page,
                        reports: RefCell::new(Vec::new()),
                        slow: Cell::new(false),
                        cancel_slow: RefCell::new(None),
                        state: Cell::new(PictureState::IDLE),
                    })
                })
                .clone()
        };
        target.reports.borrow_mut().push(report.clone());
        publish(&self.0, view, &target);
        OpenPictureReport {
            reports: Some(Rc::downgrade(&self.0)),
            view: view.to_string(),
            target: Some(target),
            report,
        }
    }
}

/// Fold a page's reports into its state, and tell the readers when that
/// changed.
fn publish(inner: &Rc<Inner>, view: &str, shown: &Rc<ShownPage>) {
    let status =
        if shown.reports.borrow().is_empty() { ResourceStatus::Idle } else { status_of(shown) };
    if status != ResourceStatus::Loading {
        if let Some(cancel) = shown.cancel_slow.borrow_mut().take() {
            cancel.call();
        }
        shown.slow.set(false);
    } else if !shown.slow.get() && shown.cancel_slow.borrow().is_none() {
        let delay = (inner.deps.slow_after_ms)();
        if delay <= 0.0 {
            shown.slow.set(true);
        } else {
            let (weak, page, view_id) =
                (Rc::downgrade(inner), Rc::downgrade(shown), view.to_string());
            let cancel = (inner.deps.after)(
                delay,
                Box::new(move || {
                    let (Some(inner), Some(shown)) = (weak.upgrade(), page.upgrade()) else {
                        return;
                    };
                    shown.cancel_slow.borrow_mut().take();
                    shown.slow.set(true);
                    publish(&inner, &view_id, &shown);
                }),
            );
            *shown.cancel_slow.borrow_mut() = Some(cancel);
        }
    }
    let next = PictureState::of(status, shown.slow.get());
    if shown.state.get() == next {
        return;
    }
    shown.state.set(next);
    (inner.deps.notify)();
    (inner.deps.changed)(&PictureStateChangedEvent {
        page: shown.page,
        view: view.to_string(),
        state: next,
    });
}

/// `OpenPictureReport`: a report a page layer keeps on the page it paints:
/// [`close`](Self::close) it when the layer goes. A report opened after the
/// document closed does nothing.
pub struct OpenPictureReport {
    reports: Option<Weak<Inner>>,
    view: String,
    target: Option<Rc<ShownPage>>,
    report: Rc<Report>,
}

impl OpenPictureReport {
    /// The reports, while this report is open on a live document.
    fn live(&self) -> Option<(Rc<Inner>, &Rc<ShownPage>)> {
        let inner = self.reports.as_ref()?.upgrade()?;
        let target = self.target.as_ref()?;
        let open = target.reports.borrow().iter().any(|r| Rc::ptr_eq(r, &self.report));
        (open && !inner.closed.get()).then_some((inner, target))
    }

    /// Every report on the page closed: it is `idle` in the view again.
    pub fn close(&self) {
        let Some((inner, target)) = self.live() else { return };
        target.reports.borrow_mut().retain(|r| !Rc::ptr_eq(r, &self.report));
        if target.reports.borrow().is_empty() {
            let mut views = inner.views.borrow_mut();
            if let Some(pages) = views.get_mut(&self.view)
                && pages.get(&target.page.page_object_number).is_some_and(|p| Rc::ptr_eq(p, target))
            {
                pages.shift_remove(&target.page.page_object_number);
            }
        }
        publish(&inner, &self.view, target);
    }
}

impl PictureReport for OpenPictureReport {
    fn shown(&self) {
        let Some((inner, target)) = self.live() else { return };
        if self.report.shown.get() {
            return;
        }
        self.report.shown.set(true);
        self.report.failure.set(None);
        publish(&inner, &self.view, target);
    }

    fn failed(&self, error: &Value) {
        let Some((inner, target)) = self.live() else { return };
        if self.report.shown.get() {
            return;
        }
        let failure =
            if is_refusal(error) { ResourceStatus::Forbidden } else { ResourceStatus::Error };
        self.report.failure.set(Some(failure));
        publish(&inner, &self.view, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_refusal_is_a_permission_denied_code() {
        assert!(is_refusal(&json!({ "$error": { "code": "permission-denied" } })));
        assert!(is_refusal(&json!({ "code": "permission-denied" })));
        assert!(!is_refusal(&json!({ "$error": { "code": "not-found" } })));
        assert!(!is_refusal(&json!("permission-denied")));
    }

    #[test]
    fn a_state_serializes_as_the_typescript_object() {
        let state = PictureState::of(ResourceStatus::Loading, true);
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            json!({ "status": "loading", "slow": true })
        );
        assert_eq!(
            PictureState::of(ResourceStatus::Ready, true),
            PictureState { status: ResourceStatus::Ready, slow: false }
        );
    }
}
