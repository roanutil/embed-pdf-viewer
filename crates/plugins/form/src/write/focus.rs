//! Which field and widget the cursor moves to, in the documents keyboard order.
use crate::{
    controller::Core,
    model::{
        FieldIndex, field_by_ref, field_key_of, shared_field_by_key, shared_field_by_ref,
        shared_field_for_widget, shown_on_screen, widget_row_of,
    },
    read::fields::{is_empty, widget_object_of},
    value::str_of,
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::rc::Rc;
struct Stop {
    row: Rc<Value>,
    field: Rc<Value>,
    at: usize,
}
struct FocusOrder {
    rows: Vec<Rc<Value>>,
    stops: Vec<Stop>,
}
pub(crate) struct FocusOrderCache {
    index: Rc<FieldIndex>,
    pages: Rc<Value>,
    rights: String,
    fillable: Value,
    order: Rc<FocusOrder>,
}
impl Core {
    fn focus_order(&self) -> Rc<FocusOrder> {
        let index = self.view();
        let pages = self.pages();
        let rights = self.rights_key();
        let fillable = self.settings_now().get("fillable").cloned().unwrap_or(Value::Null);
        if let Some(cache) = &*self.focus_order_cache.borrow()
            && Rc::ptr_eq(&cache.index, &index)
            && Rc::ptr_eq(&cache.pages, &pages)
            && cache.rights == rights
            && cache.fillable == fillable
        {
            return cache.order.clone();
        }
        let rows: Vec<_> =
            index.widgets().iter().filter(|row| shown_on_screen(row)).cloned().collect();
        let rows =
            crate::engine::tab_order_of(&rows, pages.as_array().map(Vec::as_slice).unwrap_or(&[]));
        let mut groups = HashSet::new();
        let mut stops = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            let Some(field) = shared_field_for_widget(&index, widget_object_of(&row["ref"])) else {
                continue;
            };
            if !self.takes_input(field) {
                continue;
            }
            if str_of(field, "family") == Some("radio")
                && !groups.insert(format!(
                    "{}@{}",
                    field_key_of(field),
                    row["page"]["objectNumber"]
                ))
            {
                continue;
            }
            stops.push(Stop { row: row.clone(), field: field.clone(), at });
        }
        let order = Rc::new(FocusOrder { rows, stops });
        *self.focus_order_cache.borrow_mut() =
            Some(FocusOrderCache { index, pages, rights, fillable, order: order.clone() });
        order
    }
    fn radio_buttons(
        &self,
        rows: &[Rc<Value>],
        field: &Rc<Value>,
        page: Option<&Value>,
    ) -> Vec<Rc<Value>> {
        let index = self.view();
        rows.iter()
            .filter(|row| {
                page.is_none_or(|p| p == &row["page"]["objectNumber"])
                    && shared_field_for_widget(&index, widget_object_of(&row["ref"]))
                        .is_some_and(|f| Rc::ptr_eq(f, field))
            })
            .cloned()
            .collect()
    }
    fn box_at_stop(&self, rows: &[Rc<Value>], stop: &Stop, backward: bool) -> Rc<Value> {
        if str_of(&stop.field, "family") != Some("radio") {
            return stop.row.clone();
        }
        let buttons =
            self.radio_buttons(rows, &stop.field, Some(&stop.row["page"]["objectNumber"]));
        let checked = buttons.iter().find(|r| {
            radio_button(&stop.field, widget_object_of(&r["ref"]))
                .is_some_and(|b| b["checked"] == true)
        });
        checked
            .or_else(|| if backward { buttons.last() } else { buttons.first() })
            .unwrap_or(&stop.row)
            .clone()
    }
    /// Bring a fillable field into view and give its control the cursor.
    pub fn focus(&self, reference: &Value, options: Option<&Value>) -> bool {
        let index = self.view();
        let Some(field) = shared_field_by_ref(&index, reference).filter(|f| self.takes_input(f))
        else {
            return false;
        };
        let order = self.focus_order();
        let rows = &order.rows;
        let stops = &order.stops;
        let row =
            if let Some(widget) = options.and_then(|o| o.get("widget")).filter(|w| !w.is_null()) {
                let n = widget_object_of(widget);
                if !shared_field_for_widget(&index, n).is_some_and(|f| Rc::ptr_eq(f, field)) {
                    return false;
                }
                rows.iter().find(|r| widget_object_of(&r["ref"]) == n).cloned()
            } else {
                stops
                    .iter()
                    .find(|s| Rc::ptr_eq(&s.field, field))
                    .map(|stop| self.box_at_stop(rows, stop, false))
            };
        row.is_some_and(|row| {
            self.focus_row(&row, options.and_then(|o| o["select"].as_bool()).unwrap_or(false))
        })
    }
    /// Move as Tab does, returning false when the cursor leaves the form.
    pub fn focus_next(&self, options: Option<&Value>) -> bool {
        let options = options.cloned().unwrap_or(json!({}));
        let index = self.view();
        let order = self.focus_order();
        let rows = &order.rows;
        let stops = &order.stops;
        let backward = options["backward"].as_bool().unwrap_or(false);
        let position =
            |widget: f64| rows.iter().position(|row| widget_object_of(&row["ref"]) == widget);
        let from = if let Some(reference) = options.get("from") {
            field_by_ref(&index, reference).and_then(|f| f["widgets"].as_array()).and_then(
                |widgets| {
                    widgets
                        .iter()
                        .filter_map(|w| w["objectNumber"].as_f64())
                        .find(|n| position(*n).is_some())
                },
            )
        } else {
            self.focused_widget().and_then(|f| f["widget"].as_f64()).or(self.last_focused_widget())
        };
        let at = from.and_then(position);
        let field = from.and_then(|n| shared_field_for_widget(&index, n));
        let page =
            from.and_then(|n| widget_row_of(&index, n)).map(|r| r["page"]["objectNumber"].clone());
        let candidates: Vec<_> = stops
            .iter()
            .filter(|s| {
                (!options["empty"].as_bool().unwrap_or(false) || is_empty(&s.field))
                    && !(field.is_some_and(|f| {
                        str_of(f, "family") == Some("radio") && Rc::ptr_eq(f, &s.field)
                    }) && page.as_ref() == Some(&s.row["page"]["objectNumber"]))
            })
            .collect();
        let ahead: Vec<_> = candidates
            .iter()
            .filter(|s| at.is_none_or(|at| if backward { s.at < at } else { s.at > at }))
            .copied()
            .collect();
        let next = if backward { ahead.last() } else { ahead.first() }.copied().or_else(|| {
            if options["wrap"].as_bool().unwrap_or(false) {
                if backward { candidates.last() } else { candidates.first() }.copied()
            } else {
                None
            }
        });
        next.is_some_and(|stop| self.focus_row(&self.box_at_stop(rows, stop, backward), true))
    }
    pub fn get_focused_field(&self) -> Option<Rc<Value>> {
        let focused = self.focused_widget()?;
        let index = self.view();
        shared_field_by_key(&index, focused["field"].as_str()?).cloned()
    }
    /// Radio arrows change the value before running the selected buttons activation.
    pub fn move_in_group(&self, reference: &Value, backward: bool) -> bool {
        let index = self.view();
        let widget = widget_object_of(reference);
        let Some(field) = shared_field_for_widget(&index, widget)
            .filter(|f| str_of(f, "family") == Some("radio") && self.takes_input(f))
        else {
            return false;
        };
        let order = self.focus_order();
        let buttons = self.radio_buttons(&order.rows, field, None);
        let Some(here) = buttons.iter().position(|r| widget_object_of(&r["ref"]) == widget) else {
            return false;
        };
        if buttons.len() < 2 {
            return false;
        }
        let next = &buttons[(here + if backward { buttons.len() - 1 } else { 1 }) % buttons.len()];
        if !self.focus_row(next, false) {
            return false;
        }
        if let Some(button) = radio_button(field, widget_object_of(&next["ref"])) {
            let write =
                self.set_value(field["ref"].clone(), json!({"value":button["exportValue"]}), None);
            let weak = self.weak();
            let reference = next["ref"].clone();
            write.on_settle(move |_| {
                if let Some(core) = weak.upgrade() {
                    core.activate_widget(reference.clone(), None).ignore();
                }
            });
        }
        true
    }
}
fn radio_button(field: &Value, widget: f64) -> Option<&Value> {
    field["widgets"].as_array()?.iter().find(|b| b["objectNumber"].as_f64() == Some(widget))
}
