//! `write/fields.ts`: building the form. Every design verb is one change on
//! the document's queue (`ctx.changes`): it shows at once (the fields
//! mirror predicts it), it is a step of the history, and it resolves with
//! the engine's answer. A create takes its field's and widgets' object
//! numbers first, so their refs are final from the first frame and the new
//! widget can be selected straight away.

use epdf_kernel::Error;
use epdf_kernel::changes::{PendingChange, StagedChange};
use epdf_kernel::error::{code, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::controller::CAPABILITY;
use crate::engine::{draft_writes_scripts, writes_scripts};
use crate::model::{Box, field_by_ref, widget_row_of};
use crate::sync::predict::undo_look_of;
use crate::value::{get, items, str_of};
use crate::write::next_in_line::next_in_line;

/// `LOOK`: a widget's look, as `addWidget()` copies it from the field's
/// last widget onto the new one.
const LOOK: [&str; 12] = [
    "color",
    "interiorColor",
    "strokeWidth",
    "borderStyle",
    "dashArray",
    "fontFamily",
    "fontSize",
    "fontColor",
    "textAlign",
    "caption",
    "checkStyle",
    "rotation",
];

fn look_of(widget: Option<&Value>) -> Map<String, Value> {
    let mut look = Map::new();
    let Some(widget) = widget else { return look };
    for name in LOOK {
        if let Some(value) = get(widget, name).filter(|v| !v.is_null()) {
            look.insert(name.to_string(), value.clone());
        }
    }
    look
}

/// `exportValueFor(field)`: a radio group's next free `Choice<n>`, a
/// checkbox's own value; none for other families.
fn export_value_for(field: &Value) -> Option<Value> {
    match str_of(field, "family") {
        // `return field.exportValue`: absent stays absent (`undefined`), and
        // the placement then carries no `exportValue`.
        Some("checkbox") => {
            get(field, "exportValue").filter(|v| !epdf_kernel::is_undefined(v)).cloned()
        }
        Some("radio") => {
            let widgets = items(field, "widgets");
            let taken: Vec<String> =
                widgets.iter().map(|w| crate::value::string_of(get(w, "exportValue"))).collect();
            let mut n = widgets.len() + 1;
            while taken.contains(&format!("Choice{n}")) {
                n += 1;
            }
            Some(json!(format!("Choice{n}")))
        }
        _ => None,
    }
}

/// `isMerged(field)`: the field's dictionary is its only widget's too.
fn is_merged(field: &Value) -> bool {
    let reference = get(field, "ref").unwrap_or(&Value::Null);
    str_of(reference, "kind") == Some("objectNumber")
        && items(field, "widgets").iter().any(|w| {
            crate::value::strict_eq(get(w, "objectNumber"), get(reference, "objectNumber"))
        })
}

const MODIFY: &str = "doc.forms.modify";
const SCRIPT: &str = "doc.forms.script";

impl crate::controller::Core {
    /// `allow(operation, scripts)`.
    fn allow(&self, operation: &str, scripts: bool) -> Result<(), Error> {
        self.assert_allowed(MODIFY, operation)?;
        if scripts {
            self.assert_allowed(SCRIPT, operation)?;
        }
        Ok(())
    }

    /// `stage(verb, op)`: one design op as its own change.
    fn stage_design(&self, verb: &str, op: Value) -> Result<PendingChange, Error> {
        let undo = undo_look_of(&self.view(), std::slice::from_ref(&op));
        let mut change = StagedChange::ops(json!({ "key": format!("form.{verb}") }), vec![op]);
        change.undo = undo;
        self.cx.changes()?.stage(change)
    }

    /// `answer(change, options)`: the engine's answer to a staged design
    /// change, its one item.
    fn answer(&self, change: PendingChange, signal: Option<AbortSignal>) -> Promise<Value> {
        let answer = self.cancellable(signal, change.result());
        Promise::spawn(async move {
            match answer.await {
                Ok(result) => Ok(items(&result, "items").first().cloned().unwrap_or(Value::Null)),
                Err(error) => Err(to_plugin_error(CAPABILITY, &error)),
            }
        })
    }

    /// `create(draft, options)`: a field with a widget for each place it
    /// shows. Resolves `{ field }`.
    pub fn create(&self, draft: Value, signal: Option<AbortSignal>) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.allow("form.create", draft_writes_scripts(&draft))?;
            // The field, then each widget, in the draft's order.
            let count = 1 + items(&draft, "widgets").len();
            let numbers = match me.take_numbers_now(count)? {
                Some(numbers) => numbers,
                None => me.take_numbers(count).await?,
            };
            let op = json!({
                "type": "forms.create",
                "draft": draft,
                "objectNumber": numbers[0],
                "widgetObjectNumbers": &numbers[1..],
            });
            let change = me.stage_design("create", op)?;
            let item = me.answer(change, signal).await?;
            Ok(json!({ "field": get(&item, "field").cloned().unwrap_or(Value::Null) }))
        })
    }

    /// `addWidget(ref, placement, options)`: the field in one more place.
    /// Resolves `{ field, widget }`.
    pub fn add_widget(
        &self,
        reference: Value,
        placement: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.allow("form.addWidget", writes_scripts(get(&placement, "actions")))?;
            let index = me.view();
            let Some(field) = field_by_ref(&index, &reference).cloned() else {
                return Err(me.error(code::NOT_FOUND, "no form field has this ref"));
            };
            let rows: Vec<Value> = items(&field, "widgets")
                .iter()
                .filter_map(|w| {
                    let n = get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
                    widget_row_of(&index, n).cloned()
                })
                .collect();
            let last = rows.last();
            let page = get(&placement, "page")
                .filter(|p| !p.is_null())
                .or_else(|| last.and_then(|l| get(l, "page")))
                .cloned();
            let Some(page) = page.filter(|p| crate::value::truthy(Some(p))) else {
                return Err(me.error(
                    code::INVALID_INPUT,
                    "the field shows on no page: give `page` and `rect`",
                ));
            };
            let size = me.get_page(&page).and_then(|info| get(&info, "size").cloned());
            let on_page: Vec<&Value> = rows
                .iter()
                .filter(|row| {
                    crate::value::strict_eq(
                        get(row, "page").and_then(|p| get(p, "objectNumber")),
                        get(&page, "objectNumber"),
                    )
                })
                .collect();
            let given = get(&placement, "rect")
                .filter(|r| !r.is_null())
                .or_else(|| get(&placement, "box").filter(|b| !b.is_null()))
                .cloned();
            let rect = match given {
                Some(rect) => Some(rect),
                None => match &size {
                    Some(size) if !on_page.is_empty() => {
                        let boxes: Vec<Box> = on_page
                            .iter()
                            .map(|row| Box::read(get(row, "box").unwrap_or(&Value::Null)))
                            .collect();
                        let n = |k: &str| get(size, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                        Some(next_in_line(&boxes, n("width"), n("height")).to_json())
                    }
                    _ => None,
                },
            };
            let Some(rect) = rect else {
                return Err(me.error(
                    code::INVALID_INPUT,
                    "no widget of the field on that page: give `rect`",
                ));
            };
            let export_value = get(&placement, "exportValue")
                .filter(|v| !v.is_null())
                .cloned()
                .or_else(|| export_value_for(&field));
            let split = is_merged(&field);
            let count = if split { 2 } else { 1 };
            let numbers = match me.take_numbers_now(count)? {
                Some(numbers) => numbers,
                None => me.take_numbers(count).await?,
            };
            let object_number = numbers[0];
            // `{ ...lookOf(last), ...placement, page, rect?, exportValue? }`.
            let mut full = look_of(last);
            for (key, value) in placement.as_object().into_iter().flatten() {
                full.insert(key.clone(), value.clone());
            }
            full.insert("page".into(), page);
            if !crate::value::truthy(get(&placement, "box")) {
                full.insert("rect".into(), rect);
            }
            if let Some(export_value) = export_value {
                full.insert("exportValue".into(), export_value);
            }
            let mut op = Map::new();
            op.insert("type".into(), json!("forms.addWidget"));
            op.insert("field".into(), get(&field, "ref").cloned().unwrap_or(Value::Null));
            op.insert("placement".into(), Value::Object(full));
            op.insert("objectNumber".into(), json!(object_number));
            if split {
                op.insert("splitObjectNumber".into(), json!(numbers[1]));
            }
            let change = me.stage_design("addWidget", Value::Object(op))?;
            let item = me.answer(change, signal).await?;
            let widgets = items(&item, "widgets");
            let widget = widgets
                .iter()
                .find(|row| {
                    let r = get(row, "ref").unwrap_or(&Value::Null);
                    str_of(r, "kind") == Some("objectNumber")
                        && get(r, "objectNumber").and_then(Value::as_f64)
                            == Some(f64::from(object_number))
                })
                .or_else(|| widgets.last())
                .cloned()
                .unwrap_or(Value::Null);
            Ok(
                json!({ "field": get(&item, "field").cloned().unwrap_or(Value::Null), "widget": widget }),
            )
        })
    }

    /// One design verb: allowed, staged, answered, its item read.
    fn design(
        &self,
        operation: &'static str,
        verb: &'static str,
        scripts: bool,
        op: Value,
        signal: Option<AbortSignal>,
        read: fn(&Value) -> Value,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.allow(operation, scripts)?;
            let change = me.stage_design(verb, op)?;
            let item = me.answer(change, signal).await?;
            Ok(read(&item))
        })
    }

    /// `update(ref, patch, options)`: resolves `{ field }`.
    pub fn update(
        &self,
        reference: Value,
        patch: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let scripts = writes_scripts(get(&patch, "actions"));
        let op = json!({ "type": "forms.update", "field": reference, "patch": patch });
        self.design(
            "form.update",
            "update",
            scripts,
            op,
            signal,
            |item| json!({ "field": get(item, "field").cloned().unwrap_or(Value::Null) }),
        )
    }

    /// `reorderCalculations(fields, position, options)`.
    pub fn reorder_calculations(
        &self,
        fields: Value,
        position: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let op =
            json!({ "type": "forms.reorderCalculations", "fields": fields, "position": position });
        self.design("form.reorderCalculations", "reorderCalculations", false, op, signal, |item| {
            json!({ "calculationOrder": get(item, "calculationOrder").cloned().unwrap_or(Value::Null) })
        })
    }

    /// `delete(ref, options)`.
    pub fn delete(&self, reference: Value, signal: Option<AbortSignal>) -> Promise<Value> {
        let op = json!({ "type": "forms.delete", "field": reference });
        self.design("form.delete", "delete", false, op, signal, |_| epdf_kernel::undefined())
    }

    /// `removeWidget(ref, widget, options)`: resolves `{ field }`.
    pub fn remove_widget(
        &self,
        reference: Value,
        widget: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let op = json!({ "type": "forms.removeWidget", "field": reference, "widget": widget });
        self.design(
            "form.removeWidget",
            "removeWidget",
            false,
            op,
            signal,
            |item| json!({ "field": get(item, "field").cloned().unwrap_or(Value::Null) }),
        )
    }

    /// `updateWidget(widget, patch, options)`: resolves `{ widget }`.
    pub fn update_widget(
        &self,
        widget: Value,
        patch: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let op = json!({ "type": "forms.updateWidget", "widget": widget, "patch": patch });
        self.design(
            "form.updateWidget",
            "updateWidget",
            false,
            op,
            signal,
            |item| json!({ "widget": get(item, "widget").cloned().unwrap_or(Value::Null) }),
        )
    }

    /// `deleteWidget(widget, options)`: resolves `{ field }`.
    pub fn delete_widget(&self, widget: Value, signal: Option<AbortSignal>) -> Promise<Value> {
        let op = json!({ "type": "forms.deleteWidget", "widget": widget });
        self.design(
            "form.deleteWidget",
            "deleteWidget",
            false,
            op,
            signal,
            |item| json!({ "field": get(item, "field").cloned().unwrap_or(Value::Null) }),
        )
    }

    /// `repair(options)`: a repair reads the whole form and fixes it where it
    /// stands, not a change of ops. `options` is the repair's own options
    /// (`None` when the caller gave none).
    pub fn repair(&self, options: Option<Value>, signal: Option<AbortSignal>) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.allow("form.repair", false)?;
            let run = {
                let me = me.clone();
                let signal = signal.clone();
                move || -> Result<Promise<Value>, Error> {
                    let doc = me.doc_or_throw()?;
                    let args = match options {
                        Some(options) => vec![options],
                        None => Vec::new(),
                    };
                    Ok(me.cancellable(signal, doc.call("forms.repair", None, args)))
                }
            };
            let queued = me.enqueue.run(run, signal);
            crate::promise::adopt(queued).await
        })
    }
}
