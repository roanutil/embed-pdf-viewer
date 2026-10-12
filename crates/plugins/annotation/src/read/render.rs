//! `read/render.ts`: what a page paints. The vector items (drafts, previews
//! and the tool ghost ride the same pipeline), the editable text items, the
//! navigable link areas, and the baked-appearance seam (epoch, bake scale,
//! rasters).

use std::rc::Rc;

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{
    AnnotationRef, Geom, PageRef, PdfRect, Rect, RenderItem, RenderSource, Source, ViewEnv,
    defaults_for, is_substrate_only, pdf_to_content_rect, style_from_props, viewable,
};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_render::policy::{EngineRenderPolicy, snap_appearance_scale};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::promise::then;
use crate::repository::seam::num_json;
use crate::text_item::build_text_items;
use crate::value::{get, str_of, truthy};

impl AnnotationCapability {
    /// `listPageItems(page, view)` (host): the render items, the armed
    /// tool's vector ghost included.
    pub fn list_page_items(&self, page: PageRef, view: Option<ViewEnv>) -> Vec<RenderItem> {
        let m = self.model();
        let g = self.state().tool_ghost.clone();
        let mut v = epdf_annotation_core::page_items(&m, page, view);
        if let Some(ghost) = g.as_ref().filter(|g| g.page.same_page(&page))
            && let ToolGhostKind::Vector { tool_id, geom } = &ghost.kind
        {
            let tool = self.tool(tool_id);
            let preset = tool.as_ref().map_or_else(|| tool_id.clone(), |t| t.preset.clone());
            let style = style_from_props(&defaults_for(&m, &preset));
            v.push(RenderItem {
                id: "tool-ghost".into(),
                ref_: None,
                subtype: tool.map_or_else(|| "square".into(), |t| t.subtype),
                geom: geom.clone(),
                box_: ghost.box_,
                ap_box: None,
                ap_rot: None,
                style,
                text: None,
                source: RenderSource::Ghost,
                selected: false,
                hovered: None,
                label: None,
                measure: None,
                rot: None,
                blend: None,
            });
        }
        v
    }

    /// `listTextItems(page, view)` (host).
    pub fn list_text_items(&self, page: PageRef, view: Option<ViewEnv>) -> Vec<TextItem> {
        build_text_items(&self.model(), page, view)
    }

    /// `listLinkItems(page)` (host): the navigation plane's feed.
    pub fn list_link_items(&self, page: PageRef) -> Vec<LinkNavItem> {
        let m = self.model();
        let mut v = Vec::new();
        for id in m.order.iter() {
            let Some(a) = m.by_id.get(id) else { continue };
            if !a.page.same_page(&page) || a.subtype != "link" || !viewable(&a.flags, false) {
                continue;
            }
            let data = a.data.as_ref();
            let target = match &a.link {
                Nullable::Value(t) => serde_json::to_value(t).unwrap_or_default(),
                _ => match data {
                    Some(d) if str_of(d, "subtype") == Some("link") => {
                        get(d, "target").cloned().unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                },
            };
            let Geom::Rect { rect, .. } = a.geom else { continue };
            if target.is_null() {
                continue;
            }
            let actions = data.and_then(|d| get(d, "actions"));
            let activate =
                actions.and_then(|x| get(x, "activate")).filter(|v| truthy(Some(v))).cloned();
            let ref_: Option<AnnotationRef> = a.ref_.clone().or_else(|| {
                data.and_then(|d| get(d, "ref"))
                    .and_then(|r| serde_json::from_value(r.clone()).ok())
            });
            let root =
                |k: &str| truthy(actions.and_then(|x| get(x, k)).and_then(|t| get(t, "root")));
            let (enter, exit) = (root("cursorEnter"), root("cursorExit"));
            v.push(LinkNavItem {
                id: id.clone(),
                bounds: rect,
                target,
                attached: a.group.is_some(),
                activate,
                ref_,
                hover_events: (enter || exit).then_some(HoverFlags { enter, exit }),
            });
        }
        v
    }

    /// `getAppearanceEpoch(page)` (host): the committed id and `/AP` version
    /// of every baked annotation on the page.
    pub fn get_appearance_epoch(&self, page: PageRef) -> String {
        let m = self.model();
        let mut parts: Vec<String> = Vec::new();
        for id in m.order.iter() {
            let Some(a) = m.by_id.get(id) else { continue };
            if !a.page.same_page(&page)
                || a.source != Source::Baked
                || a.ref_.is_none()
                || is_substrate_only(a)
            {
                continue;
            }
            parts.push(format!(
                "{id}@{}",
                epdf_geometry::js::number_to_string(a.ap_version.unwrap_or(0.0))
            ));
        }
        parts.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        parts.join("|")
    }

    /// `getBakeScale(renderScale)` (host): the document's render policy
    /// snapped; identity under `continuous`.
    pub fn get_bake_scale(&self, render_scale: f64) -> Result<f64, Error> {
        let policy = self
            .cx()
            .document()
            .and_then(|d| serde_json::from_value::<EngineRenderPolicy>(d.render_policy).ok())
            .unwrap_or(EngineRenderPolicy::Continuous);
        snap_appearance_scale(&policy, render_scale)
    }

    /// `renderAppearances(page, scale, signal)` (host): the baked `/AP`
    /// images, `[]` on any failure. `page(ref)` refuses an unknown page
    /// synchronously.
    pub fn render_appearances(
        &self,
        page: PageRef,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Result<Promise<Value>, Error> {
        let Some(doc) = self.doc() else { return Ok(Promise::resolved(json!([]))) };
        let pon = page.page_object_number;
        doc.page_index(pon)?;
        let task = doc.call(
            "page.annotations.renderAppearanceImages",
            Some(pon),
            vec![json!({ "scale": num_json(scale) })],
        );
        if let Some(signal) = signal {
            if signal.aborted() {
                task.abort(signal.reason().map(|r| r.to_canonical()));
            } else {
                let t = task.clone();
                signal
                    .on_abort(Rc::new(move |reason: &Error| t.abort(Some(reason.to_canonical()))));
            }
        }
        let (out, resolver) = Promise::pending();
        then(task, move |result| match result {
            Ok(r) => resolver
                .resolve(get(&r, "appearances").cloned().unwrap_or_else(epdf_kernel::undefined)),
            Err(_) => resolver.resolve(json!([])),
        });
        Ok(out)
    }

    /// `pdfToPageRect(page, rect)` (host).
    pub fn pdf_to_page_rect(&self, page: PageRef, rect: PdfRect) -> Option<Rect> {
        self.crop_of(page.page_object_number).map(|crop| pdf_to_content_rect(rect, crop))
    }
}
