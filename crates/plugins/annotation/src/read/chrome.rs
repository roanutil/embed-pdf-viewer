//! `read/chrome.ts`: selection chrome and hit-testing. The chrome nodes a
//! page paints, the selection anchor, and every "what is under this point"
//! question, projected through the live chrome settings and the page's view
//! scale.

use epdf_annotation_core::js::IdSet;
use epdf_annotation_core::{
    AnnotationRef, ChromeGeom, ChromeNode, PageRef, Point, SelectionAnchor, Target, can_move,
    cursor_at, hit_test,
};

use crate::controller::AnnotationCapability;
use crate::services::geometry::view_env;
use crate::services::store::refs_of_in;

/// `TOUCH_GRAB_BOOST`: finger-sized grab zones, 2x the mouse tolerances.
pub const TOUCH_GRAB_BOOST: f64 = 2.0;

/// `HitView`: the view a pointer sample arrives in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitView {
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub zoom: Option<f64>,
}

/// Which engaged ids a hit test treats as inert.
pub(crate) enum Inert {
    /// The page's engaged ids, resolved now.
    Engaged,
    /// No inert set at all.
    None,
}

impl AnnotationCapability {
    /// `chromeGeomAt(scale, boost)`: the CSS-px chrome settings in content
    /// units at the page's view scale; `boost` widens the grab zones only.
    pub(crate) fn chrome_geom_at(&self, scale: Option<f64>, boost: f64) -> ChromeGeom {
        let cs = self.state().chrome.clone();
        let s = match scale {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        };
        ChromeGeom {
            handle_tol: (cs.handles.hit_size / 2.0 / s) * boost,
            knob_tol: (cs.knob.hit_size / 2.0 / s) * boost,
            knob_offset: cs.knob.offset / s,
        }
    }

    /// `grabBoost(touch)`.
    pub(crate) fn grab_boost(touch: Option<bool>) -> f64 {
        if touch == Some(true) { TOUCH_GRAB_BOOST } else { 1.0 }
    }

    /// `hitAt(page, point, view, boost, inert)`: the core hit test with this
    /// plugin's inputs filled in.
    pub(crate) fn hit_at(
        &self,
        page: PageRef,
        point: Point,
        view: HitView,
        boost: f64,
        inert: Inert,
    ) -> Target {
        let m = self.model();
        let pon = page.page_object_number;
        let inert: Option<IdSet> = match inert {
            Inert::Engaged => self.engaged_ids_on(pon),
            Inert::None => None,
        };
        hit_test(
            &m,
            page,
            point,
            &self.chrome_geom_at(view.scale, boost),
            m.hit_margin,
            self.page_box_of(pon),
            inert.as_ref(),
            view_env(view.zoom, view.rotation),
        )
    }

    /// `listChromeNodes(page, scale, rotation, zoom)` (host). The
    /// `guides.enabled` setting filters the rotate guides out here.
    pub fn list_chrome_nodes(
        &self,
        page: PageRef,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Vec<ChromeNode> {
        let m = self.model();
        let cs = self.state().chrome.clone();
        let pon = page.page_object_number;
        let mut v = epdf_annotation_core::chrome(
            &m,
            page,
            self.page_box_of(pon),
            Some(self.chrome_geom_at(scale, 1.0).knob_offset),
            view_env(zoom, rotation),
        );
        if !cs.guides.enabled {
            v.retain(|n| !matches!(n, ChromeNode::RotateGuides { .. }));
        }
        v
    }

    /// `getSelectionAnchor(view)`: where selection UI attaches.
    pub fn get_selection_anchor(&self, view: HitView) -> Option<SelectionAnchor> {
        let m = self.model();
        let this = self.clone();
        let page_box = move |page: PageRef| this.page_box_of(page.page_object_number);
        let offset = self.chrome_geom_at(view.scale, 1.0).knob_offset;
        let knob = move |_: PageRef| Some(offset);
        let env = view_env(view.zoom, view.rotation);
        let view_of = move |_: PageRef| env;
        epdf_annotation_core::selection_anchor(&m, Some(&page_box), Some(&knob), Some(&view_of))
    }

    /// `hitTestAt(page, point)`: the topmost annotation under a point; a
    /// handle or the knob belongs to the selection.
    pub fn hit_test_at(&self, page: PageRef, point: Point) -> Option<AnnotationRef> {
        let m = self.model();
        match self.hit_at(page, point, HitView::default(), 1.0, Inert::Engaged) {
            Target::Annot { id } => m.by_id.get(&id).and_then(|a| a.ref_.clone()),
            Target::Empty => None,
            _ => refs_of_in(&m, &m.selected).into_iter().next(),
        }
    }

    /// `getHitKind(...)` (host): what is under a content point.
    pub fn get_hit_kind(
        &self,
        page: PageRef,
        point: Point,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
        touch: Option<bool>,
    ) -> &'static str {
        let view = HitView { scale, rotation, zoom };
        match self.hit_at(page, point, view, Self::grab_boost(touch), Inert::Engaged) {
            Target::Handle { .. } => "handle",
            Target::Rotate { .. } => "rotate",
            Target::GroupHandle { .. } => "group-handle",
            Target::Annot { .. } => "annot",
            Target::Empty => "empty",
        }
    }

    /// `claimsTouchAt(...)` (host): touch consent. Chrome always claims; a
    /// body claims only when a drag would arm a move.
    pub fn claims_touch_at(
        &self,
        page: PageRef,
        point: Point,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> bool {
        let m = self.model();
        let view = HitView { scale, rotation, zoom };
        match self.hit_at(page, point, view, TOUCH_GRAB_BOOST, Inert::Engaged) {
            Target::Handle { .. } | Target::Rotate { .. } | Target::GroupHandle { .. } => true,
            Target::Annot { id } => {
                m.selected.contains(&id) && m.selected.iter().all(|s| can_move(&m, s))
            }
            Target::Empty => false,
        }
    }

    /// `getCursorAt(...)` (host).
    pub fn get_cursor_at(
        &self,
        page: PageRef,
        point: Point,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Option<String> {
        let m = self.model();
        let pon = page.page_object_number;
        let inert = self.engaged_ids_on(pon);
        cursor_at(
            &m,
            page,
            point,
            &self.chrome_geom_at(scale, 1.0),
            m.hit_margin,
            self.page_box_of(pon),
            inert.as_ref(),
            view_env(zoom, rotation),
        )
    }
}
