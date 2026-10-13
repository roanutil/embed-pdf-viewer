//! `read/widgets.ts`: the widget plane's reads: per-page widget geometry
//! (one annotations read per page, lazily), the fused fill projection
//! (memoized per model change) and the hit test.

use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_kernel::geometry::{PageRef, PdfEdges};
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

use crate::controller::FormCapability;
use crate::core::fill_items::{
    FillItem, fill_item_for_widget as core_fill_item_for_widget, fill_items as core_fill_items,
};
use crate::core::model::{
    Box, Msg, WidgetHit, contains, field_for_widget as core_field_for_widget, number_key,
    widget_at as core_widget_at,
};
use crate::value::{get, num, str_of};

/// Session fill authority fuses into the same `disabled` the ReadOnly flag
/// feeds: without `doc.forms.fill` every widget renders inert.
fn fuse_fill(item: Option<FillItem>, fillable: bool) -> Option<FillItem> {
    match item {
        Some(item) if !fillable && !item.disabled => Some(FillItem { disabled: true, ..item }),
        other => other,
    }
}

impl FormCapability {
    /// Host lens: `ensureLoaded(page)`, the TypeScript's `ensureGeom`: warm a
    /// page's widget geometry, once, lazily.
    pub fn ensure_loaded(&self, page: PageRef) {
        let Some(doc) = self.doc() else { return };
        let pon = f64::from(page.page_object_number);
        if self.0.geom_loading.borrow().contains(&pon)
            || self.model().geom.contains_key(&number_key(pon))
        {
            return;
        }
        // The kernel's page geometry: the one PDF to page conversion.
        let Some(space) = self.cx().try_for_page(page) else { return };
        if doc.page_index(page.page_object_number).is_err() {
            return;
        }
        self.0.geom_loading.borrow_mut().push(pon);
        let call = doc.call("page.annotations.list", Some(page.page_object_number), Vec::new());
        let this = self.clone();
        let loaded: Promise<()> = Promise::spawn(async move {
            let list = call.await?;
            let mut boxes = indexmap::IndexMap::new();
            for dto in get(&list, "annotations").and_then(Value::as_array).into_iter().flatten() {
                if str_of(dto, "subtype") != Some("widget") {
                    continue;
                }
                let r = get(dto, "ref").unwrap_or(&Value::Null);
                let object_number = if str_of(r, "kind") == Some("objectNumber") {
                    num(r, "annotObjectNumber").unwrap_or(f64::NAN)
                } else {
                    0.0
                };
                if object_number > 0.0 {
                    let rect: PdfEdges = get(dto, "rect")
                        .and_then(|r| serde_json::from_value(r.clone()).ok())
                        .unwrap_or(PdfEdges {
                            left: f64::NAN,
                            bottom: f64::NAN,
                            right: f64::NAN,
                            top: f64::NAN,
                        });
                    let b = space.pdf_rect_to_page(rect);
                    boxes.insert(
                        number_key(object_number),
                        Box { x: b.x, y: b.y, width: b.width, height: b.height },
                    );
                }
            }
            this.apply(Msg::PageGeom { page_object_number: pon, boxes });
            Ok(())
        });
        let this = self.clone();
        rt::spawn(async move {
            let _ = loaded.await;
            this.0.geom_loading.borrow_mut().retain(|&p| p != pon);
        });
    }

    /// `getWidgetAt(page, point)`: the model's geometry when the page has it
    /// (kicking the lazy load so the next call does); otherwise the
    /// annotation plane's live boxes.
    pub fn get_widget_at(&self, page: PageRef, x: f64, y: f64) -> Option<WidgetHit> {
        let m = self.model();
        let pon = f64::from(page.page_object_number);
        self.ensure_loaded(page);
        if m.geom.contains_key(&number_key(pon)) {
            return core_widget_at(&m, pon, x, y);
        }
        let annotation = self.0.annotation.as_ref()?;
        let mut best: Option<WidgetHit> = None;
        let items = annotation
            .list_page_items(epdf_annotation_core::PageRef::new(page.page_object_number), None);
        for item in items {
            if !item.subtype.starts_with("widget") {
                continue;
            }
            let Some(epdf_annotation_core::AnnotationRef::ObjectNumber {
                annot_object_number, ..
            }) = item.ref_
            else {
                continue;
            };
            let bx = Box {
                x: item.box_.x,
                y: item.box_.y,
                width: item.box_.width,
                height: item.box_.height,
            };
            if !contains(&bx, x, y) {
                continue;
            }
            let Some(field) = core_field_for_widget(&m, annot_object_number) else { continue };
            if best.as_ref().is_none_or(|b| bx.width * bx.height < b.bx.width * b.bx.height) {
                best = Some(WidgetHit { annot_object_number, field, bx });
            }
        }
        best
    }

    /// Host lens: `listFillItems(page)`, which is also the public
    /// `listWidgets(page)`: one page's fill controls, the same `Rc` while
    /// the model and the fill grant are unchanged.
    pub fn list_fill_items(&self, page: PageRef) -> Rc<Vec<FillItem>> {
        let m = self.model();
        let pon = f64::from(page.page_object_number);
        let fillable = self.can("doc.forms.fill");
        let key = number_key(pon);
        if let Some((seq, cached_fillable, items)) = self.0.fill_cache.borrow().get(&key)
            && *seq == m.seq
            && *cached_fillable == fillable
        {
            return items.clone();
        }
        let items: Rc<Vec<FillItem>> = Rc::new(
            core_fill_items(&m, pon)
                .into_iter()
                .filter_map(|item| fuse_fill(Some(item), fillable))
                .collect(),
        );
        self.0.fill_cache.borrow_mut().insert(key, (m.seq, fillable, items.clone()));
        items
    }

    /// `listWidgets(page)`: the same projection as [`Self::list_fill_items`].
    pub fn list_widgets(&self, page: PageRef) -> Rc<Vec<FillItem>> {
        self.list_fill_items(page)
    }

    /// Host lens: `getFillItem(annotObjectNumber)`, the same `Rc` per model
    /// change.
    pub fn get_fill_item(&self, annot_object_number: f64) -> Option<Rc<FillItem>> {
        let m = self.model();
        let fillable = self.can("doc.forms.fill");
        let key = number_key(annot_object_number);
        if let Some((seq, cached_fillable, item)) = self.0.fill_item_cache.borrow().get(&key)
            && *seq == m.seq
            && *cached_fillable == fillable
        {
            return item.clone();
        }
        let item =
            fuse_fill(core_fill_item_for_widget(&m, annot_object_number), fillable).map(Rc::new);
        self.0.fill_item_cache.borrow_mut().insert(key, (m.seq, fillable, item.clone()));
        item
    }

    /// Host lens: `getPageBox(page)`, the page's content box `{0, 0, w, h}`.
    pub fn get_page_box(&self, page: PageRef) -> Option<Rect> {
        let space = self.cx().try_for_page(page)?;
        Some(Rect { x: 0.0, y: 0.0, width: space.width, height: space.height })
    }
}
