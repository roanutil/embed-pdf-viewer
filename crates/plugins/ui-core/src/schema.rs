//! `schema.ts`: the structure-only schema vocabulary, the authoring sugar,
//! and the normalized form the solver and projection consume.
//!
//! Every type serializes to the JSON the TypeScript object prints: members
//! the TypeScript leaves `undefined` are absent.

use std::collections::HashSet;

use epdf_kernel::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::js::Record;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomItem {
    pub slot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
    pub terminal: String,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    pub items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuSchema {
    pub id: String,
    pub sections: Vec<MenuSection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolbarPosition {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabsVisibility {
    Always,
    Multiple,
    Never,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameSchema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolbar: Option<ToolbarPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<TabsVisibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeSchema {
    pub bars: Record<BarSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_bars: Option<Record<BarSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menus: Option<Record<MenuSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strips: Option<Record<BarSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<FrameSchema>,
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

/// `Omit<CustomItem, 'slot'>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
    pub terminal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
}

/// `Omit<BarGroup, 'id' | 'items'> & { items?: readonly BarChild[] }`.
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
    pub items: Option<Vec<BarChild>>,
}

/// `group`'s second argument: the children, or options (`Array.isArray`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GroupOptsOrItems {
    Items(Vec<BarChild>),
    Opts(GroupOpts),
}

impl<'de> Deserialize<'de> for GroupOptsOrItems {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let value = Value::deserialize(deserializer)?;
        if value.is_array() {
            serde_json::from_value(value).map(GroupOptsOrItems::Items).map_err(D::Error::custom)
        } else {
            serde_json::from_value(value).map(GroupOptsOrItems::Opts).map_err(D::Error::custom)
        }
    }
}

/// `item(command, opts?)`.
pub fn item(command: &str, opts: Option<ItemOpts>) -> BarItem {
    let opts = opts.unwrap_or_default();
    BarItem { command: command.to_string(), variants: opts.variants, importance: opts.importance }
}

/// `custom(slot, opts)`.
pub fn custom(slot: &str, opts: CustomOpts) -> CustomItem {
    CustomItem {
        slot: slot.to_string(),
        variants: opts.variants,
        terminal: opts.terminal,
        importance: opts.importance,
    }
}

/// `group(id, items)` and `group(id, opts, items?)`. With children as the
/// second argument the third is ignored; with options, `items` wins over
/// `opts.items`, and neither means none.
pub fn group(id: &str, opts_or_items: GroupOptsOrItems, items: Option<Vec<BarChild>>) -> BarGroup {
    match opts_or_items {
        GroupOptsOrItems::Items(items) => BarGroup {
            id: id.to_string(),
            label_key: None,
            role: None,
            shed: None,
            collapse: None,
            importance: None,
            items,
        },
        GroupOptsOrItems::Opts(opts) => BarGroup {
            id: id.to_string(),
            label_key: opts.label_key,
            role: opts.role,
            shed: opts.shed,
            collapse: opts.collapse,
            importance: opts.importance,
            items: items.or(opts.items).unwrap_or_default(),
        },
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
        terminal: String,
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

    /// The command the unit stands for in a menu: its own, or a custom
    /// item's `terminal`.
    pub fn menu_command(&self) -> &str {
        match self {
            NormalizedUnit::Command { command, .. } => command,
            NormalizedUnit::Custom { terminal, .. } => terminal,
        }
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
            importance: c.importance.unwrap_or(DEFAULT_IMPORTANCE),
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

/// Resolve shorthands and defaults; validate uniqueness.
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
                importance: g.importance.unwrap_or(DEFAULT_IMPORTANCE),
                units,
            });
        }
        sections.push(NormalizedSection { name, groups: out });
    }
    Ok(NormalizedBar { id: bar.id.clone(), sections })
}

/// Drop units `keep` rejects, before solving. Groups that empty out stay,
/// with no units.
pub fn filter_bar(
    bar: &NormalizedBar,
    mut keep: impl FnMut(&NormalizedUnit) -> bool,
) -> NormalizedBar {
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
                        units: g.units.iter().filter(|u| keep(u)).cloned().collect(),
                        ..g.clone()
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Identity plus validation: every bar, mode bar and strip normalizes.
/// Menus are not checked.
pub fn define_chrome(chrome: ChromeSchema) -> Result<ChromeSchema, Error> {
    for bar in chrome.bars.values() {
        normalize_bar(bar)?;
    }
    for bar in chrome.mode_bars.iter().flat_map(Record::values) {
        normalize_bar(bar)?;
    }
    for strip in chrome.strips.iter().flat_map(Record::values) {
        normalize_bar(strip)?;
    }
    Ok(chrome)
}

/// Dev-time cross-check: every command the chrome references is in
/// `known_commands`. A bar that does not normalize is an error, not a
/// problem.
pub fn validate_chrome(
    chrome: &ChromeSchema,
    known_commands: &HashSet<String>,
) -> Result<Vec<String>, Error> {
    let mut problems = Vec::new();
    let mut check_bar = |bar: &BarSchema, collection: &str, id: &str| -> Result<(), Error> {
        for section in normalize_bar(bar)?.sections {
            for g in section.groups {
                for u in g.units {
                    let c = u.menu_command();
                    if !known_commands.contains(c) {
                        problems.push(format!("{collection}.{id}: unknown command \"{c}\""));
                    }
                }
            }
        }
        Ok(())
    };
    for (id, bar) in chrome.bars.iter() {
        check_bar(bar, "bars", id)?;
    }
    for (id, bar) in chrome.mode_bars.iter().flat_map(Record::iter) {
        check_bar(bar, "modeBars", id)?;
    }
    for (id, strip) in chrome.strips.iter().flat_map(Record::iter) {
        check_bar(strip, "strips", id)?;
    }
    for (id, menu) in chrome.menus.iter().flat_map(Record::iter) {
        for section in &menu.sections {
            for c in &section.items {
                if !known_commands.contains(c) {
                    problems.push(format!("menus.{id}: unknown command \"{c}\""));
                }
            }
        }
    }
    Ok(problems)
}
