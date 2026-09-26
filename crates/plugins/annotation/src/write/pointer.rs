//! `write/pointer.ts`: pointer intents. The gesture samples the interaction
//! handlers forward (edit, marquee, create, hover), each folded into the
//! pure core with this plugin's inputs filled in (page box, chrome geometry,
//! engaged ids, view).

use epdf_annotation_core::measure::{DimensionInput, measurement_readout};
use epdf_annotation_core::measurement::{
    DistanceAppearance, DistanceIntent, LineDimensionCaption, MeasurementAppearance,
};
use epdf_annotation_core::measurement_shape::{
    ShapeDimensionCaption, ShapeIntent, ShapeMeasurementAppearance,
};
use epdf_annotation_core::{
    Draft, Intent, Msg, PageRef, PdfPoint, Phase, Point, PointerInput, Target, content_to_pdf_point,
};
use serde_json::json;

use crate::controller::AnnotationCapability;
use crate::host_contract::HoverAt;
use crate::read::chrome::{HitView, Inert};

impl AnnotationCapability {
    /// `editPointer(...)` (host).
    #[allow(clippy::too_many_arguments)]
    pub fn edit_pointer(
        &self,
        phase: Phase,
        page: PageRef,
        point: Point,
        shift: bool,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
        touch: Option<bool>,
    ) {
        let pon = page.page_object_number;
        let input = PointerInput {
            page,
            point,
            shift,
            finish: None,
            page_box: self.page_box_of(pon),
            chrome: Some(self.chrome_geom_at(scale, Self::grab_boost(touch))),
            display_rotation: rotation,
            zoom,
            upright: None,
            inert: self.engaged_ids_on(pon),
        };
        self.commit(Msg::EditPointer { phase, input });
    }

    /// `marqueePointer(...)` (host).
    #[allow(clippy::too_many_arguments)]
    pub fn marquee_pointer(
        &self,
        phase: Phase,
        page: PageRef,
        point: Point,
        shift: bool,
        _scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) {
        let pon = page.page_object_number;
        let input = PointerInput {
            page,
            point,
            shift,
            finish: None,
            page_box: self.page_box_of(pon),
            chrome: None,
            display_rotation: rotation,
            zoom,
            upright: None,
            inert: self.engaged_ids_on(pon),
        };
        self.commit(Msg::MarqueePointer { phase, input });
    }

    /// `createPointer(tool, phase, page, point, finish, displayRotation)`
    /// (host): inert without create authority (a capture tool needs
    /// `doc.annotate.modify`). A dimension tool resolves its scale at the
    /// first point from the page's viewports.
    pub fn create_pointer(
        &self,
        tool: &str,
        phase: Phase,
        page: PageRef,
        point: Point,
        finish: bool,
        display_rotation: Option<f64>,
    ) {
        let pon = page.page_object_number;
        let t = self.tool(tool);
        let capture = t.as_ref().is_some_and(|t| t.captures());
        let allowed = if capture {
            self.doc().is_some_and(|d| d.security().allows("doc.annotate.modify"))
        } else {
            self.can_create()
        };
        if !allowed {
            return;
        }
        let crop = self.crop_of(pon);
        let cache = self.0.viewports.of(pon);
        let preset = t.as_ref().map_or_else(|| tool.to_string(), |t| t.preset.clone());
        let continuing = match &self.model().draft {
            Some(Draft::CreateDistance { page: p, preset: dp, .. }) => {
                p.same_page(&page) && *dp == preset
            }
            Some(Draft::CreatePoly { page: p, preset: dp, .. }) => {
                p.same_page(&page) && dp.as_deref() == Some(preset.as_str())
            }
            _ => false,
        };
        let dimension = t.as_ref().is_some_and(|t| t.is_dimension());
        if dimension
            && phase == Phase::Down
            && !continuing
            && (crop.is_none() || cache.as_ref().is_none_or(|c| c.viewports.is_none()))
        {
            return;
        }
        let viewport = match (crop, cache.as_ref().and_then(|c| c.viewports.as_ref())) {
            (Some(crop), Some(viewports)) => crate::write::measurement::viewport_for_point(
                viewports,
                content_to_pdf_point(point, crop),
            )
            .cloned(),
            _ => None,
        };
        let measure: Option<MeasurementAppearance> = match (&t, crop, &cache) {
            (Some(t), Some(crop), Some(cache)) if dimension => {
                let scale = match &viewport {
                    Some(v) => v.measure.clone(),
                    None => cache.fallback.clone(),
                };
                let caption = t
                    .measurement
                    .as_ref()
                    .map_or_else(|| json!({ "enabled": true }), |m| m.caption.clone());
                Some(if t.intent.as_deref() == Some("LineDimension") {
                    MeasurementAppearance::Distance(DistanceAppearance {
                        intent: DistanceIntent::LineDimension,
                        measure: scale,
                        caption: serde_json::from_value::<LineDimensionCaption>(caption).unwrap_or(
                            LineDimensionCaption { enabled: true, position: None, offset: None },
                        ),
                        leader: t.measurement.as_ref().and_then(|m| m.leader),
                        crop,
                        text: String::new(),
                    })
                } else {
                    MeasurementAppearance::Shape(ShapeMeasurementAppearance {
                        intent: if t.intent.as_deref() == Some("PolygonDimension") {
                            ShapeIntent::PolygonDimension
                        } else {
                            ShapeIntent::PolyLineDimension
                        },
                        measure: scale,
                        caption: serde_json::from_value::<ShapeDimensionCaption>(caption)
                            .unwrap_or(ShapeDimensionCaption { enabled: true, center: None }),
                        crop,
                        text: String::new(),
                    })
                })
            }
            _ => None,
        };
        // The scale resolves at the first point; later points keep the
        // draft's snapshot.
        if let (Some(m), Some(t)) = (&measure, &t)
            && phase == Phase::Down
            && !continuing
        {
            let (intent, scale) = match m {
                MeasurementAppearance::Distance(d) => ("LineDimension", d.measure.as_ref()),
                MeasurementAppearance::Shape(s) => (
                    if s.intent == ShapeIntent::PolygonDimension {
                        "PolygonDimension"
                    } else {
                        "PolyLineDimension"
                    },
                    s.measure.as_ref(),
                ),
            };
            let o = PdfPoint { x: 0.0, y: 0.0 };
            let readout = measurement_readout(&DimensionInput {
                subtype: &t.subtype,
                intent,
                measure: scale,
                line_points: Some((o, PdfPoint { x: 1.0, y: 0.0 })),
                vertices: vec![o, PdfPoint { x: 1.0, y: 0.0 }, PdfPoint { x: 1.0, y: 1.0 }],
            });
            if readout.readout().is_none() {
                return;
            }
        }
        let ink = t.as_ref().and_then(|t| t.ink);
        self.commit(Msg::CreatePointer {
            measure,
            capture: capture.then(|| tool.to_string()),
            phase,
            subtype: t.as_ref().map_or_else(|| tool.to_string(), |t| t.subtype.clone()),
            preset: Some(preset),
            intent: (t.as_ref().and_then(|t| t.intent.as_deref()) == Some("ink-highlight"))
                .then_some(Intent::InkHighlight),
            click_create: t.as_ref().map(|t| t.click_create),
            flags: t.as_ref().and_then(|t| t.flags),
            defer_ink_commit: Some(ink.and_then(|i| i.group_strokes_ms).unwrap_or(0.0) > 0.0),
            straighten_ink: ink.and_then(|i| i.straighten),
            input: PointerInput {
                page,
                point,
                shift: false,
                finish: Some(finish),
                page_box: self.page_box_of(pon),
                chrome: None,
                display_rotation,
                zoom: None,
                upright: t.as_ref().map(|t| t.upright),
                inert: None,
            },
        });
    }

    /// `hoverAt(at)` (host): dispatches on change only, and feeds the E/X
    /// trigger pump from this seam alone.
    pub fn hover_at(&self, at: Option<HoverAt>) {
        let m = self.model();
        let mut id: Option<String> = None;
        if let Some(at) = at
            && let Target::Annot { id: hit } = self.hit_at(
                at.page,
                at.point,
                HitView { scale: at.scale, rotation: at.rotation, zoom: at.zoom },
                1.0,
                Inert::Engaged,
            )
        {
            id = Some(hit);
        }
        if m.hovered != id {
            if let Some(feed) = &self.0.hover_feed {
                feed.hover(id.as_deref());
            }
            self.commit(Msg::Hover { id });
        }
    }
}
