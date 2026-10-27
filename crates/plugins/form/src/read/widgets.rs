//! `read/widgets.ts`: the widgets' reads: each page's widgets (from the
//! form's widget rows), the ones it shows for the form layer to paint, the
//! fill projection with the session's fill permission folded in, and the
//! widget hit test.

use std::rc::Rc;

use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::model::{Box, FieldIndex, WidgetHit, page_widgets_of, widget_at};
use crate::read::fill_items::fill_items;
use crate::value::{get, number_key, str_of};

/// One page's `listWidgets` answer and what it was made from.
pub(crate) struct WidgetItems {
    index: Rc<FieldIndex>,
    rights: String,
    fillable: Value,
    items: Rc<Vec<Value>>,
}

/// `isFillableByHand(fillable, family)`.
fn is_fillable_by_hand(fillable: Option<&Value>, family: &Value) -> bool {
    match fillable {
        Some(Value::Bool(all)) => *all,
        Some(Value::Array(families)) => families.contains(family),
        _ => false,
    }
}

impl crate::controller::Core {
    /// `ensureLoaded(page)`: every page's widgets come with the form, one
    /// read for the document.
    pub fn ensure_loaded(&self, _page: &Value) -> Promise<()> {
        self.fields.settled()
    }

    /// The page's object number, for a ref or an index the document has.
    fn page_number_of(&self, page: &Value) -> Option<f64> {
        let info = self.get_page(page)?;
        get(&info, "ref").and_then(|r| get(r, "objectNumber")).and_then(Value::as_f64)
    }

    /// `getWidgetAt(page, point)`: the widget under a page point.
    pub fn get_widget_at(&self, page: &Value, x: f64, y: f64) -> Option<WidgetHit> {
        let number = self.page_number_of(page)?;
        let index = self.view();
        let widgets = page_widgets_of(&index, number);
        widget_at(&index, Some(&widgets), x, y)
    }

    /// `mayWrite(field)`: the `fillable` setting lets people fill it in by
    /// hand, and the session may fill it in (sign it, for a signature).
    fn may_write(&self, field: &Value) -> bool {
        let settings = self.settings_now();
        is_fillable_by_hand(settings.get("fillable"), get(field, "family").unwrap_or(&Value::Null))
            && if str_of(field, "family") == Some("signature") {
                self.may_sign(field)
            } else {
                self.may_fill(field)
            }
    }

    /// `listWidgets(page)`: each widget on a page, as a fill control draws
    /// it. The same list while the form, the session's rights and the
    /// `fillable` setting stay the same (`memoByKey` in the TypeScript).
    pub fn list_widgets(&self, page: &Value) -> Rc<Vec<Value>> {
        let Some(number) = self.page_number_of(page) else { return Rc::new(Vec::new()) };
        let index = self.view();
        let rights = self.rights_key();
        let fillable = self.settings_now().get("fillable").cloned().unwrap_or(Value::Null);
        let key = number_key(number);
        if let Some(kept) = self.widget_items.borrow().get(&key)
            && Rc::ptr_eq(&kept.index, &index)
            && kept.rights == rights
            && kept.fillable == fillable
        {
            return kept.items.clone();
        }
        let widgets = page_widgets_of(&index, number);
        let items =
            Rc::new(fill_items(&index, number, Some(&widgets), &|field| self.may_write(field)));
        self.widget_items
            .borrow_mut()
            .insert(key, WidgetItems { index, rights, fillable, items: items.clone() });
        items
    }

    /// `getPageBox(page)`: the page's content box, for page-bound placement.
    pub fn get_page_box(&self, page: &Value) -> Option<Box> {
        let info = self.get_page(page)?;
        let size = get(&info, "size")?;
        let n = |k: &str| get(size, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        Some(Box { x: 0.0, y: 0.0, width: n("width"), height: n("height") })
    }

    /// `getPageBox(page)` as JSON.
    pub fn get_page_box_json(&self, page: &Value) -> Value {
        self.get_page_box(page).map_or(Value::Null, |b| json!(b.to_json()))
    }
}
