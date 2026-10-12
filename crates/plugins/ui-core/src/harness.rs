//! The conformance adapter: every function the `core/ui-*.json` vectors call,
//! by the name the generator (`tooling/plugin-conformance/src/suites/ui-core.ts`)
//! gives it. Where the TypeScript takes a callback, the vector carries the
//! callback's answers as a table, and this module builds the callback from
//! it the way the generator does.

use std::collections::HashSet;

use epdf_kernel::Error;
use plugin_conformance::{FunctionTable, scrub};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::helpers::{CHROME_HELPER_KEYS, CHROME_HELPERS};
use crate::js::{Record, number_from_value};
use crate::projection::{MenuTarget, project_overflow, project_shed, project_strip};
use crate::schema::{
    BarSchema, DEFAULT_IMPORTANCE, NormalizedBar, PINNED, custom, define_chrome, filter_bar, group,
    item, normalize_bar, validate_chrome,
};
use crate::shortcuts::{ShortcutOptions, format_shortcut, match_shortcut, parse_shortcut};
use crate::solver::{FitMetrics, solve};
use crate::transforms::{add_item, remove_items, replace_item};

/// Argument `i`, decoded: `undefined` members dropped, and an absent or
/// `undefined` argument read as `null` (so an `Option` is `None`).
fn arg<T: DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    let value = match args.get(i) {
        Some(v) if !epdf_kernel::is_undefined(v) => scrub(v),
        _ => Value::Null,
    };
    serde_json::from_value(value).map_err(|e| Error::new(format!("[harness] argument {i}: {e}")))
}

/// Argument `i` as given, `null` when absent, as [`arg`] reads it: a
/// malformed vector fails the case instead of panicking the runner.
fn raw(args: &[Value], i: usize) -> &Value {
    static NULL: Value = Value::Null;
    args.get(i).unwrap_or(&NULL)
}

fn out<T: Serialize>(value: T) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|e| Error::new(format!("[harness] result: {e}")))
}

/// The generator's `metricsFrom`: widths per key and variant, then per
/// variant; collapsed and trigger widths per group, then a default. A width
/// the table lacks is a missing measurement. A missing scalar is
/// `undefined`, which arithmetic turns into `NaN`.
struct TableMetrics(Value);

impl FitMetrics for TableMetrics {
    fn unit(&self, key: &str, variant: &str) -> Option<f64> {
        number_from_value(&self.0["units"][key][variant])
            .or_else(|| number_from_value(&self.0["variants"][variant]))
    }
    fn group_collapsed(&self, group_id: &str) -> Option<f64> {
        number_from_value(&self.0["collapsed"][group_id])
            .or_else(|| number_from_value(&self.0["collapsedDefault"]))
    }
    fn group_trigger(&self, group_id: &str) -> Option<f64> {
        number_from_value(&self.0["triggers"][group_id])
            .or_else(|| number_from_value(&self.0["triggerDefault"]))
    }
    fn overflow_trigger(&self) -> f64 {
        number_from_value(&self.0["overflowTrigger"]).unwrap_or(f64::NAN)
    }
    fn gap(&self) -> f64 {
        number_from_value(&self.0["gap"]).unwrap_or(f64::NAN)
    }
    fn separator(&self) -> f64 {
        number_from_value(&self.0["separator"]).unwrap_or(f64::NAN)
    }
}

fn number_arg(args: &[Value], i: usize) -> Result<f64, Error> {
    args.get(i)
        .and_then(number_from_value)
        .ok_or_else(|| Error::new(format!("[harness] argument {i}: expected a number")))
}

/// The generator's `resolveFrom`: an id the table lacks, or maps to null,
/// resolves to null.
fn resolver(table: &Value) -> impl Fn(&str) -> Option<MenuTarget> + '_ {
    move |id| match table.get(id) {
        None | Some(Value::Null) => None,
        Some(target) => serde_json::from_value(scrub(target)).ok(),
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

fn constants(_: &[Value]) -> Result<Value, Error> {
    Ok(json!({ "PINNED": PINNED, "DEFAULT_IMPORTANCE": DEFAULT_IMPORTANCE }))
}

fn item_fn(args: &[Value]) -> Result<Value, Error> {
    out(item(&arg::<String>(args, 0)?, arg(args, 1)?))
}

fn custom_fn(args: &[Value]) -> Result<Value, Error> {
    out(custom(&arg::<String>(args, 0)?, arg(args, 1)?))
}

fn group_fn(args: &[Value]) -> Result<Value, Error> {
    out(group(&arg::<String>(args, 0)?, arg(args, 1)?, arg(args, 2)?))
}

fn normalize_bar_fn(args: &[Value]) -> Result<Value, Error> {
    out(normalize_bar(&arg(args, 0)?)?)
}

fn filter_bar_fn(args: &[Value]) -> Result<Value, Error> {
    let keep = strings(raw(args, 1));
    out(filter_bar(&arg(args, 0)?, |u| keep.iter().any(|k| k == u.key())))
}

fn define_chrome_fn(args: &[Value]) -> Result<Value, Error> {
    out(define_chrome(arg(args, 0)?)?)
}

fn validate_chrome_fn(args: &[Value]) -> Result<Value, Error> {
    let known: HashSet<String> = strings(&raw(args, 1)["$set"]).into_iter().collect();
    out(validate_chrome(&arg(args, 0)?, &known)?)
}

fn solve_fn(args: &[Value]) -> Result<Value, Error> {
    let bar: NormalizedBar = arg(args, 0)?;
    out(solve(&bar, &TableMetrics(raw(args, 1).clone()), number_arg(args, 2)?))
}

fn solve_schema_fn(args: &[Value]) -> Result<Value, Error> {
    let bar = normalize_bar(&arg::<BarSchema>(args, 0)?)?;
    out(solve(&bar, &TableMetrics(raw(args, 1).clone()), number_arg(args, 2)?))
}

fn project_overflow_fn(args: &[Value]) -> Result<Value, Error> {
    out(project_overflow(&arg(args, 0)?, &arg(args, 1)?, resolver(raw(args, 2))))
}

fn project_shed_fn(args: &[Value]) -> Result<Value, Error> {
    out(project_shed(&arg(args, 0)?, &arg(args, 1)?, resolver(raw(args, 2))))
}

fn project_strip_fn(args: &[Value]) -> Result<Value, Error> {
    let visible = strings(raw(args, 1));
    out(project_strip(&arg(args, 0)?, |c| visible.iter().any(|v| v == c)))
}

fn solve_and_project_fn(args: &[Value]) -> Result<Value, Error> {
    let bar = normalize_bar(&arg::<BarSchema>(args, 0)?)?;
    let fit = solve(&bar, &TableMetrics(raw(args, 1).clone()), number_arg(args, 2)?);
    let mut shed = Record::new();
    for section in &bar.sections {
        for g in &section.groups {
            let rows = project_shed(g, &fit, resolver(raw(args, 3)));
            if !rows.is_empty() {
                shed.insert(g.id.clone(), rows);
            }
        }
    }
    let overflow = project_overflow(&bar, &fit, resolver(raw(args, 3)));
    out(json!({ "fit": out(fit)?, "overflow": out(overflow)?, "shed": out(shed)? }))
}

fn add_item_fn(args: &[Value]) -> Result<Value, Error> {
    out(add_item(&arg(args, 0)?, &arg(args, 1)?)?)
}

fn remove_items_fn(args: &[Value]) -> Result<Value, Error> {
    out(remove_items(&arg(args, 0)?, &arg::<Vec<String>>(args, 1)?))
}

fn replace_item_fn(args: &[Value]) -> Result<Value, Error> {
    out(replace_item(&arg(args, 0)?, &arg::<String>(args, 1)?, &arg(args, 2)?))
}

fn parse_shortcut_fn(args: &[Value]) -> Result<Value, Error> {
    out(parse_shortcut(&arg::<String>(args, 0)?)?)
}

fn match_shortcut_fn(args: &[Value]) -> Result<Value, Error> {
    out(match_shortcut(&arg(args, 0)?, &arg(args, 1)?, arg(args, 2)?))
}

fn match_text_fn(args: &[Value]) -> Result<Value, Error> {
    let parsed = parse_shortcut(&arg::<String>(args, 0)?)?;
    out(match_shortcut(&parsed, &arg(args, 1)?, ShortcutOptions { is_mac: arg(args, 2)? }))
}

fn format_shortcut_fn(args: &[Value]) -> Result<Value, Error> {
    out(format_shortcut(&arg::<String>(args, 0)?, arg(args, 1)?)?)
}

fn helper_keys(_: &[Value]) -> Result<Value, Error> {
    out(CHROME_HELPER_KEYS)
}

/// Each member of the bundle is the module's own function: in Rust the
/// bundle is built from them by name, so each is so by construction.
fn helper_identity(_: &[Value]) -> Result<Value, Error> {
    Ok(Value::Object(CHROME_HELPER_KEYS.iter().map(|k| (k.to_string(), json!(true))).collect()))
}

fn helper_call(args: &[Value]) -> Result<Value, Error> {
    let name: String = arg(args, 0)?;
    let rest = args.get(1..).unwrap_or_default();
    let h = CHROME_HELPERS;
    match name.as_str() {
        "addItem" => out((h.add_item)(&arg(rest, 0)?, &arg(rest, 1)?)?),
        "removeItems" => out((h.remove_items)(&arg(rest, 0)?, &arg::<Vec<String>>(rest, 1)?)),
        "replaceItem" => {
            out((h.replace_item)(&arg(rest, 0)?, &arg::<String>(rest, 1)?, &arg(rest, 2)?))
        }
        "item" => out((h.item)(&arg::<String>(rest, 0)?, arg(rest, 1)?)),
        "group" => out((h.group)(&arg::<String>(rest, 0)?, arg(rest, 1)?, arg(rest, 2)?)),
        "custom" => out((h.custom)(&arg::<String>(rest, 0)?, arg(rest, 1)?)),
        "defineChrome" => out((h.define_chrome)(arg(rest, 0)?)?),
        other => Err(Error::new(format!("[harness] no helper \"{other}\""))),
    }
}

pub fn functions() -> FunctionTable {
    vec![
        ("constants", constants),
        ("item", item_fn),
        ("custom", custom_fn),
        ("group", group_fn),
        ("normalizeBar", normalize_bar_fn),
        ("filterBar", filter_bar_fn),
        ("defineChrome", define_chrome_fn),
        ("validateChrome", validate_chrome_fn),
        ("solve", solve_fn),
        ("solveSchema", solve_schema_fn),
        ("projectOverflow", project_overflow_fn),
        ("projectShed", project_shed_fn),
        ("projectStrip", project_strip_fn),
        ("solveAndProject", solve_and_project_fn),
        ("addItem", add_item_fn),
        ("removeItems", remove_items_fn),
        ("replaceItem", replace_item_fn),
        ("parseShortcut", parse_shortcut_fn),
        ("matchShortcut", match_shortcut_fn),
        ("matchText", match_text_fn),
        ("formatShortcut", format_shortcut_fn),
        ("keys", helper_keys),
        ("identity", helper_identity),
        ("call", helper_call),
    ]
}
