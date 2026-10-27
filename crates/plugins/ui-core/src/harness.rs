//! The conformance adapter: every function the `core/ui-*.json` vectors call,
//! by the name the generator (`tooling/plugin-conformance/src/suites/ui-core.ts`)
//! gives it. Where the TypeScript takes a callback, the vector carries the
//! callback's answers as a table, and this module builds the callback from
//! it the way the generator does.

use epdf_kernel::Error;
use plugin_conformance::{FunctionTable, scrub};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::js::{JsMap, Record, number_from_value};
use crate::layout::{
    AddItemOptions, Layout, RemoveItemOptions, TabsVisibility, create_layout, is_layout,
};
use crate::layout_validate::{LayoutKnownIds, validate_layout};
use crate::projection::{MenuTarget, project_overflow, project_shed, project_strip};
use crate::schema::{
    BarChild, BarSchema, CustomOpts, DEFAULT_IMPORTANCE, GroupOpts, ItemOpts, NormalizedBar,
    PINNED, custom, filter_bar, group, item, normalize_bar,
};
use crate::shortcuts::{ShortcutOptions, format_shortcut, match_shortcut, parse_shortcut};
use crate::solver::{FitMetrics, FitResult, GroupAssignment, UnitAssignment, solve};
use crate::value::JsValue;

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
    out(item(&arg::<String>(args, 0)?, arg::<Option<ItemOpts>>(args, 1)?))
}

/// The generator's `custom(slot, opts)`: `terminal` comes out of `opts` as
/// the current `custom(slot, terminal, options)` takes it.
fn custom_fn(args: &[Value]) -> Result<Value, Error> {
    #[derive(Deserialize)]
    struct Opts {
        terminal: Option<String>,
        #[serde(flatten)]
        options: CustomOpts,
    }
    let opts: Option<Opts> = arg(args, 1)?;
    let (terminal, options) = match opts {
        Some(Opts { terminal, options }) => (terminal, Some(options)),
        None => (None, None),
    };
    out(custom(&arg::<String>(args, 0)?, terminal.as_deref(), options))
}

/// The generator's `group(id, optsOrItems, items?)`: with children as the
/// second argument the third is ignored; with options, `items` wins over
/// `opts.items`, and neither means none.
fn group_fn(args: &[Value]) -> Result<Value, Error> {
    #[derive(Deserialize)]
    struct Opts {
        items: Option<Vec<BarChild>>,
        #[serde(flatten)]
        options: GroupOpts,
    }
    let id: String = arg(args, 0)?;
    if raw(args, 1).is_array() {
        return out(group(&id, arg(args, 1)?, None));
    }
    let Opts { items: opt_items, options } = arg(args, 1)?;
    let items = arg::<Option<Vec<BarChild>>>(args, 2)?.or(opt_items).unwrap_or_default();
    out(group(&id, items, Some(options)))
}

fn normalize_bar_fn(args: &[Value]) -> Result<Value, Error> {
    out(normalize_bar(&arg(args, 0)?)?)
}

fn filter_bar_fn(args: &[Value]) -> Result<Value, Error> {
    let keep = strings(raw(args, 1));
    out(filter_bar(&arg(args, 0)?, |u| keep.iter().any(|k| k == u.key())))
}

/// Argument `i` as layout data: plain JSON, read as JavaScript reads it.
fn data(args: &[Value], i: usize) -> JsValue {
    JsValue::from_canonical(raw(args, i))
}

fn known(commands: &[&str]) -> LayoutKnownIds {
    LayoutKnownIds { commands: commands.iter().map(|c| c.to_string()).collect(), has_command: None }
}

fn create_layout_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(create_layout(data(args, 0)).to_json().to_canonical())
}

fn validate_layout_fn(args: &[Value]) -> Result<Value, Error> {
    let commands = strings(raw(args, 1));
    let ids = LayoutKnownIds { commands, has_command: None };
    out(validate_layout(&create_layout(data(args, 0)), &ids))
}

fn solve_fn(args: &[Value]) -> Result<Value, Error> {
    let bar: NormalizedBar = arg(args, 0)?;
    out(solve(&bar, &TableMetrics(raw(args, 1).clone()), number_arg(args, 2)?))
}

fn solve_schema_fn(args: &[Value]) -> Result<Value, Error> {
    let bar = normalize_bar(&arg::<BarSchema>(args, 0)?)?;
    out(solve(&bar, &TableMetrics(raw(args, 1).clone()), number_arg(args, 2)?))
}

/// A fit as the projection vectors write it: the generator's `fitOf` gives no
/// `length`, which the projection never reads (TypeScript `undefined`, so
/// `NaN` here).
fn fit_arg(args: &[Value], i: usize) -> Result<FitResult, Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fit {
        units: JsMap<UnitAssignment>,
        groups: JsMap<GroupAssignment>,
        has_overflow: bool,
        complete: bool,
    }
    let Fit { units, groups, has_overflow, complete } = arg(args, i)?;
    Ok(FitResult { units, groups, has_overflow, length: f64::NAN, complete })
}

fn project_overflow_fn(args: &[Value]) -> Result<Value, Error> {
    out(project_overflow(&arg(args, 0)?, &fit_arg(args, 1)?, resolver(raw(args, 2))))
}

fn project_shed_fn(args: &[Value]) -> Result<Value, Error> {
    out(project_shed(&arg(args, 0)?, &fit_arg(args, 1)?, resolver(raw(args, 2))))
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

/// `AddItemOptions` as a vector writes it.
#[derive(Deserialize)]
struct AddOptions {
    #[serde(default)]
    to: String,
    section: Option<String>,
    group: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

fn add_fn(args: &[Value]) -> Result<Value, Error> {
    let options: AddOptions = arg(args, 2)?;
    let options = AddItemOptions {
        to: options.to,
        section: options.section,
        group: options.group,
        before: options.before,
        after: options.after,
    };
    let base = create_layout(data(args, 0));
    let edited = base.add(data(args, 1), &options);
    Ok(json!({
        "before": base.to_json().to_canonical(),
        "after": edited.to_json().to_canonical(),
        "warnings": validate_layout(&edited, &known(&["a", "b", "c", "x"])),
    }))
}

fn remove_fn(args: &[Value]) -> Result<Value, Error> {
    #[derive(Deserialize)]
    struct Options {
        #[serde(default)]
        from: String,
    }
    let ids: Vec<String> = arg(args, 1)?;
    let options = RemoveItemOptions { from: arg::<Options>(args, 2)?.from };
    Ok(create_layout(data(args, 0)).remove(&ids, &options).to_json().to_canonical())
}

fn replace_fn(args: &[Value]) -> Result<Value, Error> {
    let id: String = arg(args, 1)?;
    Ok(create_layout(data(args, 0)).replace(&id, data(args, 2)).to_json().to_canonical())
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

fn is_layout_fn(args: &[Value]) -> Result<Value, Error> {
    let raw_data = data(args, 0);
    let created: Layout = create_layout(raw_data.clone());
    Ok(json!({ "raw": is_layout(&raw_data), "created": is_layout(&created) }))
}

fn chain_fn(args: &[Value]) -> Result<Value, Error> {
    let bar = JsValue::from(json!({ "place": "bottom", "end": ["x"] }));
    Ok(create_layout(data(args, 0))
        .add_bar("new", bar)
        .tabs(TabsVisibility::Never)
        .to_json()
        .to_canonical())
}

fn missing_edit_fn(args: &[Value]) -> Result<Value, Error> {
    let edited = create_layout(data(args, 0)).place("missing", "top");
    out(validate_layout(&edited, &known(&["a", "b", "c"])))
}

pub fn functions() -> FunctionTable {
    vec![
        ("constants", constants),
        ("item", item_fn),
        ("custom", custom_fn),
        ("group", group_fn),
        ("normalizeBar", normalize_bar_fn),
        ("filterBar", filter_bar_fn),
        ("createLayout", create_layout_fn),
        ("validateLayout", validate_layout_fn),
        ("solve", solve_fn),
        ("solveSchema", solve_schema_fn),
        ("projectOverflow", project_overflow_fn),
        ("projectShed", project_shed_fn),
        ("projectStrip", project_strip_fn),
        ("solveAndProject", solve_and_project_fn),
        ("add", add_fn),
        ("remove", remove_fn),
        ("replace", replace_fn),
        ("parseShortcut", parse_shortcut_fn),
        ("matchShortcut", match_shortcut_fn),
        ("matchText", match_text_fn),
        ("formatShortcut", format_shortcut_fn),
        ("isLayout", is_layout_fn),
        ("chain", chain_fn),
        ("missingEdit", missing_edit_fn),
    ]
}
