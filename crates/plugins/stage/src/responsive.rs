//! `responsive.ts`: container queries for the settings bag, the pure half.
//! What a box is, when a query matches, and what the effective settings
//! are. The capability owns the driver.

use epdf_geometry::Size;

use crate::contract::*;
use crate::for_each_setting;

/// The reported viewport as a queryable box. A square box is portrait.
pub fn box_of(vp: Size) -> StageBox {
    StageBox {
        width: vp.width,
        height: vp.height,
        orientation: if vp.height >= vp.width {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        },
    }
}

/// All bounds inclusive, all fields optional, conditions AND-ed.
pub fn matches_query(q: &BoxQuery, bx: &StageBox) -> bool {
    q.min_width.is_none_or(|v| bx.width >= v)
        && q.max_width.is_none_or(|v| bx.width <= v)
        && q.min_height.is_none_or(|v| bx.height >= v)
        && q.max_height.is_none_or(|v| bx.height <= v)
        && q.orientation.is_none_or(|o| bx.orientation == o)
}

fn rule_matches(rule: &ResponsiveRule, bx: &StageBox) -> bool {
    match &rule.when {
        RuleWhen::Predicate(when) => when(bx),
        RuleWhen::Query(q) => matches_query(q, bx),
    }
}

/// Merge a patch over settings, skipping absent values.
pub fn merge_settings(into: &StageSettings, patch: &PartialSettings) -> StageSettings {
    let mut out = into.clone();
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if let Some(value) = &patch.$field {
                out.$field = value.clone();
            }
        };
    }
    for_each_setting!(arm);
    out
}

/// Setting-value equality. The TypeScript compares primitives by identity
/// and the flat objects the vocabulary uses (`{px}`, `{x, y}`, page frames,
/// zoom specs) by one level of own keys; on the typed values that is
/// `PartialEq`, `NaN` unequal to itself included.
pub fn eq_setting<T: PartialEq>(a: &T, b: &T) -> bool {
    a == b
}

/// Base plus every matching rule's patch, and the matching names.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedResponsive {
    pub effective: StageSettings,
    pub active: Vec<String>,
}

pub fn resolve_responsive(
    base: &StageSettings,
    rules: &[ResponsiveRule],
    bx: &StageBox,
) -> ResolvedResponsive {
    let mut effective = base.clone();
    let mut active = Vec::new();
    for rule in rules {
        if !rule_matches(rule, bx) {
            continue;
        }
        if let Some(name) = &rule.name {
            active.push(name.clone());
        }
        if let Some(settings) = &rule.settings {
            effective = merge_settings(&effective, settings);
        }
    }
    ResolvedResponsive { effective, active }
}
