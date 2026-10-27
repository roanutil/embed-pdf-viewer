//! `layout.ts`: the viewer's layout. Where every bar, menu and panel goes, as
//! plain data, and the immutable [`Layout`] whose edits change it. Every edit
//! returns a new layout, so edits chain. An edit that names an id the layout
//! doesn't have changes nothing and records a warning with the closest id
//! that does exist; [`validate_layout`](crate::validate_layout) returns those
//! warnings with the rest, so a typo never stops the viewer.
//!
//! Layout data comes from people, so it is a [`JsValue`] and is read the way
//! the TypeScript reads it: what isn't the right shape is skipped here and
//! named by `validate_layout`. An edit copies only the objects on the path to
//! what it changes; every other bar, menu, panel, group and list is the same
//! allocation as before ([`JsValue::same`]), as the TypeScript keeps the same
//! reference.

use std::rc::Rc;

use crate::js::Record;
use crate::schema::SectionName;
use crate::suggestions::did_you_mean;
use crate::value::{JsValue, js_string, json_stringify, spread};

// ── the data ─────────────────────────────────────────────────────────────────

/// An edge of the viewer. `start` and `end` follow the language: left and
/// right, swapped in a right-to-left one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarEdge {
    Top,
    Bottom,
    Start,
    End,
}

impl BarEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            BarEdge::Top => "top",
            BarEdge::Bottom => "bottom",
            BarEdge::Start => "start",
            BarEdge::End => "end",
        }
    }
}

/// Where a floating bar sits over the pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatPosition {
    TopStart,
    Top,
    TopEnd,
    Start,
    End,
    BottomStart,
    Bottom,
    BottomEnd,
}

impl FloatPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            FloatPosition::TopStart => "top-start",
            FloatPosition::Top => "top",
            FloatPosition::TopEnd => "top-end",
            FloatPosition::Start => "start",
            FloatPosition::End => "end",
            FloatPosition::BottomStart => "bottom-start",
            FloatPosition::Bottom => "bottom",
            FloatPosition::BottomEnd => "bottom-end",
        }
    }
}

/// What a bar over a selection follows: selected text, or selected
/// annotations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionTarget {
    Selection,
    Annotation,
}

impl SelectionTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            SelectionTarget::Selection => "selection",
            SelectionTarget::Annotation => "annotation",
        }
    }
}

/// Where a bar goes: along an edge, floating over the pages, along another
/// bar on the side of the pages, or over what's selected. Bars on one edge
/// stack from the outside in, in the order they were added; `before` and
/// `after` put a bar next to another bar on that edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarPlace {
    /// `'top'`.
    Edge(BarEdge),
    /// `{ edge, before?, after? }`.
    OnEdge { edge: BarEdge, before: Option<String>, after: Option<String> },
    /// `{ float }`.
    Float(FloatPosition),
    /// `{ with }`.
    With(String),
    /// `{ at }`.
    At(SelectionTarget),
}

impl From<BarPlace> for JsValue {
    fn from(place: BarPlace) -> Self {
        let one = |key: &str, value: &str| {
            JsValue::object([(key, JsValue::from(value))].into_iter().collect())
        };
        match place {
            BarPlace::Edge(edge) => JsValue::from(edge.as_str()),
            BarPlace::OnEdge { edge, before, after } => {
                let mut record = Record::new();
                record.insert("edge", JsValue::from(edge.as_str()));
                if let Some(before) = before {
                    record.insert("before", JsValue::from(before));
                }
                if let Some(after) = after {
                    record.insert("after", JsValue::from(after));
                }
                JsValue::object(record)
            }
            BarPlace::Float(position) => one("float", position.as_str()),
            BarPlace::With(host) => one("with", &host),
            BarPlace::At(target) => one("at", target.as_str()),
        }
    }
}

/// When the document tabs show: always, while more than one document is
/// open, or never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsVisibility {
    Always,
    Multiple,
    Never,
}

impl TabsVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            TabsVisibility::Always => "always",
            TabsVisibility::Multiple => "multiple",
            TabsVisibility::Never => "never",
        }
    }
}

/// How a panel opens: beside the pages, over them, or up from the bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelPresentation {
    Dock,
    Overlay,
    Sheet,
}

impl PanelPresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            PanelPresentation::Dock => "dock",
            PanelPresentation::Overlay => "overlay",
            PanelPresentation::Sheet => "sheet",
        }
    }
}

// ── the layout ───────────────────────────────────────────────────────────────

/// Where [`Layout::add`] puts an item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddItemOptions {
    /// The bar or menu. A bar wins over a menu with the same id. Empty is
    /// none.
    pub to: String,
    /// The bar's section for a new group. Default `end`; a name that isn't a
    /// section is warned about and reads as `end`.
    pub section: Option<String>,
    /// The group to join, by id; a new id starts a new group at the end of
    /// the section.
    pub group: Option<String>,
    /// An item to put it before, in that item's group.
    pub before: Option<String>,
    /// An item to put it after, in that item's group.
    pub after: Option<String>,
}

/// Where [`Layout::remove`] takes items from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoveItemOptions {
    /// The bar or menu: always one place. Empty is none.
    pub from: String,
}

/// A layout: its data and the edits. Every edit returns a new layout and
/// leaves this one as it was; an edit that names an id the layout doesn't
/// have changes nothing and records a warning.
#[derive(Clone, Debug)]
pub struct Layout(Rc<LayoutState>);

#[derive(Debug)]
struct LayoutState {
    data: JsValue,
    warnings: Vec<String>,
}

type Edited = (JsValue, Vec<String>);

/// A layout of `data`, which it never changes.
pub fn create_layout(data: impl Into<JsValue>) -> Layout {
    Layout::of(data.into(), Vec::new())
}

/// What the edits that made `layout` couldn't do.
pub fn layout_warnings_of(layout: &Layout) -> &[String] {
    &layout.0.warnings
}

/// `layout` with one more warning.
pub fn with_layout_warning(layout: &Layout, warning: impl Into<String>) -> Layout {
    let mut warnings = layout.0.warnings.clone();
    warnings.push(warning.into());
    Layout::of(layout.0.data.clone(), warnings)
}

impl Default for Layout {
    /// `createLayout()`: no bars, menus or panels.
    fn default() -> Self {
        create_layout(JsValue::empty_object())
    }
}

impl Layout {
    fn of(data: JsValue, warnings: Vec<String>) -> Layout {
        Layout(Rc::new(LayoutState { data, warnings }))
    }

    fn next(&self, (data, added): Edited) -> Layout {
        let mut warnings = self.0.warnings.clone();
        warnings.extend(added);
        Layout::of(data, warnings)
    }

    /// Whether two layouts are the same value (`===`).
    pub fn same(left: &Layout, right: &Layout) -> bool {
        Rc::ptr_eq(&left.0, &right.0)
    }

    /// The layout as plain data: the data it was made from, or its edits'.
    pub fn to_json(&self) -> JsValue {
        self.0.data.clone()
    }

    /// What the edits that made this layout couldn't do.
    pub fn warnings(&self) -> &[String] {
        &self.0.warnings
    }

    /// Move a bar.
    pub fn place(&self, bar: &str, place: impl Into<JsValue>) -> Layout {
        self.next(place_bar(&self.0.data, bar, place.into()))
    }

    /// Add a bar, or replace the bar with that id.
    pub fn add_bar(&self, id: &str, bar: impl Into<JsValue>) -> Layout {
        self.next((with_record(&self.0.data, "bars", id, bar.into()), Vec::new()))
    }

    /// Remove a bar. Removing a mode's bar removes its tab.
    pub fn remove_bar(&self, id: &str) -> Layout {
        self.next(remove_bar(&self.0.data, id))
    }

    /// Put an item in a bar or a menu: at the end of a section, in a group,
    /// or next to an item.
    pub fn add(&self, item: impl Into<JsValue>, options: &AddItemOptions) -> Layout {
        self.next(add_item(&self.0.data, &item.into(), options))
    }

    /// Take items out of one bar or menu; a group or section with nothing
    /// left goes with them.
    pub fn remove<S: AsRef<str>>(&self, ids: &[S], options: &RemoveItemOptions) -> Layout {
        let ids: Vec<&str> = ids.iter().map(AsRef::as_ref).collect();
        self.next(remove_items(&self.0.data, &ids, options))
    }

    /// Put another item in an item's place, in every bar and menu.
    pub fn replace(&self, id: &str, item: impl Into<JsValue>) -> Layout {
        self.next(replace_item(&self.0.data, id, &item.into()))
    }

    /// Add a menu, or replace the menu with that id.
    pub fn add_menu(&self, id: &str, sections: impl Into<JsValue>) -> Layout {
        self.next((with_record(&self.0.data, "menus", id, sections.into()), Vec::new()))
    }

    /// Add a panel, or replace the panel with that id.
    pub fn add_panel(&self, id: &str, panel: impl Into<JsValue>) -> Layout {
        self.next((with_record(&self.0.data, "panels", id, panel.into()), Vec::new()))
    }

    /// Change a panel's fields. `present` and `width` change only what they
    /// name: a map of sizes keeps the other sizes, and a number or
    /// `{ initial }` keeps `min` and `max`.
    pub fn panel(&self, id: &str, changes: impl Into<JsValue>) -> Layout {
        self.next(update_panel(&self.0.data, id, &changes.into()))
    }

    /// Change every panel's fields, as [`Layout::panel`] changes one: every
    /// panel the layout has so far, so a panel added after it keeps its own.
    pub fn panels(&self, changes: impl Into<JsValue>) -> Layout {
        self.next((update_panels(&self.0.data, &changes.into()), Vec::new()))
    }

    /// Remove a panel, and its `panel:<id>` button from every bar and menu.
    pub fn remove_panel(&self, id: &str) -> Layout {
        self.next(remove_panel(&self.0.data, id))
    }

    /// When the document tabs show.
    pub fn tabs(&self, visibility: TabsVisibility) -> Layout {
        let mut data = spread(Some(&self.0.data));
        data.insert("tabs", JsValue::from(visibility.as_str()));
        self.next((JsValue::object(data), Vec::new()))
    }
}

/// `Layout | LayoutData`: what [`is_layout`] and
/// [`validate_layout`](crate::validate_layout) take.
#[derive(Clone, Copy, Debug)]
pub enum LayoutInput<'a> {
    Layout(&'a Layout),
    Data(&'a JsValue),
}

impl<'a> From<&'a Layout> for LayoutInput<'a> {
    fn from(layout: &'a Layout) -> Self {
        LayoutInput::Layout(layout)
    }
}

impl<'a> From<&'a JsValue> for LayoutInput<'a> {
    fn from(data: &'a JsValue) -> Self {
        LayoutInput::Data(data)
    }
}

/// Whether `value` is a layout made by [`create_layout`] or one of its edits,
/// rather than plain data.
pub fn is_layout<'a>(value: impl Into<LayoutInput<'a>>) -> bool {
    matches!(value.into(), LayoutInput::Layout(_))
}

// ── reading the data ─────────────────────────────────────────────────────────

pub const SECTION_NAMES: [SectionName; 3] = SectionName::ALL;

/// Whether a section entry is a group written as an object.
pub fn is_group_object(entry: &JsValue) -> bool {
    matches!(entry, JsValue::Object(record) if record.contains_key("items"))
}

/// A bar section's entries; none for a section that isn't a list.
pub fn entries_of(bar: &JsValue, name: SectionName) -> Vec<JsValue> {
    match bar.get(name.as_str()) {
        Some(JsValue::Array(entries)) => (*entries).clone(),
        _ => Vec::new(),
    }
}

/// A section entry's items: a group's, or the lone item.
pub fn items_of_entry(entry: &JsValue) -> Vec<JsValue> {
    match entry {
        JsValue::Array(items) => (**items).clone(),
        _ if is_group_object(entry) => match entry.get("items") {
            Some(JsValue::Array(items)) => (*items).clone(),
            _ => Vec::new(),
        },
        _ => vec![entry.clone()],
    }
}

/// What an item is called in edits: its command id, your component's id, or
/// `''` for a non-item. It is the member as written, so a `custom` that isn't
/// a string is that value, and an object with neither member has `None`
/// (`undefined`), as in the TypeScript.
pub fn item_id_of(item: &JsValue) -> Option<JsValue> {
    match item {
        JsValue::String(_) => Some(item.clone()),
        JsValue::Object(record) if record.contains_key("custom") => record.get("custom").cloned(),
        JsValue::Object(_) | JsValue::Array(_) => item.get("command"),
        _ => Some(JsValue::from("")),
    }
}

/// `itemIdOf(item) === id`.
fn has_id(item: &JsValue, id: &JsValue) -> bool {
    item_id_of(item).is_some_and(|own| JsValue::same(&own, id))
}

/// The command an item runs or stands for in menus, or `None` for your
/// component without one.
pub fn item_command_of(item: &JsValue) -> Option<JsValue> {
    match item {
        JsValue::String(_) => Some(item.clone()),
        JsValue::Object(record) if record.contains_key("custom") => {
            record.get("command").filter(|c| !matches!(c, JsValue::Null)).cloned()
        }
        JsValue::Object(_) | JsValue::Array(_) => item.get("command"),
        _ => None,
    }
}

/// Whether `value` has an item's shape: a command id, `{ command }` or
/// `{ custom }`.
pub fn is_layout_item(value: &JsValue) -> bool {
    match value {
        JsValue::String(s) => !s.is_empty(),
        JsValue::Object(record) => {
            matches!(record.get("custom"), Some(JsValue::String(_)))
                || matches!(record.get("command"), Some(JsValue::String(_)))
        }
        _ => false,
    }
}

/// `new Set(values)`, in order.
fn unique(values: impl IntoIterator<Item = JsValue>) -> Vec<JsValue> {
    let mut out: Vec<JsValue> = Vec::new();
    for value in values {
        if !out.iter().any(|seen| JsValue::same_value_zero(seen, &value)) {
            out.push(value);
        }
    }
    out
}

/// `values.includes(value)`.
pub(crate) fn includes(values: &[JsValue], value: &JsValue) -> bool {
    values.iter().any(|each| JsValue::same_value_zero(each, value))
}

/// The string candidates among `values`: a "did you mean" never suggests
/// anything else.
fn names(values: &[JsValue]) -> impl Iterator<Item = &str> {
    values.iter().filter_map(JsValue::as_str)
}

/// Every item id in a bar, in order, once each.
pub fn item_ids_of_bar(bar: &JsValue) -> Vec<JsValue> {
    unique(SECTION_NAMES.into_iter().flat_map(|name| {
        entries_of(bar, name)
            .iter()
            .flat_map(items_of_entry)
            .filter(is_layout_item)
            .filter_map(|item| item_id_of(&item))
            .collect::<Vec<_>>()
    }))
}

/// A menu section's items; none for a section that isn't one.
pub fn items_of_menu_section(section: &JsValue) -> Vec<JsValue> {
    match section {
        JsValue::Array(items) => (**items).clone(),
        _ => match section.get("items") {
            Some(JsValue::Array(items)) => (*items).clone(),
            _ => Vec::new(),
        },
    }
}

/// A menu's sections; none for a menu that isn't a list.
pub fn sections_of_menu(menu: &JsValue) -> Vec<JsValue> {
    menu.as_array().map(<[JsValue]>::to_vec).unwrap_or_default()
}

/// Every item in a menu, in order, once each.
pub fn item_ids_of_menu(menu: &JsValue) -> Vec<JsValue> {
    unique(sections_of_menu(menu).iter().flat_map(items_of_menu_section))
}

/// `data[key] ?? {}`.
fn record_of(data: &JsValue, key: &str) -> JsValue {
    match data.get(key) {
        None | Some(JsValue::Null) => JsValue::empty_object(),
        Some(value) => value,
    }
}

pub(crate) fn bars_of(data: &JsValue) -> JsValue {
    record_of(data, "bars")
}

pub(crate) fn menus_of(data: &JsValue) -> JsValue {
    record_of(data, "menus")
}

pub(crate) fn panels_of(data: &JsValue) -> JsValue {
    record_of(data, "panels")
}

/// `{ ...data, [key]: { ...data[key], [id]: value } }`.
fn with_record(data: &JsValue, key: &str, id: &str, value: JsValue) -> JsValue {
    let mut inner = spread(data.get(key).as_ref());
    inner.insert(id, value);
    let mut outer = spread(Some(data));
    outer.insert(key, JsValue::object(inner));
    JsValue::object(outer)
}

/// `{ ...data, [key]: rest }`, where `rest` is `data[key] ?? {}` without `id`.
fn without_record(data: &JsValue, key: &str, id: &str) -> JsValue {
    let mut rest = spread(Some(&record_of(data, key)));
    rest.remove(id);
    let mut outer = spread(Some(data));
    outer.insert(key, JsValue::object(rest));
    JsValue::object(outer)
}

/// The entry with its items replaced: `None` when the edit took its last one,
/// so the group goes. A group that held none to begin with stays: the `modes`
/// group, whose tabs are derived.
fn with_entry_items(entry: &JsValue, items: Vec<JsValue>) -> Option<JsValue> {
    if items.is_empty() {
        return items_of_entry(entry).is_empty().then(|| entry.clone());
    }
    if is_group_object(entry) {
        let mut group = spread(Some(entry));
        group.insert("items", JsValue::array(items));
        return Some(JsValue::object(group));
    }
    if matches!(entry, JsValue::Array(_)) {
        return Some(JsValue::array(items));
    }
    // A lone item that gained a neighbour becomes a group of its own.
    Some(if items.len() == 1 {
        items.into_iter().next().expect("one item")
    } else {
        JsValue::array(items)
    })
}

/// The bar with each entry mapped; entries mapped to `None` and sections left
/// empty go.
fn map_entries(
    bar: &JsValue,
    mut map: impl FnMut(&JsValue, SectionName, usize) -> Option<JsValue>,
) -> JsValue {
    let mut next = Vec::new();
    for name in SECTION_NAMES {
        let entries = entries_of(bar, name);
        if entries.is_empty() {
            continue;
        }
        let mapped: Vec<JsValue> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| map(entry, name, index))
            .collect();
        if !mapped.is_empty() {
            next.push((name, JsValue::array(mapped)));
        }
    }
    let mut rest = spread(Some(bar));
    for name in SECTION_NAMES {
        rest.remove(name.as_str());
    }
    for (name, section) in next {
        rest.insert(name.as_str(), section);
    }
    JsValue::object(rest)
}

fn with_menu_section_items(section: &JsValue, items: Vec<JsValue>) -> Option<JsValue> {
    if items.is_empty() {
        return None;
    }
    Some(match section {
        JsValue::Array(_) => JsValue::array(items),
        _ => {
            let mut titled = spread(Some(section));
            titled.insert("items", JsValue::array(items));
            JsValue::object(titled)
        }
    })
}

fn not_found(verb: &str, id: &JsValue, place: &str, candidates: &[JsValue]) -> String {
    format!("{verb}: \"{}\" is not in \"{place}\"{}", js_string(id), suggest(id, candidates))
}

/// `didYouMean(id, candidates)`: an id that isn't a string has none.
fn suggest(id: &JsValue, candidates: &[JsValue]) -> String {
    match id.as_str() {
        Some(id) => did_you_mean(id, names(candidates)),
        None => String::new(),
    }
}

// ── the edits ────────────────────────────────────────────────────────────────

fn place_bar(data: &JsValue, id: &str, place: JsValue) -> Edited {
    let bars = bars_of(data);
    let Some(bar) = bars.get(id).filter(JsValue::truthy) else {
        let warning = format!(
            "place: no bar \"{id}\"{}",
            did_you_mean(id, bars.keys().iter().map(String::as_str))
        );
        return (data.clone(), vec![warning]);
    };
    let mut moved = spread(Some(&bar));
    moved.insert("place", place);
    (with_record(data, "bars", id, JsValue::object(moved)), Vec::new())
}

fn remove_bar(data: &JsValue, id: &str) -> Edited {
    let bars = bars_of(data);
    if !bars.get(id).is_some_and(|bar| bar.truthy()) {
        let warning = format!(
            "removeBar: no bar \"{id}\"{}",
            did_you_mean(id, bars.keys().iter().map(String::as_str))
        );
        return (data.clone(), vec![warning]);
    }
    (without_record(data, "bars", id), Vec::new())
}

fn add_item(data: &JsValue, item: &JsValue, options: &AddItemOptions) -> Edited {
    if !is_layout_item(item) {
        return (data.clone(), vec![format!("add: {} is not an item", json_stringify(item))]);
    }
    let to = options.to.as_str();
    if to.is_empty() {
        let id = item_id_of(item).map(|id| js_string(&id)).unwrap_or_default();
        return (
            data.clone(),
            vec![format!("add: \"{id}\" needs a bar or menu to go to ({{ to }})")],
        );
    }
    if let Some(bar) = bars_of(data).get(to).filter(JsValue::truthy) {
        return add_to_bar(data, to, &bar, item, options);
    }
    if let Some(menu) = menus_of(data).get(to).filter(JsValue::truthy) {
        return add_to_menu(data, to, &menu, item, options);
    }
    let mut places = bars_of(data).keys();
    places.extend(menus_of(data).keys());
    let warning = format!(
        "add: no bar or menu \"{to}\"{}",
        did_you_mean(to, places.iter().map(String::as_str))
    );
    (data.clone(), vec![warning])
}

fn add_to_bar(
    data: &JsValue,
    bar_id: &str,
    bar: &JsValue,
    item: &JsValue,
    options: &AddItemOptions,
) -> Edited {
    // A layout item always has an id.
    let id = item_id_of(item).unwrap_or(JsValue::Null);
    let mut warnings = Vec::new();
    let twice = |items: &[JsValue]| items.iter().any(|each| has_id(each, &id));
    let already =
        || format!("add: \"{}\" is already in that group of \"{bar_id}\"", js_string(&id));

    if let Some(anchor) = options.before.as_ref().or(options.after.as_ref()) {
        let anchor_value = JsValue::from(anchor.as_str());
        for name in SECTION_NAMES {
            for (index, entry) in entries_of(bar, name).iter().enumerate() {
                let items = items_of_entry(entry);
                let Some(at) = items.iter().position(|each| has_id(each, &anchor_value)) else {
                    continue;
                };
                if twice(&items) {
                    return (data.clone(), vec![already()]);
                }
                let cut = if options.before.is_some() { at } else { at + 1 };
                let mut next_items = items[..cut].to_vec();
                next_items.push(item.clone());
                next_items.extend_from_slice(&items[cut..]);
                let mut next_items = Some(next_items);
                let next_bar = map_entries(bar, |entry, section, i| {
                    if section == name && i == index {
                        with_entry_items(entry, next_items.take().unwrap_or_default())
                    } else {
                        Some(entry.clone())
                    }
                });
                return (with_record(data, "bars", bar_id, next_bar), Vec::new());
            }
        }
        // Not found: the item still goes in, where it would go without
        // `before`/`after`.
        warnings.push(not_found("add", &anchor_value, bar_id, &item_ids_of_bar(bar)));
    }

    if let Some(group) = &options.group {
        let group_value = JsValue::from(group.as_str());
        for name in SECTION_NAMES {
            let entries = entries_of(bar, name);
            let Some(index) = entries.iter().position(|entry| {
                is_group_object(entry)
                    && entry.get("id").is_some_and(|id| JsValue::same(&id, &group_value))
            }) else {
                continue;
            };
            let mut items = items_of_entry(&entries[index]);
            if twice(&items) {
                return (data.clone(), vec![already()]);
            }
            items.push(item.clone());
            let mut items = Some(items);
            let next_bar = map_entries(bar, |entry, section, i| {
                if section == name && i == index {
                    with_entry_items(entry, items.take().unwrap_or_default())
                } else {
                    Some(entry.clone())
                }
            });
            return (with_record(data, "bars", bar_id, next_bar), warnings);
        }
    }

    let mut section = options.section.clone().unwrap_or_else(|| "end".to_string());
    if !SECTION_NAMES.iter().any(|name| name.as_str() == section) {
        warnings.push(format!(
            "add: \"{section}\" is not a section (start, center or end); using \"end\""
        ));
        section = "end".to_string();
    }
    let entry = match &options.group {
        Some(group) => {
            let mut record = Record::new();
            record.insert("id", JsValue::from(group.as_str()));
            record.insert("items", JsValue::array(vec![item.clone()]));
            JsValue::object(record)
        }
        None => JsValue::array(vec![item.clone()]),
    };
    let name = SECTION_NAMES.into_iter().find(|name| name.as_str() == section).expect("a section");
    let mut entries = entries_of(bar, name);
    entries.push(entry);
    let mut next_bar = spread(Some(bar));
    next_bar.insert(section, JsValue::array(entries));
    (with_record(data, "bars", bar_id, JsValue::object(next_bar)), warnings)
}

/// A menu row is a command: `item` itself, a command object's command, or
/// `None` for your own component.
fn menu_command_of(item: &JsValue) -> Option<JsValue> {
    match item {
        JsValue::String(_) => Some(item.clone()),
        _ if item.has("custom") => None,
        _ => Some(item.get("command").unwrap_or(JsValue::Null)),
    }
}

fn add_to_menu(
    data: &JsValue,
    menu_id: &str,
    menu: &JsValue,
    item: &JsValue,
    options: &AddItemOptions,
) -> Edited {
    let Some(command) = menu_command_of(item) else {
        let warning =
            format!("add: \"{menu_id}\" is a menu, which holds commands, not your own components");
        return (data.clone(), vec![warning]);
    };
    if includes(&item_ids_of_menu(menu), &command) {
        let warning = format!("add: \"{}\" is already in \"{menu_id}\"", js_string(&command));
        return (data.clone(), vec![warning]);
    }

    let mut warnings = Vec::new();
    if let Some(anchor) = options.before.as_ref().or(options.after.as_ref()) {
        let anchor_value = JsValue::from(anchor.as_str());
        let sections = sections_of_menu(menu);
        if let Some(index) = sections
            .iter()
            .position(|section| includes(&items_of_menu_section(section), &anchor_value))
        {
            let items = items_of_menu_section(&sections[index]);
            let at =
                items.iter().position(|each| JsValue::same(each, &anchor_value)).expect("included");
            let cut = if options.before.is_some() { at } else { at + 1 };
            let mut next_items = items[..cut].to_vec();
            next_items.push(command);
            next_items.extend_from_slice(&items[cut..]);
            let next_menu: Vec<JsValue> = sections
                .iter()
                .enumerate()
                .map(|(i, section)| {
                    if i == index {
                        with_menu_section_items(section, next_items.clone()).expect("items")
                    } else {
                        section.clone()
                    }
                })
                .collect();
            return (with_record(data, "menus", menu_id, JsValue::array(next_menu)), Vec::new());
        }
        warnings.push(not_found("add", &anchor_value, menu_id, &item_ids_of_menu(menu)));
    }
    let mut sections = sections_of_menu(menu);
    sections.push(JsValue::array(vec![command]));
    (with_record(data, "menus", menu_id, JsValue::array(sections)), warnings)
}

fn remove_items(data: &JsValue, ids: &[&str], options: &RemoveItemOptions) -> Edited {
    let from = options.from.as_str();
    if from.is_empty() {
        let listed: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
        let warning = format!(
            "remove: {} needs a bar or menu to come out of ({{ from }}); \
             to leave something out everywhere, turn it off with `disable`",
            listed.join(", ")
        );
        return (data.clone(), vec![warning]);
    }
    let gone = |id: &JsValue| id.as_str().is_some_and(|id| ids.contains(&id));
    let mut found: Vec<JsValue> = Vec::new();
    let mut keep = |id: Option<JsValue>| {
        let Some(id) = id.filter(gone) else { return true };
        if !includes(&found, &id) {
            found.push(id);
        }
        false
    };
    let missing = |found: &[JsValue]| -> Vec<JsValue> {
        ids.iter().map(|id| JsValue::from(*id)).filter(|id| !includes(found, id)).collect()
    };

    if let Some(bar) = bars_of(data).get(from).filter(JsValue::truthy) {
        let next_bar = map_entries(&bar, |entry, _, _| {
            let items =
                items_of_entry(entry).into_iter().filter(|each| keep(item_id_of(each))).collect();
            with_entry_items(entry, items)
        });
        let candidates = item_ids_of_bar(&bar);
        let warnings =
            missing(&found).iter().map(|id| not_found("remove", id, from, &candidates)).collect();
        let next =
            if found.is_empty() { data.clone() } else { with_record(data, "bars", from, next_bar) };
        return (next, warnings);
    }
    if let Some(menu) = menus_of(data).get(from).filter(JsValue::truthy) {
        let next_menu: Vec<JsValue> = sections_of_menu(&menu)
            .iter()
            .filter_map(|section| {
                let items = items_of_menu_section(section)
                    .into_iter()
                    .filter(|id| keep(Some(id.clone())))
                    .collect();
                with_menu_section_items(section, items)
            })
            .collect();
        let candidates = item_ids_of_menu(&menu);
        let warnings =
            missing(&found).iter().map(|id| not_found("remove", id, from, &candidates)).collect();
        let next = if found.is_empty() {
            data.clone()
        } else {
            with_record(data, "menus", from, JsValue::array(next_menu))
        };
        return (next, warnings);
    }
    let mut places = bars_of(data).keys();
    places.extend(menus_of(data).keys());
    let warning = format!(
        "remove: no bar or menu \"{from}\"{}",
        did_you_mean(from, places.iter().map(String::as_str))
    );
    (data.clone(), vec![warning])
}

fn replace_item(data: &JsValue, id: &str, item: &JsValue) -> Edited {
    if !is_layout_item(item) {
        return (data.clone(), vec![format!("replace: {} is not an item", json_stringify(item))]);
    }
    let id_value = JsValue::from(id);
    let mut found = false;
    let mut bars = Record::new();
    for (bar_id, bar) in bars_of(data).entries() {
        if !includes(&item_ids_of_bar(&bar), &id_value) {
            bars.insert(bar_id, bar);
            continue;
        }
        found = true;
        let next_bar = map_entries(&bar, |entry, _, _| {
            let items = items_of_entry(entry)
                .into_iter()
                .map(|each| if has_id(&each, &id_value) { item.clone() } else { each })
                .collect();
            with_entry_items(entry, items)
        });
        bars.insert(bar_id, next_bar);
    }
    // A menu row is a command: your own component has no place there.
    let command = menu_command_of(item);
    let mut menus = Record::new();
    for (menu_id, menu) in menus_of(data).entries() {
        let Some(command) =
            command.as_ref().filter(|_| includes(&item_ids_of_menu(&menu), &id_value))
        else {
            menus.insert(menu_id, menu);
            continue;
        };
        found = true;
        let sections = sections_of_menu(&menu)
            .iter()
            .map(|section| {
                let items = items_of_menu_section(section)
                    .into_iter()
                    .map(
                        |each| if JsValue::same(&each, &id_value) { command.clone() } else { each },
                    )
                    .collect();
                // The TypeScript asserts a section is left (`!`); a section
                // with no items stays as `null`, as it does there.
                with_menu_section_items(section, items).unwrap_or(JsValue::Null)
            })
            .collect();
        menus.insert(menu_id, JsValue::array(sections));
    }
    if !found {
        let mut everywhere: Vec<JsValue> = Vec::new();
        for (_, bar) in bars_of(data).entries() {
            everywhere.extend(item_ids_of_bar(&bar));
        }
        for (_, menu) in menus_of(data).entries() {
            everywhere.extend(item_ids_of_menu(&menu));
        }
        let warning = format!(
            "replace: \"{id}\" is in no bar or menu{}",
            did_you_mean(id, names(&everywhere))
        );
        return (data.clone(), vec![warning]);
    }
    let mut next = spread(Some(data));
    next.insert("bars", JsValue::object(bars));
    next.insert("menus", JsValue::object(menus));
    (JsValue::object(next), Vec::new())
}

/// A panel's `present` as a map of sizes: one way at every size, or the
/// sizes it names.
pub fn panel_presentations_of(present: &JsValue) -> JsValue {
    match present {
        JsValue::String(_) => JsValue::object(
            ["compact", "medium", "expanded"]
                .into_iter()
                .map(|size| (size, present.clone()))
                .collect(),
        ),
        _ => present.clone(),
    }
}

/// A panel's `width` as its fields: a number is where it starts.
pub fn panel_width_fields_of(width: &JsValue) -> JsValue {
    match width {
        JsValue::Number(_) => JsValue::object([("initial", width.clone())].into_iter().collect()),
        _ => width.clone(),
    }
}

/// `panel` with `changes`: each field replaced, except `present` and
/// `width`, which change only what the change names.
fn changed_panel(panel: &JsValue, changes: &JsValue) -> JsValue {
    let mut next = spread(Some(panel));
    for (field, value) in changes.entries() {
        next.insert(field, value);
    }
    let present = changes.get("present");
    if let (Some(present), Some(old)) = (&present, panel.get("present"))
        && present.type_of() == "object"
    {
        let mut sizes = spread(Some(&panel_presentations_of(&old)));
        for (size, way) in spread(Some(present)).iter() {
            sizes.insert(size, way.clone());
        }
        next.insert("present", JsValue::object(sizes));
    }
    if let (Some(width), Some(old)) = (changes.get("width"), panel.get("width"))
        && (width.type_of() == "object" || old.type_of() == "object")
    {
        let mut fields = spread(Some(&panel_width_fields_of(&old)));
        for (field, value) in spread(Some(&panel_width_fields_of(&width))).iter() {
            fields.insert(field, value.clone());
        }
        next.insert("width", JsValue::object(fields));
    }
    JsValue::object(next)
}

fn update_panel(data: &JsValue, id: &str, changes: &JsValue) -> Edited {
    let panels = panels_of(data);
    let Some(panel) = panels.get(id).filter(JsValue::truthy) else {
        let warning = format!(
            "panel: no panel \"{id}\"{}",
            did_you_mean(id, panels.keys().iter().map(String::as_str))
        );
        return (data.clone(), vec![warning]);
    };
    (with_record(data, "panels", id, changed_panel(&panel, changes)), Vec::new())
}

fn update_panels(data: &JsValue, changes: &JsValue) -> JsValue {
    let panels: Record<JsValue> = panels_of(data)
        .entries()
        .into_iter()
        .map(|(id, panel)| (id, changed_panel(&panel, changes)))
        .collect();
    let mut next = spread(Some(data));
    next.insert("panels", JsValue::object(panels));
    JsValue::object(next)
}

fn remove_panel(data: &JsValue, id: &str) -> Edited {
    let panels = panels_of(data);
    if !panels.get(id).is_some_and(|panel| panel.truthy()) {
        let warning = format!(
            "removePanel: no panel \"{id}\"{}",
            did_you_mean(id, panels.keys().iter().map(String::as_str))
        );
        return (data.clone(), vec![warning]);
    }
    // Its button goes with it, from every bar and menu.
    let button = JsValue::from(format!("panel:{id}"));
    let bars: Record<JsValue> = bars_of(data)
        .entries()
        .into_iter()
        .map(|(bar_id, bar)| {
            if !includes(&item_ids_of_bar(&bar), &button) {
                return (bar_id, bar);
            }
            let next_bar = map_entries(&bar, |entry, _, _| {
                let items = items_of_entry(entry)
                    .into_iter()
                    .filter(|each| !has_id(each, &button))
                    .collect();
                with_entry_items(entry, items)
            });
            (bar_id, next_bar)
        })
        .collect();
    let menus: Record<JsValue> = menus_of(data)
        .entries()
        .into_iter()
        .map(|(menu_id, menu)| {
            if !includes(&item_ids_of_menu(&menu), &button) {
                return (menu_id, menu);
            }
            let sections = sections_of_menu(&menu)
                .iter()
                .filter_map(|section| {
                    let items = items_of_menu_section(section)
                        .into_iter()
                        .filter(|each| !JsValue::same(each, &button))
                        .collect();
                    with_menu_section_items(section, items)
                })
                .collect();
            (menu_id, JsValue::array(sections))
        })
        .collect();
    let mut next = spread(Some(&without_record(data, "panels", id)));
    next.insert("bars", JsValue::object(bars));
    next.insert("menus", JsValue::object(menus));
    (JsValue::object(next), Vec::new())
}

// ── the viewer's size ────────────────────────────────────────────────────────

/// The viewer's size class, from its own width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerSize {
    Compact,
    Medium,
    Expanded,
}

impl ViewerSize {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewerSize::Compact => "compact",
            ViewerSize::Medium => "medium",
            ViewerSize::Expanded => "expanded",
        }
    }
}

/// How the user's language reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

/// What the layout function knows about the viewer, measured on the viewer
/// itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutEnv {
    /// `Compact` under 600 px, `Medium` to 1023 px, `Expanded` from 1024 px.
    pub size: ViewerSize,
    /// The viewer's width, in px.
    pub width: f64,
    /// Whether the main way of pointing is a finger.
    pub touch: bool,
    pub dir: TextDirection,
}

/// The narrowest a medium viewer is, in px.
pub const MEDIUM_MIN_WIDTH_PX: f64 = 600.0;
/// The narrowest an expanded viewer is, in px.
pub const EXPANDED_MIN_WIDTH_PX: f64 = 1024.0;

/// The size class of a viewer this wide.
pub fn size_of(width_px: f64) -> ViewerSize {
    if width_px < MEDIUM_MIN_WIDTH_PX {
        return ViewerSize::Compact;
    }
    if width_px < EXPANDED_MIN_WIDTH_PX { ViewerSize::Medium } else { ViewerSize::Expanded }
}

/// Whether two envs are the same: the layout function runs again only when
/// one of the fields changes.
pub fn same_layout_env(left: &LayoutEnv, right: &LayoutEnv) -> bool {
    left.size == right.size
        && left.width == right.width
        && left.touch == right.touch
        && left.dir == right.dir
}

// ── the layout option ────────────────────────────────────────────────────────

/// What a layout function gives back: a layout, plain data, or nothing.
#[derive(Clone, Debug)]
pub enum LayoutResult {
    Layout(Layout),
    Data(JsValue),
    /// `undefined`.
    Nothing,
}

/// Gets the default layout and the viewer's env, and returns your layout. An
/// `Err` is the function throwing, with its message.
pub type LayoutFunction = Rc<dyn Fn(&Layout, &LayoutEnv) -> Result<LayoutResult, String>>;

/// What the `layout` config takes: a layout, plain layout data, or a
/// function of the default.
#[derive(Clone)]
pub enum LayoutOption {
    Layout(Layout),
    Data(JsValue),
    Function(LayoutFunction),
}

/// The layout an option gives at this env: the default without one, the
/// function's result, or the option itself. A function that throws or
/// returns no layout gives the default, with a warning saying so.
pub fn resolve_layout(option: Option<&LayoutOption>, defaults: &Layout, env: &LayoutEnv) -> Layout {
    match option {
        None => defaults.clone(),
        Some(LayoutOption::Function(function)) => match function(defaults, env) {
            Err(message) => with_layout_warning(
                defaults,
                format!("the layout function threw ({message}); showing the default layout"),
            ),
            Ok(result) => to_layout(result, defaults, "the layout function returned"),
        },
        Some(LayoutOption::Layout(layout)) => {
            to_layout(LayoutResult::Layout(layout.clone()), defaults, "layout is")
        }
        Some(LayoutOption::Data(data)) => {
            to_layout(LayoutResult::Data(data.clone()), defaults, "layout is")
        }
    }
}

fn to_layout(value: LayoutResult, defaults: &Layout, what: &str) -> Layout {
    let kind = match value {
        LayoutResult::Layout(layout) => return layout,
        LayoutResult::Data(data @ JsValue::Object(_)) => return create_layout(data),
        LayoutResult::Data(data) => data.type_of(),
        LayoutResult::Nothing => "nothing",
    };
    with_layout_warning(
        defaults,
        format!("{what} {kind}, not a layout; showing the default layout"),
    )
}
