//! `view/lifecycle.ts`: the view lifecycle. The viewport report (initial
//! placement is level-triggered here: the moment the stage first learns a
//! real size and the document has pages), the device pixel ratio, the
//! initial-view providers resolved once by priority, the refit after
//! page-registry mutations, and the view memento reads.

use epdf_kernel::Unsubscribe;

use crate::prelude::*;

impl StageCapability {
    /// Report the viewport. The first report with both axes positive and a
    /// document with pages runs initial placement; later ones keep the
    /// anchor and re-resolve fit modes.
    pub fn set_viewport_size(&self, v: Size) {
        if !self.inner.placement_started.get() {
            self.dispatch(StageAction::Vp { vp: v });
            // Rules see the box before placement resolves. With react false
            // nothing clamps, so this cannot fail.
            let _ = self.sync_responsive(false);
            if v.width > 0.0 && v.height > 0.0 && self.page_count_now() > 0 {
                self.place_initial();
            }
            return;
        }
        self.cancel_anim();
        let anchor = self.current_anchor(); // against the old viewport
        self.dispatch(StageAction::Vp { vp: v });
        let fx = self.sync_responsive(false).unwrap_or(SettingEffect::None);
        if fx == SettingEffect::Reflow {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else {
            self.reapply(anchor, None);
        }
    }

    /// Report the device pixel ratio. Only a positive, changed ratio lands.
    pub fn set_device_pixel_ratio(&self, ratio: f64) {
        if ratio > 0.0 && ratio != self.dpr() {
            self.dispatch(StageAction::Dpr { dpr: ratio });
        }
    }

    /// Re-resolve the zoom intent against the current scene, keeping the
    /// anchor. A no-op before the first placement. The watchers call it on
    /// every page-registry revision.
    pub fn refit(&self) {
        if !self.inner.placement_started.get() {
            return;
        }
        self.cancel_anim();
        self.reapply(self.current_anchor(), None);
    }

    /// Serializable view state: the unit of session persistence.
    pub fn get_view_state(&self) -> StageViewState {
        StageViewState {
            settings: self.snapshot_settings(),
            cursor: Some(self.st().cursor),
            anchor: self.current_anchor(),
        }
    }

    /// This lens's identity: the stage plugin id it registered under.
    /// Pointer samples are stamped with it.
    pub fn get_lens_id(&self) -> String {
        self.ctx.id().to_string()
    }

    /// Offer a candidate initial view; the highest priority non-null one
    /// wins at placement. The returned unsubscribe withdraws it.
    pub fn provide_initial_view(
        &self,
        priority: f64,
        provider: impl Fn() -> Option<StageViewState> + 'static,
    ) -> Unsubscribe {
        let id = self.inner.next_provider.get();
        self.inner.next_provider.set(id + 1);
        self.inner.providers.borrow_mut().push((id, priority, Rc::new(provider)));
        let inner = Rc::downgrade(&self.inner);
        Unsubscribe::new(move || {
            if let Some(inner) = inner.upgrade() {
                inner.providers.borrow_mut().retain(|(i, _, _)| *i != id);
            }
        })
    }

    /// Resolve the providers once (else reset), then publish PLACED.
    pub fn place_initial(&self) {
        self.inner.placement_started.set(true);
        let mut sorted = self.inner.providers.borrow().clone();
        // `Array#sort` with `(a, b) => b.priority - a.priority`: stable.
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (_, _, provider) in sorted {
            if let Some(view) = provider() {
                self.apply_view_state(view);
                self.dispatch(StageAction::Placed);
                return;
            }
        }
        self.reset_view();
        self.dispatch(StageAction::Placed);
    }
}
