//! `read/chrome.ts`: selection chrome and hit-testing. The chrome nodes a
//! page paints, the selection anchor, and every "what is under this point"
//! question, projected through the live chrome settings and the page's view
//! scale.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::anchor::anchor_mode_of;
use epdf_annotation_core::hit::{can_move, hit_test, is_on_text_box};
use epdf_annotation_core::js::IdSet;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{
    ChromeGeometry, ChromeNode, Id, Model, PageRef, Point, Target, ViewEnv,
};
use epdf_annotation_core::view::{
    annotation_anchor, chrome as core_chrome, rotation_anchor, selection_anchor,
};
use serde_json::{Value, json};

use crate::contract::ChromeNumbers;
use crate::controller::Core;
use crate::js::{key_or_none, page_object_number};

/// `HitView`: the view a pointer sample arrives in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitView {
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub zoom: Option<f64>,
}

/// Finger-sized grab zones: 2x the mouse tolerances.
pub const TOUCH_GRAB_BOOST: f64 = 2.0;

/// `viewEnv(zoom, rotation)`: the core's view env, or none.
pub fn view_env(zoom: Option<f64>, rotation: Option<f64>) -> Option<ViewEnv> {
    if zoom.is_none() && rotation.is_none() {
        return None;
    }
    Some(ViewEnv::new(zoom.unwrap_or(1.0), rotation.unwrap_or(0.0)))
}

/// The core's page ref for a page ref value.
pub fn core_page(page: &Value) -> PageRef {
    PageRef::of_value(page).unwrap_or_else(|| PageRef::new(page_object_number(page)))
}

type ChromeKey = (Rc<Model>, Rc<serde_json::Value>, Option<f64>, Option<f64>, Option<f64>);

#[derive(Default)]
pub struct ChromeState {
    nodes: RefCell<HashMap<String, (ChromeKey, Value)>>,
    anchor: RefCell<Option<(ChromeKey, Value)>>,
    anchors: RefCell<HashMap<String, (Rc<Model>, Option<(f64, f64)>, Value)>>,
    rotation: RefCell<Option<(Rc<Model>, Value)>>,
}

fn same_key(a: &ChromeKey, b: &ChromeKey) -> bool {
    Rc::ptr_eq(&a.0, &b.0) && Rc::ptr_eq(&a.1, &b.1) && a.2 == b.2 && a.3 == b.3 && a.4 == b.4
}

impl Core {
    fn chrome_numbers(&self) -> ChromeNumbers {
        self.parsed().chrome
    }

    /// `chromeGeomAt(scale, boost)`: the CSS-px chrome settings in content units.
    pub(crate) fn chrome_geom_at(&self, scale: Option<f64>, boost: f64) -> ChromeGeometry {
        let cs = self.chrome_numbers();
        let effective = match scale {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        };
        ChromeGeometry {
            handle_tol: (cs.handle_hit_size / 2.0 / effective) * boost,
            knob_tol: (cs.knob_hit_size / 2.0 / effective) * boost,
            knob_offset: cs.knob_offset / effective,
            rotation_handle: Some(cs.rotation_handle),
            order: Default::default(),
        }
    }

    pub(crate) fn grab_boost(touch: Option<bool>) -> f64 {
        if touch == Some(true) { TOUCH_GRAB_BOOST } else { 1.0 }
    }

    /// The hit margin, a screen-pixel setting, in content units.
    fn hit_margin_at(&self, scale: Option<f64>) -> f64 {
        let s = match scale {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        };
        self.model().hit_margin / s
    }

    /// The page box gestures clamp to.
    pub(crate) fn page_box_of(&self, page: f64) -> Option<epdf_annotation_core::types::Rect> {
        let size = self.size_of(page)?;
        Some(epdf_annotation_core::types::Rect { x: 0.0, y: 0.0, width: size.0, height: size.1 })
    }

    /// `geometry.sizeOf(page)`: the page's size.
    pub(crate) fn size_of(&self, page: f64) -> Option<(f64, f64)> {
        let layout = self.layout_of(page)?;
        let size = layout.get("size")?;
        Some((size.get("width")?.as_f64()?, size.get("height")?.as_f64()?))
    }

    /// `geometry.cropOf(page)`: the crop box in the file's numbers.
    pub(crate) fn crop_of(&self, page: f64) -> Option<Value> {
        self.layout_of(page)?.get("pdfCropBox").cloned().filter(|c| !c.is_null())
    }

    fn layout_of(&self, page: f64) -> Option<Value> {
        let pages = self.pages();
        pages
            .as_array()?
            .iter()
            .find(|p| p.get("ref").map(page_object_number) == Some(page))
            .cloned()
    }

    /// `hitAt(page, point, view, boost, inert)`: the core hit-test with this
    /// plugin's inputs filled in. `inert: Some(None)` tests with no inert set.
    pub(crate) fn hit_at(
        &self,
        page: &Value,
        point: Point,
        view: HitView,
        boost: f64,
        inert: Option<Option<IdSet>>,
    ) -> Target {
        let model = self.model();
        let number = page_object_number(page);
        let inert = match inert {
            Some(given) => given,
            None => self.engaged_ids_on(number),
        };
        hit_test(
            &model,
            &core_page(page),
            point,
            &self.chrome_geom_at(view.scale, boost),
            self.hit_margin_at(view.scale),
            self.page_box_of(number),
            inert.as_ref(),
            view_env(view.zoom, view.rotation).as_ref(),
        )
    }

    /// `textBoxAt(page, point, view)`: the text box whose box is under the point.
    pub(crate) fn text_box_at(&self, page: &Value, point: Point, view: HitView) -> Option<Id> {
        let target = self.hit_at(page, point, view, 1.0, Some(None));
        let id = match target {
            Target::Annot(t) => t.id,
            Target::Handle(t) => t.id,
            _ => return None,
        };
        let model = self.model();
        let record = model.by_id.get(&id)?;
        is_on_text_box(
            record,
            point,
            self.chrome_geom_at(view.scale, 1.0).handle_tol,
            view_env(view.zoom, view.rotation).as_ref(),
        )
        .then_some(id)
    }

    /// `listChromeNodes(page, scale, rotation, zoom)`.
    pub fn list_chrome_nodes(
        &self,
        page: &Value,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Value {
        let number = page_object_number(page);
        let model = self.model();
        let settings = self.settings.get();
        let key: ChromeKey = (model.clone(), settings.clone(), scale, rotation, zoom);
        let page_key = epdf_annotation_core::js::number_to_string(number);
        if let Some((k, v)) = self.chrome.nodes.borrow().get(&page_key)
            && same_key(k, &key)
        {
            return v.clone();
        }
        let cs = self.chrome_numbers();
        let mut nodes: Vec<ChromeNode> = core_chrome(
            &model,
            &core_page(page),
            self.page_box_of(number),
            self.chrome_geom_at(scale, 1.0).knob_offset,
            view_env(zoom, rotation).as_ref(),
        );
        if !cs.guides {
            nodes.retain(|n| !matches!(n, ChromeNode::RotateGuides(_)));
        }
        if !cs.rotation_handle {
            nodes.retain(|n| !matches!(n, ChromeNode::RotateKnob(_)));
        }
        let value = Value::Array(nodes.iter().map(Json::json).collect());
        self.chrome.nodes.borrow_mut().insert(page_key, (key, value.clone()));
        value
    }

    /// `selectionAnchorOf(scale, rotation, zoom)`.
    pub(crate) fn selection_anchor_of(
        &self,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Value {
        let model = self.model();
        let settings = self.settings.get();
        let key: ChromeKey = (model.clone(), settings, scale, rotation, zoom);
        if let Some((k, v)) = &*self.chrome.anchor.borrow()
            && same_key(k, &key)
        {
            return v.clone();
        }
        let knob = self.chrome_geom_at(scale, 1.0).knob_offset;
        let found = selection_anchor(
            &model,
            &|page| self.page_box_of(page.object_number),
            &|_| Some(knob),
            &|_| view_env(zoom, rotation),
        );
        let enabled = self.chrome_numbers().rotation_handle;
        let anchor = match found {
            Some(found) => {
                let mut out = json!({ "page": found.page.json(), "bounds": found.bounds.json() });
                if let Some(k) = found.knob
                    && enabled
                {
                    out["rotationHandle"] = k.json();
                }
                out
            }
            None => Value::Null,
        };
        *self.chrome.anchor.borrow_mut() = Some((key, anchor.clone()));
        anchor
    }

    /// `getAnnotationAnchor(ref, view)`.
    pub fn get_annotation_anchor(&self, reference: &Value, view: Option<ViewEnv>) -> Value {
        let model = self.model();
        let key = key_or_none(reference);
        let view_key = view.as_ref().map(|v| (v.zoom, v.rotation));
        if let Some((m, vk, v)) = self.chrome.anchors.borrow().get(&key)
            && Rc::ptr_eq(m, &model)
            && *vk == view_key
        {
            return v.clone();
        }
        let record = model.by_id.get(&key);
        let found = record.and_then(|r| annotation_anchor(&model, &r.id, view.as_ref()));
        let mut anchor = match found {
            Some((page, bounds)) => json!({ "page": page.json(), "bounds": bounds.json() }),
            None => Value::Null,
        };
        // Without a view, a note that keeps its size on screen says where it is in any view.
        if !anchor.is_null()
            && view.is_none()
            && record.is_some_and(|r| anchor_mode_of(&**r).is_some())
        {
            anchor["boundsIn"] = json!({ "$function": true });
        }
        let previous = self.chrome.anchors.borrow().get(&key).map(|(_, _, v)| v.clone());
        let anchor = match previous {
            Some(previous) if previous == anchor => previous,
            _ => anchor,
        };
        self.chrome.anchors.borrow_mut().insert(key, (model, view_key, anchor.clone()));
        anchor
    }

    /// `annotationAt(page, point)`: the topmost annotation at a page point.
    pub fn annotation_at(&self, page: &Value, point: Point) -> Option<Value> {
        let info = self.get_page(page)?;
        let page_ref = info.get("ref").cloned().unwrap_or(Value::Null);
        let model = self.model();
        match self.hit_at(&page_ref, point, HitView::default(), 1.0, None) {
            Target::Annot(t) => {
                model.by_id.get(&t.id).map(|r| Value::Object((*r.annotation).clone()))
            }
            Target::Empty(_) => None,
            _ => model
                .selected
                .first()
                .and_then(|id| model.by_id.get(id))
                .map(|r| Value::Object((*r.annotation).clone())),
        }
    }

    /// `selection.getRotationAnchor()`: the same object until it changes.
    pub fn rotation_anchor(&self) -> Value {
        let model = self.model();
        if let Some((m, v)) = &*self.chrome.rotation.borrow()
            && Rc::ptr_eq(m, &model)
        {
            return v.clone();
        }
        let value = rotation_anchor(&model).map_or(Value::Null, |a| a.json());
        *self.chrome.rotation.borrow_mut() = Some((model, value.clone()));
        value
    }

    /// `getHitKind(page, point, scale, rotation, zoom, touch)`.
    pub fn get_hit_kind(
        &self,
        page: &Value,
        point: Point,
        view: HitView,
        touch: Option<bool>,
    ) -> &'static str {
        match self.hit_at(page, point, view, Core::grab_boost(touch), None) {
            Target::Handle(_) => "handle",
            Target::Rotate(_) => "rotate",
            Target::GroupHandle(_) => "group-handle",
            Target::Annot(_) => "annot",
            Target::Empty(_) => "empty",
        }
    }

    /// `claimsTouchAt(page, point, scale, rotation, zoom)`.
    pub fn claims_touch_at(&self, page: &Value, point: Point, view: HitView) -> bool {
        let model = self.model();
        match self.hit_at(page, point, view, TOUCH_GRAB_BOOST, None) {
            Target::Handle(_) | Target::Rotate(_) | Target::GroupHandle(_) => true,
            Target::Annot(t) => {
                model.is_selected(&t.id) && model.selected.iter().all(|id| can_move(&model, id))
            }
            Target::Empty(_) => false,
        }
    }

    /// `getCursorAt(page, point, scale, rotation, zoom)`: a cursor, `null` over
    /// nothing, and `undefined` for a handle whose turn has no cursor (the
    /// core's `cursorAt` answers `None` for both; Task 13.7's carry).
    pub fn get_cursor_at(&self, page: &Value, point: Point, view: HitView) -> Value {
        let model = self.model();
        match self.hit_at(page, point, view, 1.0, None) {
            Target::Handle(t) => t.cursor.map_or_else(epdf_kernel::undefined, Value::String),
            Target::GroupHandle(t) => t.cursor.map_or_else(epdf_kernel::undefined, Value::String),
            Target::Rotate(_) => json!("grab"),
            Target::Annot(t) => {
                json!(if model.is_selected(&t.id) && can_move(&model, &t.id) {
                    "move"
                } else {
                    "pointer"
                })
            }
            Target::Empty(_) => Value::Null,
        }
    }
}
