//! `projection.ts`: the overflow menu, a group's shed menu and a strip, each
//! derived from the bar and the fit instead of authored.

use serde::{Deserialize, Serialize};

use crate::schema::{GroupRole, NormalizedBar, NormalizedGroup, NormalizedUnit};
use crate::solver::{FitResult, UnitAssignment};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OverflowRow {
    /// A plain command row.
    Command { command: String },
    /// A command that opens a menu.
    Submenu { command: String, menu: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionRole {
    Radio,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverflowSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    /// `radio` when the group is a tab strip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<SectionRole>,
    pub rows: Vec<OverflowRow>,
}

/// What `ResolveMenuTarget` returns for a known command: `{ menu?: string }`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<String>,
}

/// `ResolveMenuTarget`: `None` for an unknown command.
pub trait ResolveMenuTarget: Fn(&str) -> Option<MenuTarget> {}
impl<F: Fn(&str) -> Option<MenuTarget>> ResolveMenuTarget for F {}

fn project_unit(
    unit: &NormalizedUnit,
    resolve: &dyn Fn(&str) -> Option<MenuTarget>,
) -> OverflowRow {
    let command = unit.menu_command().to_string();
    // `menu ? submenu : command`: an empty menu id is falsy.
    match resolve(&command).and_then(|t| t.menu).filter(|m| !m.is_empty()) {
        Some(menu) => OverflowRow::Submenu { command, menu },
        None => OverflowRow::Command { command },
    }
}

fn assigned(fit: &FitResult, unit: &NormalizedUnit, want: fn(&UnitAssignment) -> bool) -> bool {
    fit.units.get(unit.key()).is_some_and(want)
}

/// The overflow menu: one section per group with overflowed units, in bar
/// order.
pub fn project_overflow(
    bar: &NormalizedBar,
    fit: &FitResult,
    resolve: impl Fn(&str) -> Option<MenuTarget>,
) -> Vec<OverflowSection> {
    let mut sections = Vec::new();
    for section in &bar.sections {
        for group in &section.groups {
            let rows: Vec<OverflowRow> = group
                .units
                .iter()
                .filter(|u| assigned(fit, u, |a| matches!(a, UnitAssignment::Overflow)))
                .map(|u| project_unit(u, &resolve))
                .collect();
            if rows.is_empty() {
                continue;
            }
            sections.push(OverflowSection {
                label_key: group.label_key.clone(),
                role: (group.role == GroupRole::Tabs).then_some(SectionRole::Radio),
                rows,
            });
        }
    }
    sections
}

/// A group's disclosure menu: its shed units, in bar order.
pub fn project_shed(
    group: &NormalizedGroup,
    fit: &FitResult,
    resolve: impl Fn(&str) -> Option<MenuTarget>,
) -> Vec<OverflowRow> {
    group
        .units
        .iter()
        .filter(|u| assigned(fit, u, |a| matches!(a, UnitAssignment::Shed)))
        .map(|u| project_unit(u, &resolve))
        .collect()
}

/// One strip group: its visible commands in bar order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripGroup {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    pub commands: Vec<String>,
}

/// A bar projected for un-measured rendering: hidden commands drop out and
/// groups that empty out vanish.
pub fn project_strip(
    bar: &NormalizedBar,
    mut visible: impl FnMut(&str) -> bool,
) -> Vec<StripGroup> {
    let mut groups = Vec::new();
    for section in &bar.sections {
        for group in &section.groups {
            let commands: Vec<String> = group
                .units
                .iter()
                .map(|u| u.menu_command())
                .filter(|c| visible(c))
                .map(str::to_string)
                .collect();
            if commands.is_empty() {
                continue;
            }
            groups.push(StripGroup {
                id: group.id.clone(),
                label_key: group.label_key.clone(),
                commands,
            });
        }
    }
    groups
}
