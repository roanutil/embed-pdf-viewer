//! `schema.ts`: the structure-only bar vocabulary, the authoring sugar, and
//! the normalized form the solver and projection consume. Where bars go, and
//! the menus and panels around them, is the layout (`layout.rs`).
//!
//! Every type serializes to the JSON the TypeScript object prints: members
//! the TypeScript leaves `undefined` are absent.

use std::collections::HashSet;

use epdf_kernel::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Who yields space first: 1 degrades first, 3 is the default, 5 is pinned.
/// The TypeScript type is the literal union `1 | 2 | 3 | 4 | 5`; the solver
/// only ever compares it, so any value orders the same way here.
pub type Importance = u8;

pub const PINNED: Importance = 5;
pub const DEFAULT_IMPORTANCE: Importance = 3;

/// Presentation ladder for a command button, richest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    #[serde(rename = "icon+label")]
    IconLabel,
    #[serde(rename = "icon")]
    Icon,
    #[serde(rename = "label")]
    Label,
}

impl Variant {
    pub fn as_str(self) -> &'static str {
        match self {
            Variant::IconLabel => "icon+label",
            Variant::Icon => "icon",
            Variant::Label => "label",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BarItem {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<Variant>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
}

/// An app-rendered slot. Its `terminal` command is its menu form, what it
/// becomes when it overflows; an item without one never leaves the bar (it is
/// pinned, and so is its group).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomItem {
    pub slot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
}

/// `BarItem | CustomItem | string`. An object with a `slot` member is a
/// custom item (`'slot' in child`), whatever else it carries.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum BarChild {
    Command(String),
    Item(BarItem),
    Custom(CustomItem),
}

impl<'de> Deserialize<'de> for BarChild {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let value = Value::deserialize(deserializer)?;
        match &value {
            Value::String(s) => Ok(BarChild::Command(s.clone())),
            Value::Object(o) if o.contains_key("slot") => {
                serde_json::from_value(value).map(BarChild::Custom).map_err(D::Error::custom)
            }
            _ => serde_json::from_value(value).map(BarChild::Item).map_err(D::Error::custom),
        }
    }
}

impl From<&str> for BarChild {
    fn from(command: &str) -> Self {
        BarChild::Command(command.to_string())
    }
}

impl From<BarItem> for BarChild {
    fn from(item: BarItem) -> Self {
        BarChild::Item(item)
    }
}

impl From<CustomItem> for BarChild {
    fn from(item: CustomItem) -> Self {
        BarChild::Custom(item)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupRole {
    Buttons,
    Tabs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collapse {
    Menu,
    Select,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarGroup {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<GroupRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse: Option<Collapse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
    /// Shows only while something that isn't dependent shows in the same
    /// bar, like undo and redo beside a mode's tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependent: Option<bool>,
    pub items: Vec<BarChild>,
}

/// `keyof BarSections`, in the order every function walks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionName {
    Start,
    Center,
    End,
}

impl SectionName {
    pub const ALL: [SectionName; 3] = [SectionName::Start, SectionName::Center, SectionName::End];

    pub fn as_str(self) -> &'static str {
        match self {
            SectionName::Start => "start",
            SectionName::Center => "center",
            SectionName::End => "end",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BarSections {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Vec<BarGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Vec<BarGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Vec<BarGroup>>,
}

impl BarSections {
    /// `sections[name]`.
    pub fn get(&self, name: SectionName) -> Option<&Vec<BarGroup>> {
        match name {
            SectionName::Start => self.start.as_ref(),
            SectionName::Center => self.center.as_ref(),
            SectionName::End => self.end.as_ref(),
        }
    }

    /// `sections[name] = groups`.
    pub fn set(&mut self, name: SectionName, groups: Option<Vec<BarGroup>>) {
        match name {
            SectionName::Start => self.start = groups,
            SectionName::Center => self.center = groups,
            SectionName::End => self.end = groups,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BarSchema {
    pub id: String,
    pub sections: BarSections,
}

// ── authoring sugar ─────────────────────────────────────────────────────────

/// `Omit<BarItem, 'command'>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<Variant>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
}

/// `Omit<CustomItem, 'slot' | 'terminal'>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
}

/// `Omit<BarGroup, 'id' | 'items'>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<GroupRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse: Option<Collapse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependent: Option<bool>,
}

/// A command button: `item("zoom:in", Some(ItemOpts { variants: .. }))`.
pub fn item(command: &str, options: Option<ItemOpts>) -> BarItem {
    let options = options.unwrap_or_default();
    BarItem {
        command: command.to_string(),
        variants: options.variants,
        importance: options.importance,
    }
}

/// An item you draw yourself, by its slot name, and the command it becomes in
/// the "More" menu. Without a command, the item never leaves the bar.
pub fn custom(slot: &str, terminal: Option<&str>, options: Option<CustomOpts>) -> CustomItem {
    let options = options.unwrap_or_default();
    CustomItem {
        slot: slot.to_string(),
        terminal: terminal.map(str::to_string),
        variants: options.variants,
        importance: options.importance,
    }
}

/// A group of items, with a separator between groups:
/// `group("zoom", vec!["zoom:out".into(), "zoom:in".into()], None)`.
pub fn group(id: &str, items: Vec<BarChild>, options: Option<GroupOpts>) -> BarGroup {
    let options = options.unwrap_or_default();
    BarGroup {
        id: id.to_string(),
        label_key: options.label_key,
        role: options.role,
        shed: options.shed,
        collapse: options.collapse,
        importance: options.importance,
        dependent: options.dependent,
        items,
    }
}

// ── normalized form ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NormalizedUnit {
    Command {
        /// Unique within the bar: `${groupId}:${command}`.
        key: String,
        command: String,
        variants: Vec<String>,
        importance: Importance,
    },
    Custom {
        key: String,
        slot: String,
        variants: Vec<String>,
        /// The command it becomes in menus, or `None` (`null`) for an item
        /// without one, which [`filter_bar`] keeps, pinned.
        terminal: Option<String>,
        importance: Importance,
    },
}

impl NormalizedUnit {
    pub fn key(&self) -> &str {
        match self {
            NormalizedUnit::Command { key, .. } | NormalizedUnit::Custom { key, .. } => key,
        }
    }

    pub fn variants(&self) -> &[String] {
        match self {
            NormalizedUnit::Command { variants, .. } | NormalizedUnit::Custom { variants, .. } => {
                variants
            }
        }
    }

    pub fn importance(&self) -> Importance {
        match self {
            NormalizedUnit::Command { importance, .. }
            | NormalizedUnit::Custom { importance, .. } => *importance,
        }
    }
}

/// Whether a unit is a custom item with no command to become in menus: it
/// never leaves the bar.
pub fn is_standalone_unit(unit: &NormalizedUnit) -> bool {
    matches!(unit, NormalizedUnit::Custom { terminal: None, .. })
}

/// The command a unit runs: its own, or a custom item's `terminal`; `None`
/// for a standalone item.
pub fn command_of_unit(unit: &NormalizedUnit) -> Option<&str> {
    match unit {
        NormalizedUnit::Command { command, .. } => Some(command),
        NormalizedUnit::Custom { terminal, .. } => terminal.as_deref(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedGroup {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    pub role: GroupRole,
    pub shed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse: Option<Collapse>,
    pub importance: Importance,
    /// See [`BarGroup::dependent`]; the rule is [`has_visible_units`]. A
    /// group read without one is not dependent, as the TypeScript reads
    /// `undefined`.
    #[serde(default)]
    pub dependent: bool,
    pub units: Vec<NormalizedUnit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedSection {
    pub name: SectionName,
    pub groups: Vec<NormalizedGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedBar {
    pub id: String,
    pub sections: Vec<NormalizedSection>,
}

const DEFAULT_COMMAND_VARIANTS: [Variant; 1] = [Variant::Icon];
const DEFAULT_CUSTOM_VARIANT: &str = "default";

fn normalize_child(
    group_id: &str,
    child: &BarChild,
    seen: &mut HashSet<String>,
) -> Result<NormalizedUnit, Error> {
    let unit = match child {
        BarChild::Custom(c) => NormalizedUnit::Custom {
            key: format!("{group_id}:{}", c.slot),
            slot: c.slot.clone(),
            variants: c
                .variants
                .clone()
                .unwrap_or_else(|| vec![DEFAULT_CUSTOM_VARIANT.to_string()]),
            terminal: c.terminal.clone(),
            importance: match c.terminal {
                None => PINNED,
                Some(_) => c.importance.unwrap_or(DEFAULT_IMPORTANCE),
            },
        },
        BarChild::Command(command) => NormalizedUnit::Command {
            key: format!("{group_id}:{command}"),
            command: command.clone(),
            variants: DEFAULT_COMMAND_VARIANTS.iter().map(|v| v.as_str().to_string()).collect(),
            importance: DEFAULT_IMPORTANCE,
        },
        BarChild::Item(i) => NormalizedUnit::Command {
            key: format!("{group_id}:{}", i.command),
            command: i.command.clone(),
            variants: i
                .variants
                .as_deref()
                .unwrap_or(&DEFAULT_COMMAND_VARIANTS)
                .iter()
                .map(|v| v.as_str().to_string())
                .collect(),
            importance: i.importance.unwrap_or(DEFAULT_IMPORTANCE),
        },
    };
    if unit.variants().is_empty() {
        return Err(Error::new(format!("[ui-core] empty variant ladder: {}", unit.key())));
    }
    if !seen.insert(unit.key().to_string()) {
        return Err(Error::new(format!("[ui-core] duplicate unit in bar: {}", unit.key())));
    }
    Ok(unit)
}

/// Resolve shorthands and defaults; validate uniqueness. A group holding a
/// custom item without a command is pinned with it: it never collapses or
/// overflows whole, so the item stays in the bar.
pub fn normalize_bar(bar: &BarSchema) -> Result<NormalizedBar, Error> {
    let mut seen = HashSet::new();
    let mut group_ids = HashSet::new();
    let mut sections = Vec::new();
    for name in SectionName::ALL {
        let Some(groups) = bar.sections.get(name) else { continue };
        if groups.is_empty() {
            continue;
        }
        let mut out = Vec::with_capacity(groups.len());
        for g in groups {
            if !group_ids.insert(g.id.clone()) {
                return Err(Error::new(format!("[ui-core] duplicate group in bar: {}", g.id)));
            }
            let units = g
                .items
                .iter()
                .map(|c| normalize_child(&g.id, c, &mut seen))
                .collect::<Result<Vec<_>, _>>()?;
            out.push(NormalizedGroup {
                id: g.id.clone(),
                label_key: g.label_key.clone(),
                role: g.role.unwrap_or(GroupRole::Buttons),
                shed: g.shed.unwrap_or(false),
                collapse: g.collapse,
                importance: if units.iter().any(is_standalone_unit) {
                    PINNED
                } else {
                    g.importance.unwrap_or(DEFAULT_IMPORTANCE)
                },
                dependent: g.dependent == Some(true),
                units,
            });
        }
        sections.push(NormalizedSection { name, groups: out });
    }
    Ok(NormalizedBar { id: bar.id.clone(), sections })
}

/// Drop units whose command is currently invisible before solving: an
/// invisible unit must not consume budget or appear in the overflow. A custom
/// item without a command has nothing to ask `keep` about, and stays. A bar
/// that shows nothing ([`has_visible_units`]) keeps no unit at all, so its
/// dependent groups go too.
pub fn filter_bar(
    bar: &NormalizedBar,
    mut keep: impl FnMut(&NormalizedUnit) -> bool,
) -> NormalizedBar {
    let shows = has_visible_units(bar, &mut keep);
    NormalizedBar {
        id: bar.id.clone(),
        sections: bar
            .sections
            .iter()
            .map(|s| NormalizedSection {
                name: s.name,
                groups: s
                    .groups
                    .iter()
                    .map(|g| NormalizedGroup {
                        units: if shows {
                            g.units
                                .iter()
                                .filter(|u| is_standalone_unit(u) || keep(u))
                                .cloned()
                                .collect()
                        } else {
                            Vec::new()
                        },
                        ..g.clone()
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Whether the bar shows anything once `keep` (as for [`filter_bar`]) has had
/// its say: a unit left in a group that isn't dependent. Dependent groups show
/// only beside one, so a bar with only dependent groups left shows nothing.
pub fn has_visible_units(
    bar: &NormalizedBar,
    mut keep: impl FnMut(&NormalizedUnit) -> bool,
) -> bool {
    bar.sections.iter().any(|section| {
        section
            .groups
            .iter()
            .any(|g| !g.dependent && g.units.iter().any(|u| is_standalone_unit(u) || keep(u)))
    })
}
