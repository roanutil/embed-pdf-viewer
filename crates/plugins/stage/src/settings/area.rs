//! `settings/settings.ts`: the settings bag and its responsive resolution.
//! The base (config plus runtime setters) with the matching rules' patches
//! over it is the effective settings the reducer holds. `apply_settings` is
//! the one reactive applier: the strongest effect among the touched
//! settings wins (`reflow` over `scene` and `refit` over `reclamp` over
//! `none`), so a breakpoint crossing gets the invariant a hand-written
//! update would.

use crate::prelude::*;

impl StageCapability {
    pub(crate) fn snapshot_settings(&self) -> StageSettings {
        self.st().settings.clone()
    }

    /// A settings change, reacted per the registry: the strongest effect
    /// among the touched settings wins.
    pub(crate) fn apply_settings(&self, patch: PartialSettings) -> Result<(), Error> {
        self.cancel_anim();
        let touched = |effect: SettingEffect| {
            SETTING_KEYS.iter().any(|&k| patch_has(&patch, k) && settings_effect(k) == effect)
        };
        let align = if touched(SettingEffect::Refit)
            && !touched(SettingEffect::Scene)
            && !touched(SettingEffect::Reflow)
        {
            self.st().settings.zoom_align
        } else {
            self.st().settings.anchor_align
        };
        let anchor = self.anchor_at(self.align_point(align, self.vp()))?;
        let scene = touched(SettingEffect::Scene);
        let reflow = touched(SettingEffect::Reflow);
        let refit = touched(SettingEffect::Refit);
        let reclamp = touched(SettingEffect::Reclamp);
        self.dispatch(StageAction::Patch { patch });
        if scene {
            self.invalidate_scene();
        }
        if reflow {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else if scene || refit {
            self.reapply(anchor, Some(align));
        } else if reclamp {
            self.set_cam_current()?;
        }
        Ok(())
    }

    // ── responsive settings ───────────────────────────────────────────────────

    pub(crate) fn publish_active(&self, active: Vec<String>) {
        let prev = &self.st().active_rules;
        if prev.len() != active.len() || active.iter().zip(prev.iter()).any(|(n, p)| n != p) {
            self.dispatch(StageAction::Responsive { active });
        }
    }

    /// Re-resolve and apply what changed since the last resolution.
    pub(crate) fn sync_responsive(&self, react: bool) -> Result<SettingEffect, Error> {
        let rules = self.inner.rules.borrow().clone();
        let resolved = resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.vp()));
        let patch = diff_settings(&resolved.effective, &self.inner.last_effective.borrow());
        *self.inner.last_effective.borrow_mut() = resolved.effective;
        self.publish_active(resolved.active);
        let keys = patch_keys(&patch);
        if keys.is_empty() {
            return Ok(SettingEffect::None);
        }
        if react {
            self.apply_settings(patch)?;
            return Ok(SettingEffect::None);
        }
        self.dispatch(StageAction::Patch { patch });
        let strongest =
            keys.iter().map(|&k| settings_effect(k)).max().unwrap_or(SettingEffect::None);
        if strongest == SettingEffect::Scene || strongest == SettingEffect::Reflow {
            self.invalidate_scene();
        }
        Ok(strongest)
    }
}

/// The keys whose effective value changed, carrying the new value.
fn diff_settings(effective: &StageSettings, last: &StageSettings) -> PartialSettings {
    let mut patch = PartialSettings::default();
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if !eq_setting(&effective.$field, &last.$field) {
                patch.$field = Some(effective.$field.clone());
            }
        };
    }
    crate::for_each_setting!(arm);
    patch
}

// ── the area's api ────────────────────────────────────────────────────────────

impl StageCapability {
    /// Every live setting (build or save a preset).
    pub fn get_settings(&self) -> StageSettings {
        self.snapshot_settings()
    }

    /// Back to the constructed config.
    pub fn reset_settings(&self) -> Result<(), Error> {
        *self.inner.base.borrow_mut() = self.inner.initial_base.clone();
        self.sync_responsive(true).map(|_| ())
    }

    /// Patch any subset in one anchor-preserving update. Writes the
    /// responsive base: a matching rule's key wins until its rule stops
    /// matching.
    pub fn update_settings(&self, patch: PartialSettings) -> Result<(), Error> {
        let merged = merge_settings(&self.inner.base.borrow(), &patch);
        *self.inner.base.borrow_mut() = merged;
        self.sync_responsive(true).map(|_| ())
    }

    /// Replace the container-query rules and re-resolve.
    pub fn set_responsive_rules(&self, rules: Vec<ResponsiveRule>) -> Result<(), Error> {
        *self.inner.rules.borrow_mut() = Rc::new(rules);
        self.sync_responsive(true).map(|_| ())
    }

    /// Is the named rule matching?
    pub fn matches_rule(&self, name: &str) -> bool {
        self.st().active_rules.iter().any(|n| n == name)
    }

    /// Names of the matching rules, in source order.
    pub fn list_active_rules(&self) -> Vec<String> {
        self.st().active_rules.clone()
    }

    pub fn set_flow(&self, flow: FlowMode) -> Result<(), Error> {
        self.update_settings(PartialSettings { flow: Some(flow), ..Default::default() })
    }

    pub fn set_layout(&self, layout: LayoutKind) -> Result<(), Error> {
        self.update_settings(PartialSettings { layout: Some(layout), ..Default::default() })
    }

    pub fn set_spread(&self, spread: sc::SpreadMode) -> Result<(), Error> {
        self.update_settings(PartialSettings { spread: Some(spread), ..Default::default() })
    }

    pub fn set_sizing(&self, sizing: sc::SizingMode) -> Result<(), Error> {
        self.update_settings(PartialSettings { sizing: Some(sizing), ..Default::default() })
    }

    /// Restore a saved view: it writes the base and the rules re-assert on
    /// top, in one PATCH, then the cursor and the anchor.
    pub fn apply_view_state(&self, view: StageViewState) {
        self.cancel_anim();
        let merged = merge_settings(
            &self.inner.base.borrow(),
            &PartialSettings::from(view.settings.clone()),
        );
        *self.inner.base.borrow_mut() = merged;
        let rules = self.inner.rules.borrow().clone();
        let resolved = resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.vp()));
        *self.inner.last_effective.borrow_mut() = resolved.effective.clone();
        self.publish_active(resolved.active);
        self.dispatch(StageAction::Patch { patch: PartialSettings::from(resolved.effective) });
        self.dispatch(StageAction::Cursor { cursor: view.cursor.unwrap_or(0) });
        self.invalidate_scene();
        self.apply_anchor(view.anchor, None);
    }
}
