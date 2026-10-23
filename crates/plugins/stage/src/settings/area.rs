//! `settings/settings.ts`: the settings bag and its responsive resolution.
//! The base (config plus `update_settings` changes) merged with the
//! matching rules' patches is the effective settings the state holds.
//! `apply_settings` is the one reactive applier: the strongest effect among
//! the touched settings wins (`reflow` over `scene` and `refit` over
//! `reclamp` over `none`), so a breakpoint crossing gets exactly the
//! invariant a hand-written update would.

use crate::model::{patch_settings, set_active_rules, set_cursor};
use crate::prelude::*;
use crate::settings::pick_settings;

impl StageCapability {
    /// `snapshotSettings`: the settings slice of the state. The TypeScript
    /// memoizes it so readers can compare by identity; a Rust reader
    /// compares the value.
    pub(crate) fn snapshot_settings(&self) -> StageSettings {
        pick_settings(&self.st().settings)
    }

    /// React to a settings change per the registry: the strongest effect
    /// among the touched settings wins. Both `update_settings` and the
    /// responsive driver land here.
    pub(crate) fn apply_settings(&self, patch: StageSettingsPatch) -> Result<(), Error> {
        self.cancel_animation();
        let touched = |effect: SettingEffect| {
            SETTING_KEYS.iter().any(|&k| patch_has(&patch, k) && settings_effect(k) == effect)
        };
        // The change's invariant point: a pure zoom-intent change holds the
        // zoomAlign focal point; every other reframe the anchorAlign one.
        let align = if touched(SettingEffect::Refit)
            && !touched(SettingEffect::Scene)
            && !touched(SettingEffect::Reflow)
        {
            self.st().settings.zoom_align
        } else {
            self.st().settings.anchor_align
        };
        // Captured against the current scene.
        let anchor = self.anchor_at(self.align_point(align, self.viewport()))?;
        let scene = touched(SettingEffect::Scene);
        let reflow = touched(SettingEffect::Reflow);
        let refit = touched(SettingEffect::Refit);
        let reclamp = touched(SettingEffect::Reclamp);
        self.update(|s| patch_settings(s, &patch));
        if scene {
            self.invalidate_scene();
        }
        if reflow {
            // The flow toggled: re-place onto the cursor's page under the
            // new flow's scene.
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else if scene || refit {
            self.reapply(anchor, Some(align));
        } else if reclamp {
            // The clamp policy changed: re-clamp the current camera.
            self.write_camera_stay(self.camera_object())?;
        }
        Ok(())
    }

    // ── responsive settings ──

    fn publish_active(&self, active: Vec<String>) {
        let current = &self.st().active_rules;
        if current.len() != active.len() || active.iter().zip(current.iter()).any(|(n, c)| n != c) {
            self.update(|s| set_active_rules(s, active));
        }
    }

    /// Re-resolve and diff against the last resolution: the patch of what
    /// changed and its keys (`syncResponsive`'s first half).
    fn resolve_changes(&self) -> (StageSettingsPatch, Vec<SettingKey>) {
        let rules = self.inner.rules.borrow().clone();
        let resolved =
            resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.viewport()));
        let patch = diff_settings(&resolved.effective, &self.inner.last_effective.borrow());
        *self.inner.last_effective.borrow_mut() = resolved.effective;
        self.publish_active(resolved.active);
        let keys = patch_keys(&patch);
        (patch, keys)
    }

    /// `syncResponsive(true)`: re-resolve and route what changed through
    /// `apply_settings`, the full anchor-preserving update.
    pub(crate) fn sync_responsive(&self) -> Result<(), Error> {
        let (patch, keys) = self.resolve_changes();
        if keys.is_empty() {
            return Ok(());
        }
        self.apply_settings(patch)
    }

    /// `syncResponsive(false)`, inside `set_viewport_size`, whose caller
    /// runs its own reframe: only the patch and the cache invalidation land,
    /// and the strongest effect is reported so the caller can escalate a
    /// rule-driven flow flip. Nothing here clamps, so nothing can fail.
    pub(crate) fn sync_responsive_quietly(&self) -> SettingEffect {
        let (patch, keys) = self.resolve_changes();
        if keys.is_empty() {
            return SettingEffect::None;
        }
        self.update(|s| patch_settings(s, &patch));
        let strongest =
            keys.iter().map(|&k| settings_effect(k)).max().unwrap_or(SettingEffect::None);
        if strongest == SettingEffect::Scene || strongest == SettingEffect::Reflow {
            self.invalidate_scene();
        }
        strongest
    }
}

/// The keys whose effective value changed, carrying the new value.
fn diff_settings(effective: &StageSettings, last: &StageSettings) -> StageSettingsPatch {
    let mut patch = StageSettingsPatch::default();
    macro_rules! arm {
        (PageFrame, $field:ident, $effect:ident) => {
            if !eq_setting(&effective.$field, &last.$field) {
                patch.$field = Some(effective.$field.into());
            }
        };
        ($key:ident, $field:ident, $effect:ident) => {
            if !eq_setting(&effective.$field, &last.$field) {
                patch.$field = Some(effective.$field.clone());
            }
        };
    }
    crate::for_each_setting!(arm);
    patch
}

// ── the area's api ──

impl StageCapability {
    /// Every live setting (build or save a preset).
    pub fn get_settings(&self) -> StageSettings {
        self.snapshot_settings()
    }

    /// Back to the settings the plugin was registered with. Fires
    /// `on_settings_changed`.
    pub fn reset_settings(&self) -> Result<(), Error> {
        *self.inner.base.borrow_mut() = self.inner.initial_base.clone();
        self.sync_responsive()
    }

    /// Change any subset in one update that keeps the view on the same
    /// page. Each value replaces the current one whole; a matching
    /// responsive rule still wins on the settings it names.
    pub fn update_settings(&self, changes: StageSettingsPatch) -> Result<(), Error> {
        let merged = merge_settings(&self.inner.base.borrow(), &changes);
        *self.inner.base.borrow_mut() = merged;
        self.sync_responsive()
    }

    /// Replace the container-query rules and re-resolve.
    pub fn set_responsive_rules(&self, rules: Vec<ResponsiveRule>) -> Result<(), Error> {
        *self.inner.rules.borrow_mut() = Rc::new(rules);
        self.sync_responsive()
    }

    /// Whether the named responsive rule matches the current box.
    pub fn matches_rule(&self, name: &str) -> bool {
        self.st().active_rules.iter().any(|n| n == name)
    }

    /// Names of the responsive rules matching the current box, in source
    /// order.
    pub fn list_active_rules(&self) -> Vec<String> {
        self.st().active_rules.clone()
    }

    /// Restore a saved view state: it writes the base and the rules
    /// re-assert on top, in one patch, then the cursor and the anchor.
    pub fn apply_view_state(&self, view: impl Into<StageViewPatch>) {
        let view = view.into();
        self.cancel_animation();
        let merged = merge_settings(&self.inner.base.borrow(), &view.settings);
        *self.inner.base.borrow_mut() = merged;
        let rules = self.inner.rules.borrow().clone();
        let resolved =
            resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.viewport()));
        *self.inner.last_effective.borrow_mut() = resolved.effective.clone();
        self.publish_active(resolved.active);
        let effective = StageSettingsPatch::from(resolved.effective);
        self.update(|s| patch_settings(s, &effective));
        let cursor = view.cursor.unwrap_or(0);
        self.update(|s| set_cursor(s, cursor));
        self.invalidate_scene();
        self.apply_anchor(view.anchor, None);
    }
}
