//! `transforms.ts`: surgical edits over a `ChromeSchema`. Bars are addressed
//! by id or by collection key across `bars`, `modeBars` and `strips`.

use std::collections::HashSet;

use epdf_kernel::Error;
use serde::{Deserialize, Serialize};

use crate::js::{self, Record};
use crate::schema::{
    BarChild, BarGroup, BarSchema, BarSections, ChromeSchema, GroupOptsOrItems, MenuSchema,
    MenuSection, SectionName, group as make_group,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddItemSpec {
    /// Bar id, or a collection key such as `mode:annotate`.
    pub bar: String,
    pub section: SectionName,
    /// Target group id; unknown in the bar creates a group at the end of
    /// `section`.
    pub group: String,
    pub item: BarChild,
    /// Insert position within the group, with `splice`'s rules. Default:
    /// append.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "js::opt_number")]
    pub at: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Collection {
    Bars,
    ModeBars,
    Strips,
}

impl Collection {
    const ALL: [Collection; 3] = [Collection::Bars, Collection::ModeBars, Collection::Strips];

    fn name(self) -> &'static str {
        match self {
            Collection::Bars => "bars",
            Collection::ModeBars => "modeBars",
            Collection::Strips => "strips",
        }
    }

    fn of(self, schema: &ChromeSchema) -> Option<&Record<BarSchema>> {
        match self {
            Collection::Bars => Some(&schema.bars),
            Collection::ModeBars => schema.mode_bars.as_ref(),
            Collection::Strips => schema.strips.as_ref(),
        }
    }
}

fn command_of(child: &BarChild) -> Option<&str> {
    match child {
        BarChild::Command(c) => Some(c),
        BarChild::Item(i) => Some(&i.command),
        BarChild::Custom(_) => None,
    }
}

fn locate(schema: &ChromeSchema, id: &str) -> Vec<(Collection, String)> {
    let mut hits = Vec::new();
    for coll in Collection::ALL {
        for (key, bar) in coll.of(schema).into_iter().flat_map(Record::iter) {
            if bar.id == id || key == id {
                hits.push((coll, key.to_string()));
            }
        }
    }
    hits
}

/// Insert `spec.item` into a group of one bar.
pub fn add_item(schema: &ChromeSchema, spec: &AddItemSpec) -> Result<ChromeSchema, Error> {
    let hits = locate(schema, &spec.bar);
    if hits.is_empty() {
        return Err(Error::new(format!(
            "[ui-core] addItem: no bar \"{}\" in bars/modeBars/strips",
            spec.bar
        )));
    }
    if hits.len() > 1 {
        let names: Vec<&str> = hits.iter().map(|(c, _)| c.name()).collect();
        return Err(Error::new(format!(
            "[ui-core] addItem: bar id \"{}\" is ambiguous ({})",
            spec.bar,
            names.join(", ")
        )));
    }
    let (coll, key) = &hits[0];
    let bar = coll.of(schema).and_then(|r| r.get(key)).expect("located");

    for name in SectionName::ALL {
        if name == spec.section {
            continue;
        }
        if bar.sections.get(name).is_some_and(|gs| gs.iter().any(|g| g.id == spec.group)) {
            return Err(Error::new(format!(
                "[ui-core] addItem: group \"{}\" lives in section \"{}\", not \"{}\"",
                spec.group,
                name.as_str(),
                spec.section.as_str()
            )));
        }
    }

    let groups = bar.sections.get(spec.section).cloned().unwrap_or_default();
    let next_groups = match groups.iter().position(|g| g.id == spec.group) {
        None => {
            let mut next = groups;
            next.push(make_group(
                &spec.group,
                GroupOptsOrItems::Items(vec![spec.item.clone()]),
                None,
            ));
            next
        }
        Some(idx) => {
            let mut next = groups;
            let items = &mut next[idx].items;
            let at = spec.at.map_or(items.len(), |at| js::splice_start(at, items.len()));
            items.insert(at, spec.item.clone());
            next
        }
    };

    let mut sections = bar.sections.clone();
    sections.set(spec.section, Some(next_groups));
    let next_bar = BarSchema { id: bar.id.clone(), sections };

    let mut out = schema.clone();
    let target = match coll {
        Collection::Bars => &mut out.bars,
        Collection::ModeBars => out.mode_bars.as_mut().expect("located"),
        Collection::Strips => out.strips.as_mut().expect("located"),
    };
    target.insert(key.clone(), next_bar);
    Ok(out)
}

/// Rebuild `sections` from the three names in order, keeping what `f`
/// returns (`Object.fromEntries(SECTION_NAMES.flatMap(...))`).
fn rebuild_sections(
    sections: &BarSections,
    mut f: impl FnMut(Option<&Vec<BarGroup>>) -> Option<Vec<BarGroup>>,
) -> BarSections {
    let mut out = BarSections::default();
    for name in SectionName::ALL {
        out.set(name, f(sections.get(name)));
    }
    out
}

/// Purge commands (or custom slots, by slot name) from every bar, mode bar,
/// strip and menu. Groups and menu sections that empty out are dropped;
/// emptied menus keep their key.
pub fn remove_items(schema: &ChromeSchema, ids: &[String]) -> ChromeSchema {
    let gone: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let keep = |child: &BarChild| -> bool {
        match child {
            BarChild::Custom(c) => !gone.contains(c.slot.as_str()),
            other => !gone.contains(command_of(other).unwrap_or_default()),
        }
    };
    let filter_bar = |bar: &BarSchema| BarSchema {
        id: bar.id.clone(),
        sections: rebuild_sections(&bar.sections, |groups| {
            let groups: Vec<BarGroup> = groups?
                .iter()
                .map(|g| BarGroup {
                    items: g.items.iter().filter(|c| keep(c)).cloned().collect(),
                    ..g.clone()
                })
                .filter(|g| !g.items.is_empty())
                .collect();
            (!groups.is_empty()).then_some(groups)
        }),
    };
    ChromeSchema {
        bars: schema.bars.map_values(filter_bar),
        mode_bars: schema.mode_bars.as_ref().map(|c| c.map_values(filter_bar)),
        strips: schema.strips.as_ref().map(|c| c.map_values(filter_bar)),
        menus: schema.menus.as_ref().map(|menus| {
            menus.map_values(|menu| MenuSchema {
                id: menu.id.clone(),
                sections: menu
                    .sections
                    .iter()
                    .map(|s| MenuSection {
                        label_key: s.label_key.clone(),
                        items: s
                            .items
                            .iter()
                            .filter(|c| !gone.contains(c.as_str()))
                            .cloned()
                            .collect(),
                    })
                    .filter(|s| !s.items.is_empty())
                    .collect(),
            })
        }),
        frame: schema.frame.clone(),
    }
}

/// Swap a command in place everywhere it appears: bars, mode bars, strips,
/// and, when the replacement is a plain command, menu rows. Custom items are
/// never matched.
pub fn replace_item(schema: &ChromeSchema, id: &str, item: &BarChild) -> ChromeSchema {
    let replacement_command = command_of(item);
    let swap =
        |child: &BarChild| if command_of(child) == Some(id) { item.clone() } else { child.clone() };
    let map_bar = |bar: &BarSchema| BarSchema {
        id: bar.id.clone(),
        sections: rebuild_sections(&bar.sections, |groups| {
            Some(
                groups?
                    .iter()
                    .map(|g| BarGroup { items: g.items.iter().map(swap).collect(), ..g.clone() })
                    .collect(),
            )
        }),
    };
    ChromeSchema {
        bars: schema.bars.map_values(map_bar),
        mode_bars: schema.mode_bars.as_ref().map(|c| c.map_values(map_bar)),
        strips: schema.strips.as_ref().map(|c| c.map_values(map_bar)),
        menus: match replacement_command {
            None => schema.menus.clone(),
            Some(replacement) => schema.menus.as_ref().map(|menus| {
                menus.map_values(|menu| MenuSchema {
                    id: menu.id.clone(),
                    sections: menu
                        .sections
                        .iter()
                        .map(|s| MenuSection {
                            label_key: s.label_key.clone(),
                            items: s
                                .items
                                .iter()
                                .map(|c| if c == id { replacement.to_string() } else { c.clone() })
                                .collect(),
                        })
                        .collect(),
                })
            }),
        },
        frame: schema.frame.clone(),
    }
}
