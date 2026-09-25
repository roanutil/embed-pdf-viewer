//! `solver.ts`: the fit solver. Given a normalized bar, measured widths and
//! the container budget, decide what every unit renders as.
//!
//! The policy, the two-pass trigger fixpoint and the tie-breaking are the
//! TypeScript's, step for step; see that file's header. Widths are summed in
//! the same order, so a fractional result is the same `f64`.

use serde::{Deserialize, Serialize};

use crate::js::{self, JsMap};
use crate::schema::{NormalizedBar, NormalizedGroup, NormalizedUnit, PINNED, SectionName};

/// Widths the solver consults. `None` is a measurement not yet taken: it
/// counts as 0 and makes the result incomplete.
pub trait FitMetrics {
    /// Width of unit `key` rendered at `variant`.
    fn unit(&self, key: &str, variant: &str) -> Option<f64>;
    /// Width of group `id` in its collapsed form.
    fn group_collapsed(&self, group_id: &str) -> Option<f64>;
    /// Width of group `id`'s disclosure trigger.
    fn group_trigger(&self, group_id: &str) -> Option<f64>;
    /// Width of the overflow trigger button.
    fn overflow_trigger(&self) -> f64;
    /// Gap between adjacent visible units.
    fn gap(&self) -> f64;
    /// Width of a derived separator between adjacent visible groups.
    fn separator(&self) -> f64;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UnitAssignment {
    Variant {
        variant: String,
    },
    /// Hidden behind its group's disclosure trigger.
    Shed,
    /// Hidden behind its group's collapsed control.
    Collapsed,
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupAssignment {
    /// How many children sit behind the group's disclosure trigger.
    pub shed_count: usize,
    pub collapsed: bool,
    pub overflowed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FitResult {
    pub units: JsMap<UnitAssignment>,
    pub groups: JsMap<GroupAssignment>,
    pub has_overflow: bool,
    /// Total visible width at this assignment.
    #[serde(with = "js::number")]
    pub width: f64,
    /// False when any consulted measurement was missing.
    pub complete: bool,
}

struct UnitState<'a> {
    unit: &'a NormalizedUnit,
    variant_index: usize,
    shed: bool,
    overflowed: bool,
    position: usize,
}

struct GroupState<'a> {
    group: &'a NormalizedGroup,
    section: SectionName,
    collapsed: bool,
    overflowed: bool,
    units: Vec<UnitState<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepType {
    Variant,
    Shed,
    Collapse,
    OverflowUnit,
    OverflowGroup,
}

impl StepType {
    /// variant < shed < collapse < overflow when importance ties.
    fn rank(self) -> u8 {
        match self {
            StepType::Variant => 0,
            StepType::Shed => 1,
            StepType::Collapse => 2,
            StepType::OverflowUnit | StepType::OverflowGroup => 3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Step {
    kind: StepType,
    importance: u8,
    position: usize,
    group: usize,
    /// The unit's index in its group, for unit steps.
    unit: usize,
}

fn build_state(bar: &NormalizedBar) -> Vec<GroupState<'_>> {
    let mut groups = Vec::new();
    let mut position = 0;
    for section in &bar.sections {
        for g in &section.groups {
            let mut units = Vec::with_capacity(g.units.len());
            for u in &g.units {
                units.push(UnitState {
                    unit: u,
                    variant_index: 0,
                    shed: false,
                    overflowed: false,
                    position,
                });
                position += 1;
            }
            groups.push(GroupState {
                group: g,
                section: section.name,
                collapsed: false,
                overflowed: false,
                units,
            });
        }
    }
    groups
}

fn better(step: &Step, best: &Option<Step>) -> bool {
    let Some(best) = best else { return true };
    step.importance < best.importance
        || (step.importance == best.importance
            && (step.kind.rank() < best.kind.rank()
                || (step.kind.rank() == best.kind.rank() && step.position > best.position)))
}

fn next_step(groups: &[GroupState<'_>]) -> Option<Step> {
    let mut best: Option<Step> = None;
    let mut consider = |step: Step| {
        if better(&step, &best) {
            best = Some(step);
        }
    };

    for (gi, gs) in groups.iter().enumerate() {
        if gs.overflowed {
            continue;
        }
        let group_position = gs.units.last().map_or(0, |u| u.position);
        let group_step = |kind| Step {
            kind,
            importance: gs.group.importance,
            position: group_position,
            group: gi,
            unit: 0,
        };

        if gs.collapsed {
            // A collapsed group degrades as one unit: overflow it whole.
            if gs.group.importance < PINNED {
                consider(group_step(StepType::OverflowGroup));
            }
            continue;
        }

        let mut children_exhausted = true;
        let visible_count = gs.units.iter().filter(|u| !u.overflowed && !u.shed).count();
        for (ui, us) in gs.units.iter().enumerate() {
            if us.overflowed || us.shed {
                continue;
            }
            let unit_step = |kind| Step {
                kind,
                importance: us.unit.importance(),
                position: us.position,
                group: gi,
                unit: ui,
            };
            if us.variant_index + 1 < us.unit.variants().len() {
                children_exhausted = false;
                consider(unit_step(StepType::Variant));
            } else if us.unit.importance() < PINNED {
                if gs.group.shed && visible_count > 1 {
                    consider(unit_step(StepType::Shed));
                } else if !gs.group.shed && gs.group.collapse.is_none() {
                    consider(unit_step(StepType::OverflowUnit));
                }
            }
        }

        let shed_exhausted = !gs.group.shed || visible_count <= 1;
        if children_exhausted && shed_exhausted && gs.group.importance < PINNED {
            if gs.group.collapse.is_some() {
                consider(group_step(StepType::Collapse));
            } else if gs.group.shed {
                consider(group_step(StepType::OverflowGroup));
            }
        }
    }
    best
}

fn apply_step(groups: &mut [GroupState<'_>], step: Step) {
    let gs = &mut groups[step.group];
    match step.kind {
        StepType::Variant => gs.units[step.unit].variant_index += 1,
        StepType::Shed => gs.units[step.unit].shed = true,
        StepType::Collapse => gs.collapsed = true,
        StepType::OverflowUnit => {
            gs.units[step.unit].overflowed = true;
            if gs.units.iter().all(|u| u.overflowed) {
                gs.overflowed = true;
            }
        }
        StepType::OverflowGroup => {
            gs.overflowed = true;
            for u in &mut gs.units {
                u.overflowed = true;
            }
        }
    }
}

struct Measured {
    width: f64,
    complete: bool,
}

/// Visible width: unit widths, then the overflow trigger, then gaps between
/// all adjacent visible units, then separators per section, in the order
/// sections were first seen (a `Map` keyed by section name).
fn measure(groups: &[GroupState<'_>], metrics: &dyn FitMetrics, has_trigger: bool) -> Measured {
    let mut width = 0.0;
    let mut complete = true;
    let mut visible_units = 0usize;
    let mut visible_by_section: Vec<(SectionName, usize)> = Vec::new();
    let mut bump =
        |section: SectionName| match visible_by_section.iter_mut().find(|(s, _)| *s == section) {
            Some(entry) => entry.1 += 1,
            None => visible_by_section.push((section, 1)),
        };

    for gs in groups {
        if gs.overflowed {
            continue;
        }
        if gs.collapsed {
            let w = metrics.group_collapsed(&gs.group.id);
            if w.is_none() {
                complete = false;
            }
            width += w.unwrap_or(0.0);
            visible_units += 1;
            bump(gs.section);
            continue;
        }
        let mut group_visible = false;
        let mut shed_count = 0;
        for us in &gs.units {
            if us.overflowed {
                continue;
            }
            if us.shed {
                shed_count += 1;
                continue;
            }
            // An empty ladder (which `normalize_bar` rejects) has no variant
            // to measure: it counts as unmeasured.
            let w = us
                .unit
                .variants()
                .get(us.variant_index)
                .and_then(|variant| metrics.unit(us.unit.key(), variant));
            if w.is_none() {
                complete = false;
            }
            width += w.unwrap_or(0.0);
            visible_units += 1;
            group_visible = true;
        }
        if shed_count > 0 {
            let w = metrics.group_trigger(&gs.group.id);
            if w.is_none() {
                complete = false;
            }
            width += w.unwrap_or(0.0);
            visible_units += 1;
            group_visible = true;
        }
        if group_visible {
            bump(gs.section);
        }
    }

    if has_trigger {
        width += metrics.overflow_trigger();
        visible_units += 1;
    }
    if visible_units > 1 {
        width += metrics.gap() * (visible_units - 1) as f64;
    }
    for (_, count) in visible_by_section {
        if count > 1 {
            width += metrics.separator() * (count - 1) as f64;
        }
    }
    Measured { width, complete }
}

fn any_overflow(groups: &[GroupState<'_>]) -> bool {
    groups.iter().any(|gs| gs.units.iter().any(|u| u.overflowed))
}

fn solve_pass<'a>(
    bar: &'a NormalizedBar,
    metrics: &dyn FitMetrics,
    budget: f64,
    has_trigger: bool,
) -> (Vec<GroupState<'a>>, Measured) {
    let mut groups = build_state(bar);
    let mut measured = measure(&groups, metrics, has_trigger);
    while measured.width > budget {
        let Some(step) = next_step(&groups) else { break };
        apply_step(&mut groups, step);
        measured = measure(&groups, metrics, has_trigger);
    }
    (groups, measured)
}

pub fn solve(bar: &NormalizedBar, metrics: &dyn FitMetrics, container_width: f64) -> FitResult {
    let mut pass = solve_pass(bar, metrics, container_width, false);
    if any_overflow(&pass.0) {
        pass = solve_pass(bar, metrics, container_width, true);
    }
    let (groups, measured) = pass;

    let mut units = JsMap::new();
    let mut group_assignments = JsMap::new();
    for gs in &groups {
        let shed_count =
            gs.units.iter().filter(|u| u.shed && !u.overflowed && !gs.collapsed).count();
        group_assignments.set(
            gs.group.id.clone(),
            GroupAssignment { shed_count, collapsed: gs.collapsed, overflowed: gs.overflowed },
        );
        for us in &gs.units {
            let assignment = if us.overflowed {
                UnitAssignment::Overflow
            } else if gs.collapsed {
                UnitAssignment::Collapsed
            } else if us.shed {
                UnitAssignment::Shed
            } else {
                UnitAssignment::Variant {
                    variant: us.unit.variants().get(us.variant_index).cloned().unwrap_or_default(),
                }
            };
            units.set(us.unit.key(), assignment);
        }
    }
    FitResult {
        units,
        groups: group_assignments,
        has_overflow: any_overflow(&groups),
        width: measured.width,
        complete: measured.complete,
    }
}
