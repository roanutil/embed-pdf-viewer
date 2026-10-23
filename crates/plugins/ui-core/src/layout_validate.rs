//! `layout-validate.ts`: checks a layout against the viewer it runs in. Every
//! id it names exists, every place is a place, and every bar it is placed
//! next to is where the place says. Each problem is one sentence with the id,
//! and the closest id that exists when there is one, for a console warning:
//! the viewer still starts with a layout that has problems, and leaves out
//! what it can't place.

use std::collections::HashSet;

use crate::layout::{
    LayoutInput, SECTION_NAMES, includes, is_group_object, is_layout_item, item_command_of,
    item_id_of, items_of_entry, items_of_menu_section,
};
use crate::layout_compile::{
    EDGES, VIEW_MODE_COMMAND, edge_of, host_of, mode_command_of, modes_of, panel_command_of,
};
use crate::suggestions::did_you_mean;
use crate::value::{JsValue, js_string, json_stringify};

/// Whether a command id exists that a list can't hold.
pub type HasCommand = Box<dyn Fn(&str) -> bool>;

/// What the viewer has, to check a layout's ids against.
#[derive(Default)]
pub struct LayoutKnownIds {
    /// Every command id the viewer has. The ones the layout makes itself need
    /// not be listed: the mode tabs (`mode:view`, `mode:<mode>`) and the panel
    /// buttons (`panel:<id>`).
    pub commands: Vec<String>,
    /// Whether a command id exists that `commands` can't list, such as one of
    /// a family's.
    pub has_command: Option<HasCommand>,
}

const FLOAT_POSITIONS: [&str; 8] =
    ["top-start", "top", "top-end", "start", "end", "bottom-start", "bottom", "bottom-end"];
const SELECTION_TARGETS: [&str; 2] = ["selection", "annotation"];
const PRESENTATIONS: [&str; 3] = ["dock", "overlay", "sheet"];
const SIZES: [&str; 3] = ["compact", "medium", "expanded"];
const WIDTH_FIELDS: [&str; 3] = ["initial", "min", "max"];
const TABS: [&str; 3] = ["always", "multiple", "never"];

fn is_one_of(value: &JsValue, names: &[&str]) -> bool {
    value.as_str().is_some_and(|s| names.contains(&s))
}

/// `didYouMean(id, candidates)`: an id that isn't a string has none.
fn suggest<'a>(id: &JsValue, candidates: impl IntoIterator<Item = &'a str>) -> String {
    id.as_str().map(|id| did_you_mean(id, candidates)).unwrap_or_default()
}

/// The commands a layout may name: the viewer's, in order, then the ones the
/// layout makes itself.
struct Commands<'a> {
    names: Vec<String>,
    set: HashSet<String>,
    known: &'a LayoutKnownIds,
}

impl Commands<'_> {
    fn add(&mut self, command: String) {
        if self.set.insert(command.clone()) {
            self.names.push(command);
        }
    }

    fn check(&self, problems: &mut Vec<String>, place: &str, command: &JsValue) {
        let known = command.as_str().is_some_and(|c| self.set.contains(c))
            || self.known.has_command.as_ref().is_some_and(|has| has(&js_string(command)));
        if !known {
            problems.push(format!(
                "{place}: no command \"{}\"{}",
                js_string(command),
                suggest(command, self.names.iter().map(String::as_str))
            ));
        }
    }
}

/// Every problem with a layout, as sentences: what its edits couldn't do
/// first, then what its data names that the viewer doesn't have. Empty when
/// there is none.
pub fn validate_layout<'a>(
    layout: impl Into<LayoutInput<'a>>,
    known: &LayoutKnownIds,
) -> Vec<String> {
    let (mut problems, data) = match layout.into() {
        LayoutInput::Layout(layout) => (layout.warnings().to_vec(), layout.to_json()),
        LayoutInput::Data(data) => (Vec::new(), data.clone()),
    };
    if !data.is_object() {
        problems.push("the layout is not an object".to_string());
        return problems;
    }

    let bars = crate::layout::bars_of(&data);
    let known_commands: HashSet<&str> = known.commands.iter().map(String::as_str).collect();
    let mut commands = Commands { names: Vec::new(), set: HashSet::new(), known };
    for command in &known.commands {
        commands.add(command.clone());
    }
    let modes = modes_of(&data);
    for mode in &modes {
        commands.add(mode_command_of(mode));
    }
    if !modes.is_empty() {
        commands.add(VIEW_MODE_COMMAND.to_string());
    }
    for panel in crate::layout::panels_of(&data).keys() {
        commands.add(panel_command_of(&panel));
    }

    for (id, bar) in bars.entries() {
        let place = format!("bar \"{id}\"");
        if !bar.is_object() {
            problems.push(format!("{place} is not a bar"));
            continue;
        }
        problems.extend(place_problems(&place, &id, bar.get("place").as_ref(), &bars));
        if let Some(mode) = bar.get("mode")
            && bar.get("title").is_none()
            && bar.get("titleKey").is_none()
            && !known_commands.contains(mode_command_of(&js_string(&mode)).as_str())
        {
            let mode = js_string(&mode);
            problems.push(format!(
                "{place}: its mode \"{mode}\" has no title, so its tab reads \"{mode}\""
            ));
        }
        problems.extend(item_problems(&place, &bar, &commands));
    }

    for (id, menu) in crate::layout::menus_of(&data).entries() {
        problems.extend(menu_problems(&format!("menu \"{id}\""), &menu, &commands));
    }

    for (id, panel) in crate::layout::panels_of(&data).entries() {
        problems.extend(panel_problems(&format!("panel \"{id}\""), &panel));
    }

    if let Some(tabs) = data.get("tabs")
        && !is_one_of(&tabs, &TABS)
    {
        problems.push(format!(
            "tabs: \"{}\" is not one of always, multiple or never",
            js_string(&tabs)
        ));
    }

    problems
}

fn place_problems(
    place_of: &str,
    id: &str,
    place: Option<&JsValue>,
    bars: &JsValue,
) -> Vec<String> {
    let not_a_place = |shown: String| {
        format!(
            "{place_of}: {shown} is not a place (an edge, {{ edge, before | after }}, {{ float }}, {{ with }} or {{ at }})"
        )
    };
    let Some(place) = place else {
        return vec![format!("{place_of} has no place")];
    };
    if let JsValue::String(edge) = place {
        return if EDGES.contains(&&**edge) {
            Vec::new()
        } else {
            vec![not_a_place(format!("\"{edge}\""))]
        };
    }
    if !place.is_object() {
        return vec![not_a_place(js_string(place))];
    }

    if place.has("edge") {
        let edge = place.get("edge");
        if !edge.as_ref().is_some_and(|edge| is_one_of(edge, &EDGES)) {
            let shown = edge.as_ref().map(js_string).unwrap_or_else(|| "undefined".into());
            return vec![format!("{place_of}: \"{shown}\" is not an edge")];
        }
        let edge = edge.and_then(|edge| edge.as_str().map(str::to_string)).unwrap_or_default();
        let before = place.get("before");
        let after = place.get("after");
        // `before ?? after`: `null` is no anchor either.
        let present = |v: &Option<JsValue>| v.clone().filter(|v| !matches!(v, JsValue::Null));
        let Some(anchor) = present(&before).or_else(|| present(&after)) else {
            return Vec::new();
        };
        let side = if before.is_some() { "before" } else { "after" };
        let on_edge: Vec<JsValue> = bars
            .keys()
            .into_iter()
            .filter(|other| {
                other != id
                    && edge_of(bars.get(other).and_then(|bar| bar.get("place")).as_ref()).as_deref()
                        == Some(&edge)
            })
            .map(JsValue::from)
            .collect();
        if includes(&on_edge, &anchor) {
            return Vec::new();
        }
        let anchor_name = js_string(&anchor);
        if bars.get(&anchor_name).is_some_and(|bar| bar.truthy()) {
            return vec![format!(
                "{place_of} goes {side} \"{anchor_name}\", which is not on the {edge} edge"
            )];
        }
        let suggestion = suggest(&anchor, on_edge.iter().filter_map(JsValue::as_str));
        return vec![format!("{place_of} goes {side} \"{anchor_name}\": no such bar{suggestion}")];
    }
    if place.has("float") {
        let float = place.get("float").unwrap_or(JsValue::Null);
        if is_one_of(&float, &FLOAT_POSITIONS) {
            return Vec::new();
        }
        return vec![format!(
            "{place_of}: \"{}\" is not a floating position{}",
            js_string(&float),
            suggest(&float, FLOAT_POSITIONS)
        )];
    }
    if place.has("with") {
        let host = place.get("with").unwrap_or(JsValue::Null);
        if host.as_str() == Some(id) {
            return vec![format!("{place_of} rides along itself")];
        }
        let host_name = js_string(&host);
        if !bars.get(&host_name).is_some_and(|bar| bar.truthy()) {
            let keys = bars.keys();
            return vec![format!(
                "{place_of} rides along \"{host_name}\": no such bar{}",
                suggest(&host, keys.iter().map(String::as_str))
            )];
        }
        // A chain of `with` that comes back to this bar has nowhere to go.
        let mut seen = vec![JsValue::from(id)];
        let mut next = Some(host);
        while let Some(host) = next.filter(|host| !matches!(host, JsValue::Null)) {
            if includes(&seen, &host) {
                return vec![format!("{place_of} rides along a bar that rides along it")];
            }
            next = host_of(bars.get(&js_string(&host)).and_then(|bar| bar.get("place")).as_ref());
            seen.push(host);
        }
        return Vec::new();
    }
    if place.has("at") {
        let at = place.get("at").unwrap_or(JsValue::Null);
        if is_one_of(&at, &SELECTION_TARGETS) {
            return Vec::new();
        }
        return vec![format!("{place_of}: \"{}\" is not selection or annotation", js_string(&at))];
    }
    vec![not_a_place(json_stringify(place))]
}

fn item_problems(place: &str, bar: &JsValue, commands: &Commands) -> Vec<String> {
    let mut problems = Vec::new();
    let mut group_ids: Vec<JsValue> = Vec::new();
    let mut groups = 0usize;
    let mut dependent_groups = 0usize;
    for name in SECTION_NAMES {
        let Some(entries) = bar.get(name.as_str()) else { continue };
        let Some(entries) = entries.as_array() else {
            problems
                .push(format!("{place}: its {} section is not a list of groups", name.as_str()));
            continue;
        };
        for entry in entries {
            if is_group_object(entry)
                && let Some(id) = entry.get("id")
            {
                if includes(&group_ids, &id) {
                    problems.push(format!("{place}: two groups are called \"{}\"", js_string(&id)));
                }
                group_ids.push(id);
            }
            groups += 1;
            if is_group_object(entry)
                && let Some(dependent) = entry.get("dependent")
            {
                match dependent {
                    JsValue::Bool(true) => dependent_groups += 1,
                    JsValue::Bool(false) => {}
                    other => {
                        let group = match entry.get("id") {
                            None => "a group".to_string(),
                            Some(id) => format!("group \"{}\"", js_string(&id)),
                        };
                        problems.push(format!(
                            "{place}: {group} has dependent {}, which is not true or false",
                            json_stringify(&other)
                        ));
                    }
                }
            }
            let mut seen: Vec<JsValue> = Vec::new();
            for item in items_of_entry(entry) {
                if !is_layout_item(&item) {
                    problems.push(format!("{place}: {} is not an item", json_stringify(&item)));
                    continue;
                }
                // A layout item always has an id.
                let id = item_id_of(&item).unwrap_or(JsValue::Null);
                if includes(&seen, &id) {
                    problems.push(format!("{place}: \"{}\" is twice in one group", js_string(&id)));
                }
                seen.push(id);
                if let Some(command) = item_command_of(&item) {
                    commands.check(&mut problems, place, &command);
                }
            }
        }
    }
    if groups > 0 && dependent_groups == groups {
        problems.push(format!("{place}: every group in it is dependent, so it never shows"));
    }
    problems
}

fn menu_problems(place: &str, menu: &JsValue, commands: &Commands) -> Vec<String> {
    let Some(sections) = menu.as_array() else {
        return vec![format!("{place} is not a list of sections")];
    };
    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for section in sections {
        for command in items_of_menu_section(section) {
            let Some(name) = command.as_str() else {
                problems.push(format!("{place}: {} is not a command id", json_stringify(&command)));
                continue;
            };
            if seen.iter().any(|s| s == name) {
                problems.push(format!("{place}: \"{name}\" is in it twice"));
            }
            seen.push(name.to_string());
            commands.check(&mut problems, place, &command);
        }
    }
    problems
}

fn is_width_px(value: &JsValue) -> bool {
    value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0)
}

fn present_problems(place: &str, present: &JsValue) -> Vec<String> {
    if let JsValue::String(way) = present {
        return if PRESENTATIONS.contains(&&**way) {
            Vec::new()
        } else {
            vec![format!("{place}: present \"{way}\" is not dock, overlay or sheet")]
        };
    }
    if !matches!(present, JsValue::Object(_)) {
        return vec![format!(
            "{place}: present {} is not a way to open, or one per size",
            json_stringify(present)
        )];
    }
    let mut problems = Vec::new();
    for (size, way) in present.entries() {
        if !SIZES.contains(&size.as_str()) {
            problems.push(format!(
                "{place}: present has \"{size}\", not a size{}",
                did_you_mean(&size, SIZES)
            ));
        } else if !is_one_of(&way, &PRESENTATIONS) {
            problems.push(format!(
                "{place}: present.{size} \"{}\" is not dock, overlay or sheet",
                js_string(&way)
            ));
        }
    }
    problems
}

fn width_problems(place: &str, width: &JsValue) -> Vec<String> {
    if let JsValue::Number(n) = width {
        return if is_width_px(width) {
            Vec::new()
        } else {
            vec![format!(
                "{place}: width {} is not a width in px",
                epdf_kernel::js::number_to_string(*n)
            )]
        };
    }
    if !matches!(width, JsValue::Object(_)) {
        return vec![format!(
            "{place}: width {} is not a width in px, or {{ initial, min, max }}",
            json_stringify(width)
        )];
    }
    let mut problems = Vec::new();
    for (field, value) in width.entries() {
        if !WIDTH_FIELDS.contains(&field.as_str()) {
            problems.push(format!("{place}: width has \"{field}\", not initial, min or max"));
        } else if !is_width_px(&value) {
            problems.push(format!(
                "{place}: width.{field} {} is not a width in px",
                json_stringify(&value)
            ));
        }
    }
    if let (Some(min), Some(max)) = (width.get("min"), width.get("max"))
        && is_width_px(&min)
        && is_width_px(&max)
        && min.as_f64() > max.as_f64()
    {
        problems.push(format!(
            "{place}: width.min {} is wider than width.max {}",
            js_string(&min),
            js_string(&max)
        ));
    }
    problems
}

fn panel_problems(place: &str, panel: &JsValue) -> Vec<String> {
    if !panel.is_object() {
        return vec![format!("{place} is not a panel")];
    }
    let mut problems = Vec::new();
    if let Some(side) = panel.get("side")
        && !is_one_of(&side, &["start", "end"])
    {
        problems.push(format!("{place}: side \"{}\" is not start or end", js_string(&side)));
    }
    if let Some(present) = panel.get("present") {
        problems.extend(present_problems(place, &present));
    }
    if let Some(width) = panel.get("width") {
        problems.extend(width_problems(place, &width));
    }
    if let Some(resizable) = panel.get("resizable")
        && !matches!(resizable, JsValue::Bool(_))
    {
        problems.push(format!(
            "{place}: resizable {} is not true or false",
            json_stringify(&resizable)
        ));
    }
    problems
}
