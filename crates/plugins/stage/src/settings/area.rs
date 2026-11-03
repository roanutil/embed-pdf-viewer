//! `settings/settings.ts`: the settings bag and its resolution. The base
//! (config plus `update_settings` changes), then the matching rules'
//! patches, then the locks, is the effective settings the state holds.
//! `apply_settings` is the one reactive applier: the strongest effect among
//! the touched settings wins (`reflow` over `scene` and `refit` over
//! `reclamp` over `none`), so a breakpoint crossing gets exactly the
//! invariant a hand-written update would.

use epdf_kernel::Unsubscribe;
use epdf_kernel::error::plugin_error;

use crate::model::{StageLocks, patch_settings, set_active_rules, set_cursor, set_locks};
use crate::prelude::*;
use crate::responsive::ResolvedResponsive;
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

    /// Base, then the matching rules, then the locks: what the state holds.
    fn resolve(&self) -> ResolvedResponsive {
        let rules = self.inner.rules.borrow().clone();
        let mut resolved =
            resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.viewport()));
        for lock in self.inner.locks.borrow().iter() {
            resolved.effective = merge_settings(&resolved.effective, &lock.settings);
        }
        resolved
    }

    /// Who holds each locked setting, in state: the first lock that names it.
    fn publish_locks(&self) {
        let mut holders = StageLocks::default();
        for lock in self.inner.locks.borrow().iter() {
            for key in patch_keys(&lock.settings) {
                if holders.get(key).is_none() {
                    let holder =
                        SettingLock { by: lock.by.clone(), reason_key: lock.reason_key.clone() };
                    holders.0.push((key, holder));
                }
            }
        }
        let current = &self.st().locks;
        let same = holders.0.len() == current.0.len()
            && holders.0.iter().all(|(key, holder)| current.get(*key) == Some(holder));
        if !same {
            self.update(|s| set_locks(s, holders));
        }
    }

    /// Re-resolve and diff against the last resolution: the patch of what
    /// changed and its keys (`syncResponsive`'s first half). `asked` names
    /// the settings a caller just set. Each of them lands whenever the live
    /// value differs, even if the resolution itself didn't move: a zoom
    /// step, a pinch or a zoom box records its level straight into the
    /// state, so the resolution can still hold the fit mode the user now
    /// asks for again.
    fn resolve_changes(&self, asked: &[SettingKey]) -> (StageSettingsPatch, Vec<SettingKey>) {
        let resolved = self.resolve();
        let live = self.st();
        let patch = diff_settings(
            &resolved.effective,
            &self.inner.last_effective.borrow(),
            &live.settings,
            asked,
        );
        *self.inner.last_effective.borrow_mut() = resolved.effective;
        self.publish_active(resolved.active);
        let keys = patch_keys(&patch);
        (patch, keys)
    }

    /// `syncResponsive(true, asked)`: re-resolve and route what changed
    /// through `apply_settings`, the full anchor-preserving update.
    pub(crate) fn sync_responsive(&self, asked: &[SettingKey]) -> Result<(), Error> {
        let (patch, keys) = self.resolve_changes(asked);
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
        let (patch, keys) = self.resolve_changes(&[]);
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

/// The keys whose effective value changed since the last resolution, and
/// the asked ones whose live value differs, carrying the effective value.
fn diff_settings(
    effective: &StageSettings,
    last: &StageSettings,
    live: &StageSettings,
    asked: &[SettingKey],
) -> StageSettingsPatch {
    let mut patch = StageSettingsPatch::default();
    macro_rules! lands {
        ($key:ident, $field:ident) => {
            !eq_setting(&effective.$field, &last.$field)
                || (asked.contains(&SettingKey::$key)
                    && !eq_setting(&effective.$field, &live.$field))
        };
    }
    macro_rules! arm {
        (PageFrame, $field:ident, $effect:ident) => {
            if lands!(PageFrame, $field) {
                patch.$field = Some(effective.$field.into());
            }
        };
        (ZoomBox, $field:ident, $effect:ident) => {
            if lands!(ZoomBox, $field) {
                patch.$field = Some(effective.$field.clone().into());
            }
        };
        ($key:ident, $field:ident, $effect:ident) => {
            if lands!($key, $field) {
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
        self.sync_responsive(&SETTING_KEYS)
    }

    /// Change any subset in one update that keeps the view on the same
    /// page. Each value replaces the current one whole; a matching
    /// responsive rule still wins on the settings it names.
    pub fn update_settings(&self, changes: StageSettingsPatch) -> Result<(), Error> {
        let merged = merge_settings(&self.inner.base.borrow(), &changes);
        *self.inner.base.borrow_mut() = merged;
        self.sync_responsive(&patch_keys(&changes))
    }

    /// Replace the container-query rules and re-resolve.
    pub fn set_responsive_rules(&self, rules: Vec<ResponsiveRule>) -> Result<(), Error> {
        *self.inner.rules.borrow_mut() = Rc::new(rules);
        self.sync_responsive(&[])
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
        let resolved = self.resolve();
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

// ── locks ──

impl StageCapability {
    /// The settings a saved view keeps: what shows, but the locked ones as
    /// they are underneath, so restoring it never writes a lock's value as
    /// the user's.
    pub(crate) fn view_state_settings(&self) -> StageSettings {
        let shown = self.snapshot_settings();
        if self.inner.locks.borrow().is_empty() {
            return shown;
        }
        let rules = self.inner.rules.borrow().clone();
        let unlocked =
            resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.viewport()))
                .effective;
        let mut settings = shown;
        for (key, _) in &self.st().locks.0 {
            copy_setting(&mut settings, &unlocked, *key);
        }
        settings
    }

    /// Hold `lock.settings` at their values for this document until the
    /// returned release is called, or the document closes. A lock on a
    /// setting another lock holds at a different value is refused
    /// (`conflict`), naming the holder; at the same value both hold, until
    /// both let go. `get_view_state` keeps the locked settings as they are
    /// underneath. The zoom can't be locked (`invalid-input`): gestures set
    /// it. A lock names its settings in setting order, where the TypeScript
    /// record keeps the caller's order.
    pub fn lock_settings(&self, lock: StageSettingsLock) -> Result<Unsubscribe, Error> {
        let keys = patch_keys(&lock.settings);
        for &key in &keys {
            if key == SettingKey::Zoom {
                return Err(plugin_error(
                    "invalid-input",
                    "stage",
                    "the zoom cannot be locked: gestures set it",
                    None,
                    None,
                ));
            }
            let holder = self.inner.locks.borrow().iter().find_map(|held| {
                (patch_has(&held.settings, key)
                    && !patch_value_eq(&held.settings, &lock.settings, key))
                .then(|| held.by.clone())
            });
            if let Some(by) = holder {
                return Err(plugin_error(
                    "conflict",
                    "stage",
                    format!("\"{}\" is locked by {by} at another value", serde_str(&key)),
                    None,
                    None,
                ));
            }
        }
        let held = Rc::new(lock);
        self.inner.locks.borrow_mut().push(held.clone());
        self.publish_locks();
        self.sync_responsive(&keys)?;
        let stage = self.clone();
        Ok(Unsubscribe::new(move || {
            stage.inner.locks.borrow_mut().retain(|other| !Rc::ptr_eq(other, &held));
            stage.publish_locks();
            // Rust only: the release can't throw; a failed reframe goes the
            // way of the TypeScript's throw out of the release function, which
            // a caller can't act on either.
            let _ = stage.sync_responsive(&keys);
        }))
    }

    /// Who holds a setting and why, or `None` when it isn't locked.
    pub fn get_lock(&self, setting: SettingKey) -> Option<SettingLock> {
        self.st().locks.get(setting).cloned()
    }
}

/// `settings[key] = from[key]` for one key.
fn copy_setting(settings: &mut StageSettings, from: &StageSettings, key: SettingKey) {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if key == SettingKey::$key {
                settings.$field = from.$field.clone();
            }
        };
    }
    crate::for_each_setting!(arm);
}

/// `eqSetting(a[key], b[key])` over two patches' values.
fn patch_value_eq(a: &StageSettingsPatch, b: &StageSettingsPatch, key: SettingKey) -> bool {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if key == SettingKey::$key {
                return eq_setting(&a.$field, &b.$field);
            }
        };
    }
    crate::for_each_setting!(arm);
    false
}
