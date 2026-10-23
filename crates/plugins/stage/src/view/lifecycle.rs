//! `view/lifecycle.ts`: the view lifecycle. The viewport report (initial
//! placement is level-triggered here, the moment the stage first learns a
//! real size and the document has pages), the device pixel ratio, the
//! initial-view providers resolved once by priority, the refit after
//! page-registry changes, and the view memento reads.

use epdf_kernel::Unsubscribe;

use crate::model::{mark_placed, set_dpr, set_viewport};
use crate::prelude::*;

impl StageCapability {
    /// Report the container box. The first report with both axes positive
    /// and a document with pages runs initial placement. After that, a box
    /// with no area, or the size the stage already has, keeps the view;
    /// a real resize keeps the anchored page-point and re-resolves fit
    /// modes. `Err` where the TypeScript throws: no item under the anchor
    /// point.
    pub fn set_viewport_size(&self, size: Size) -> Result<(), Error> {
        if !self.inner.placement.started.get() {
            self.update(|s| set_viewport(s, size));
            // Rules see the box before placement resolves.
            self.sync_responsive_quietly();
            if size.width > 0.0 && size.height > 0.0 && self.page_count() > 0 {
                self.place_initial();
            }
            return Ok(());
        }
        // A report with no area means the view isn't shown: keep the last
        // real viewport and the view.
        if size.width <= 0.0 || size.height <= 0.0 {
            return Ok(());
        }
        // A report of the size the stage already has says nothing new.
        let current = self.st().viewport;
        if size.width == current.width && size.height == current.height {
            return Ok(());
        }
        self.cancel_animation();
        let anchor = self.current_anchor()?; // against the old viewport
        self.update(|s| set_viewport(s, size));
        // A breakpoint crossing rides the same reframe; only a rule-driven
        // flow flip escalates past reapply.
        let effect = self.sync_responsive_quietly();
        if effect == SettingEffect::Reflow {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else {
            self.reapply(anchor, None);
        }
        Ok(())
    }

    /// Report the device pixel ratio. Only a positive, changed ratio lands.
    pub fn set_device_pixel_ratio(&self, ratio: f64) {
        if ratio > 0.0 && ratio != self.dpr() {
            self.update(|s| set_dpr(s, ratio));
        }
    }

    /// Re-resolve the zoom intent against the current scene, keeping the
    /// anchor. A no-op before the first placement. `connect` calls it on
    /// every page-registry revision.
    pub fn refit(&self) -> Result<(), Error> {
        if !self.inner.placement.started.get() {
            return Ok(());
        }
        self.cancel_animation();
        self.reapply(self.current_anchor()?, None);
        Ok(())
    }

    /// Serializable view state: the unit of session persistence.
    pub fn get_view_state(&self) -> Result<StageViewState, Error> {
        Ok(StageViewState {
            settings: self.snapshot_settings(),
            cursor: Some(self.st().cursor),
            anchor: self.current_anchor()?,
        })
    }

    /// This lens's identity: the stage plugin id it registered under.
    pub fn get_lens_id(&self) -> String {
        self.ctx.id().to_string()
    }

    /// Offer a candidate initial view; the highest-priority non-null one
    /// wins at placement. The returned unsubscribe withdraws it.
    pub fn provide_initial_view<V: Into<StageViewPatch>>(
        &self,
        priority: f64,
        provider: impl Fn() -> Option<V> + 'static,
    ) -> Unsubscribe {
        let id = self.inner.next_provider.get();
        self.inner.next_provider.set(id + 1);
        let provider: Rc<dyn Fn() -> Option<StageViewPatch>> =
            Rc::new(move || provider().map(Into::into));
        self.inner.providers.borrow_mut().push((id, priority, provider));
        let inner = Rc::downgrade(&self.inner);
        Unsubscribe::new(move || {
            if let Some(inner) = inner.upgrade() {
                inner.providers.borrow_mut().retain(|(i, _, _)| *i != id);
            }
        })
    }

    /// Resolve the registered providers once (else reset), land the
    /// navigation asked for before placement, then publish `placed`.
    pub fn place_initial(&self) {
        self.inner.placement.started.set(true);
        let mut by_priority = self.inner.providers.borrow().clone();
        // `Array#sort` with `(left, right) => right.priority - left.priority`:
        // stable. A NaN priority makes that comparator inconsistent, and
        // neither language promises an order then (Rust's `sort_by` may even
        // panic on a non-total order past 20 entries); `total_cmp` would not
        // match V8's order either, so equal-on-NaN stays.
        by_priority.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut view = None;
        for (_, _, provider) in by_priority {
            view = provider();
            if view.is_some() {
                break;
            }
        }
        match view {
            Some(view) => self.apply_view_state(view),
            None => self.reset_view(),
        }
        // Navigation the app asked for before there was a viewport lands
        // last, from the initial view.
        let pending = self.inner.placement.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending(self);
        }
        // Publish renderability last.
        self.update(mark_placed);
    }
}
