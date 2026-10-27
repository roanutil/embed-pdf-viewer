//! `read/render.ts`: what a page paints. The vector items (drafts, previews
//! and the tool's ghost ride the same pipeline), the editable text items, the
//! navigable link areas, and the baked-appearance seam (epoch and pictures).
//! The engine's pictures come through the render plugin, which shares and
//! keeps them. A stamp this session placed shows the preview it was placed
//! with until the engine's picture of it exists, and an annotation an undo
//! brings back the picture it had.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::flags::viewable;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::plane::is_substrate_only;
use epdf_annotation_core::record::{group_of, shape_of};
use epdf_annotation_core::types::{Model, Shape, Source, ViewEnv};
use epdf_annotation_core::view::page_items;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_render::{AppearanceFamily, AppearanceItem, AppearancesOptions, RENDER};
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::js::{key_or_none, num, page_object_number};
use crate::model::{ForeignPlacement, GhostPointer};
use crate::read::chrome::core_page;
use crate::text_item::build_text_items;

type ItemsKey =
    (Rc<Model>, Option<Rc<GhostPointer>>, Option<Rc<ForeignPlacement>>, Option<(f64, f64)>);

#[derive(Default)]
pub struct RenderState {
    items: RefCell<HashMap<String, (ItemsKey, Value)>>,
    texts: RefCell<HashMap<String, (Rc<Model>, Option<(f64, f64)>, Value)>>,
    links: RefCell<HashMap<String, (Rc<Model>, Value)>>,
}

fn opt_ptr<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn view_key(view: Option<&ViewEnv>) -> Option<(f64, f64)> {
    view.map(|v| (v.zoom, v.rotation))
}

fn page_key(page: f64) -> String {
    epdf_annotation_core::js::number_to_string(page)
}

/// What a page's pictures depend on.
struct Epoch {
    engine: Vec<AppearanceItem>,
    placed: String,
    lent: String,
}

impl Core {
    /// `listPageItems(page, view)`.
    pub fn list_page_items(&self, page: &Value, view: Option<ViewEnv>) -> Value {
        let number = page_object_number(page);
        let model = self.page_model(number);
        let state = self.state();
        let ghost_at = state.ghost_at.clone().filter(|g| page_object_number(&g.page) == number);
        let placing = state.placing.clone().filter(|p| page_object_number(&p.page) == number);
        let key: ItemsKey = (model.clone(), ghost_at, placing, view_key(view.as_ref()));
        if let Some((k, v)) = self.render.items.borrow().get(&page_key(number))
            && Rc::ptr_eq(&k.0, &key.0)
            && opt_ptr(&k.1, &key.1)
            && opt_ptr(&k.2, &key.2)
            && k.3 == key.3
        {
            return v.clone();
        }
        let mut items: Vec<Value> =
            page_items(&model, &core_page(page), view.as_ref()).iter().map(Json::json).collect();
        items.extend(self.ghost_items_on(number, &model, view.as_ref()));
        let value = Value::Array(items);
        self.render.items.borrow_mut().insert(page_key(number), (key, value.clone()));
        value
    }

    /// `listTextItems(page, view)`.
    pub fn list_text_items(&self, page: &Value, view: Option<ViewEnv>) -> Value {
        let number = page_object_number(page);
        let model = self.page_model(number);
        if let Some((m, vk, v)) = self.render.texts.borrow().get(&page_key(number))
            && Rc::ptr_eq(m, &model)
            && *vk == view_key(view.as_ref())
        {
            return v.clone();
        }
        let items = Value::Array(build_text_items(&model, &core_page(page), view.as_ref()));
        self.render
            .texts
            .borrow_mut()
            .insert(page_key(number), (model, view_key(view.as_ref()), items.clone()));
        items
    }

    /// `listLinkItems(page)`: standalone links and attached-link segments.
    pub fn list_link_items(&self, page: &Value) -> Value {
        let number = page_object_number(page);
        let model = self.page_model(number);
        if let Some((m, v)) = self.render.links.borrow().get(&page_key(number))
            && Rc::ptr_eq(m, &model)
        {
            return v.clone();
        }
        let mut items: Vec<Value> = Vec::new();
        for id in model.order.iter() {
            let Some(record) = model.by_id.get(id) else { continue };
            let annotation = &record.annotation;
            if record.page_number() != Some(number)
                || annotation.get("subtype").and_then(Value::as_str) != Some("link")
            {
                continue;
            }
            if !viewable(annotation, false) {
                continue;
            }
            let target =
                annotation.get("target").filter(|t| !t.is_null() && !epdf_kernel::is_undefined(t));
            let Shape::Box(geometry) = shape_of(annotation) else { continue };
            let Some(target) = target else { continue };
            let actions = annotation.get("actions").filter(|a| a.is_object());
            let activate = actions
                .and_then(|a| a.get("activate"))
                .filter(|a| !a.is_null() && !epdf_kernel::is_undefined(a));
            let reference = annotation.get("ref").filter(|r| !r.is_null());
            let root = |name: &str| {
                actions
                    .and_then(|a| a.get(name))
                    .and_then(|e| e.get("root"))
                    .is_some_and(|r| epdf_annotation_core::json::truthy(Some(r)))
            };
            let (enter, exit) = (root("cursorEnter"), root("cursorExit"));
            let mut item = Map::new();
            item.insert("id".into(), json!(id));
            item.insert("bounds".into(), geometry.box_.json());
            item.insert("target".into(), target.clone());
            item.insert("attached".into(), json!(group_of(annotation).is_some()));
            if let Some(a) = activate {
                item.insert("activate".into(), a.clone());
            }
            if let Some(r) = reference {
                item.insert("ref".into(), r.clone());
            }
            if enter || exit {
                item.insert("hoverEvents".into(), json!({ "enter": enter, "exit": exit }));
            }
            items.push(Value::Object(item));
        }
        let value = Value::Array(items);
        self.render.links.borrow_mut().insert(page_key(number), (model, value.clone()));
        value
    }

    /// The engine's last pictures of a record, kept a while after it left its page.
    fn kept_of(&self, id: &str, reference: &Value) -> Vec<Value> {
        let Ok(Some(render)) = self.cx.try_get(&RENDER) else { return Vec::new() };
        let family = if self.lending.owns(id) {
            AppearanceFamily::Widgets
        } else {
            AppearanceFamily::Annotations
        };
        render
            .kept_appearances(family, reference)
            .iter()
            .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
            .collect()
    }

    fn epoch_of(&self, page: &Value) -> Epoch {
        let number = page_object_number(page);
        let model = self.page_model(number);
        let mut engine = Vec::new();
        let mut placed: Vec<String> = Vec::new();
        let mut lent_shown = false;
        for id in model.order.iter() {
            let Some(record) = model.by_id.get(id) else { continue };
            if record.page_number() != Some(number) || record.source != Source::Baked {
                continue;
            }
            if is_substrate_only(&record.annotation) {
                continue;
            }
            if record.unconfirmed == Some(true) {
                let reference = record.annotation.get("ref").cloned().unwrap_or(Value::Null);
                if self.look_of(record).is_some() {
                    placed.push(format!("{id}@placed"));
                } else if !self.kept_of(id, &reference).is_empty() {
                    placed.push(format!("{id}@kept"));
                }
                continue;
            }
            if self.lending.owns(id) {
                lent_shown = true;
                continue;
            }
            engine.push(AppearanceItem {
                reference: record.annotation.get("ref").cloned().unwrap_or(Value::Null),
                version: record.picture_version.unwrap_or(0.0),
            });
        }
        placed.sort();
        let lent = match (lent_shown, self.lending.lender()) {
            (true, Some(lender)) => format!("lent:{}", lender.picture_epoch(page)),
            _ => String::new(),
        };
        Epoch { engine, placed: placed.join("|"), lent }
    }

    /// `getAppearanceEpoch(page)`.
    pub fn get_appearance_epoch(&self, page: &Value) -> String {
        let epoch = self.epoch_of(page);
        let mut drawn: Vec<String> = epoch
            .engine
            .iter()
            .map(|item| {
                format!(
                    "{}@{}",
                    key_or_none(&item.reference),
                    epdf_annotation_core::js::number_to_string(item.version)
                )
            })
            .collect();
        drawn.sort();
        let drawn = drawn.join("|");
        [drawn, epoch.placed, epoch.lent]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// `renderAppearances(page, scale, signal)`: every picture the page's
    /// annotations show at rest.
    pub fn render_appearances(
        &self,
        page: &Value,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>> {
        let epoch = self.epoch_of(page);
        let theirs = self.engine_pictures(page, scale, epoch.engine, signal.clone());
        let lent = self.lent_pictures(page, scale, signal);
        let placed = self.own_pictures(page, scale);
        let all = crate::promise::all(vec![theirs, lent, placed]);
        Promise::spawn(async move { Ok(all.await?.into_iter().flatten().collect()) })
    }

    fn lent_pictures(
        &self,
        page: &Value,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>> {
        let Some(lender) = self.lending.lender() else { return Promise::resolved(Vec::new()) };
        let model = self.page_model(page_object_number(page));
        let lends = model.order.iter().any(|id| {
            self.lending.owns(id)
                && model.by_id.get(id).is_some_and(|r| r.unconfirmed != Some(true))
        });
        if !lends {
            return Promise::resolved(Vec::new());
        }
        let pictures = lender.render_appearances(page, scale, signal);
        Promise::spawn(async move { Ok(pictures.await.unwrap_or_default()) })
    }

    fn engine_pictures(
        &self,
        page: &Value,
        scale: f64,
        items: Vec<AppearanceItem>,
        signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>> {
        let render = self.cx.try_get(&RENDER).ok().flatten();
        let (Some(render), Some(_)) = (render, self.doc()) else {
            return Promise::resolved(Vec::new());
        };
        if items.is_empty() {
            return Promise::resolved(Vec::new());
        }
        let Some(page_ref) = epdf_kernel::geometry::PageRef::from_json(page) else {
            return Promise::resolved(Vec::new());
        };
        let pictures = render.render_appearances(
            page_ref,
            AppearancesOptions { family: AppearanceFamily::Annotations, scale, items, signal },
        );
        Promise::spawn(async move {
            Ok(match pictures.await {
                Ok(list) => {
                    list.iter().map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).collect()
                }
                Err(_) => Vec::new(),
            })
        })
    }

    fn own_pictures(&self, page: &Value, scale: f64) -> Promise<Vec<Value>> {
        let model = self.page_model(page_object_number(page));
        let mut parts: Vec<Promise<Vec<Value>>> = Vec::new();
        for id in model.order.iter() {
            let Some(record) = model.by_id.get(id).cloned() else { continue };
            if record.unconfirmed != Some(true) {
                parts.push(Promise::resolved(Vec::new()));
                continue;
            }
            let version = record.picture_version.unwrap_or(0.0);
            let reference = record.annotation.get("ref").cloned().unwrap_or(Value::Null);
            let Some(look) = self.look_of(&record) else {
                let kept: Vec<Value> = self
                    .kept_of(id, &reference)
                    .into_iter()
                    .map(|mut kept| {
                        kept["version"] = num(version);
                        kept
                    })
                    .collect();
                parts.push(Promise::resolved(kept));
                continue;
            };
            let Some(rect) = record.ap_box else {
                parts.push(Promise::resolved(Vec::new()));
                continue;
            };
            let preview = look(rect.width * scale);
            parts.push(Promise::spawn(async move {
                let Some(preview) = preview.await? else { return Ok(Vec::new()) };
                let mime = preview.get("mimeType").and_then(Value::as_str).unwrap_or("image/png").to_string();
                Ok(vec![json!({
                    "ref": reference,
                    "mode": "normal",
                    "state": null,
                    "rect": rect.json(),
                    // `imageSourceOfBytes(bytes, contentType)`: an `ImageSource` whose
                    // `blob()` and `objectUrl()` read these bytes.
                    "image": { "$imageBytes": { "bytes": preview.get("bytes").cloned().unwrap_or(Value::Null), "contentType": mime } },
                    "version": num(version),
                })])
            }));
        }
        let all = crate::promise::all(parts);
        Promise::spawn(async move { Ok(all.await?.into_iter().flatten().collect()) })
    }
}
