//! `write/fields.ts`: design mode. Fields and widgets are created, patched,
//! deleted, attached and detached through `doc.forms`; the widget plane is
//! nudged to re-read pages whose widget population changed.

use epdf_geometry::js::{max, min};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::contract::{CreateFieldInput, CreatedField};
use crate::controller::{FormCapability, key_of};
use crate::core::fill_items::page_of;
use crate::core::model::{Box, Msg, field_by_key};
use crate::promise::await_engine;
use crate::value::{get, num, str_of, truthy};

fn page_ref_of(value: Option<&Value>) -> Option<PageRef> {
    value.and_then(PageRef::from_json)
}

impl FormCapability {
    /// `nudgeAnnotations(pages)`: re-read each page once on the annotation
    /// plane, unawaited.
    fn nudge_annotations(&self, pages: &[PageRef]) {
        let Some(annotation) = &self.0.annotation else { return };
        let mut seen: Vec<u32> = Vec::new();
        for page in pages {
            if seen.contains(&page.page_object_number) {
                continue;
            }
            seen.push(page.page_object_number);
            annotation
                .reload_page(epdf_annotation_core::PageRef::new(page.page_object_number))
                .ignore();
        }
    }

    /// `autoName(family)`: `text_1`, `text_2`, ... against the current
    /// snapshot.
    fn auto_name(&self, family: &str) -> String {
        let model = self.model();
        let names: Vec<&str> = model
            .snapshot
            .as_ref()
            .and_then(|s| get(s, "fields"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| str_of(f, "name"))
            .collect();
        let mut n = 1u64;
        while names.contains(&format!("{family}_{n}").as_str()) {
            n += 1;
        }
        format!("{family}_{n}")
    }

    async fn place_field(&self, input: CreateFieldInput) -> Result<CreatedField, Error> {
        let page = input.page;
        let pon = f64::from(page.page_object_number);
        let space = self.cx().try_for_page(page);
        let (Some(doc), Some(space)) = (self.doc(), space) else {
            return Err(plugin_error(
                code::NOT_READY,
                "form",
                "createField: document/page not ready",
                None,
                None,
            ));
        };
        // Placement is page-bound: intersect the box with the page.
        let bounds = self.get_page_box(page).unwrap_or_default();
        let b = input.bounds;
        let x = max(bounds.x, min(b.x, bounds.width));
        let y = max(bounds.y, min(b.y, bounds.height));
        let bx = Box {
            x,
            y,
            width: max(0.0, min(b.x + b.width, bounds.width) - x),
            height: max(0.0, min(b.y + b.height, bounds.height) - y),
        };
        if bx.width < 1.0 || bx.height < 1.0 {
            return Err(plugin_error(
                code::INVALID_INPUT,
                "form",
                "createField: degenerate bounds (size the box before placing)",
                None,
                None,
            ));
        }
        let family = input.family.as_str();
        let name = match &input.name {
            Some(name) => name.clone(),
            None => self.auto_name(family),
        };
        let rect = space.page_rect_to_pdf(epdf_kernel::geometry::Rect {
            x: bx.x,
            y: bx.y,
            width: bx.width,
            height: bx.height,
        });
        let mut placement = json!({
            "page": page.to_json(),
            "rect": {
                "left": crate::value::number(rect.left),
                "right": crate::value::number(rect.right),
                "top": crate::value::number(rect.top),
                "bottom": crate::value::number(rect.bottom),
            },
        });
        if let Some(appearance) = input.appearance.filter(|a| truthy(Some(a))) {
            placement["appearance"] = appearance;
        }
        let draft = match family {
            "radio" => {
                let mut widget = placement;
                widget["onState"] = json!("option1");
                json!({ "family": family, "name": name, "widgets": [widget] })
            }
            "combobox" | "listbox" => json!({
                "family": family,
                "name": name,
                "widget": placement,
                "options": match &input.options {
                    Some(options) => json!(options),
                    None => json!([
                        { "label": "Option 1", "value": "Option 1" },
                        { "label": "Option 2", "value": "Option 2" },
                    ]),
                },
            }),
            _ => json!({ "family": family, "name": name, "widget": placement }),
        };
        let result = await_engine(doc.call("forms.createField", None, vec![draft])).await?;
        self.refresh().await?;
        self.apply(Msg::ClearGeom { page_object_number: Some(pon) });
        // Await the annotation-plane reload so the widget is selectable.
        if let Some(annotation) = &self.0.annotation {
            annotation
                .reload_page(epdf_annotation_core::PageRef::new(page.page_object_number))
                .await?;
        }
        let field = get(&result, "field").cloned().unwrap_or_else(epdf_kernel::undefined);
        let widget = get(&field, "widgets")
            .and_then(Value::as_array)
            .and_then(|widgets| widgets.iter().find(|w| page_of(w) == Some(pon)))
            .cloned();
        Ok(CreatedField { field, widget })
    }

    /// `createField(input)`: a field with one widget at a page-space box,
    /// clamped to the page, auto-named unless named.
    pub fn create_field(&self, input: CreateFieldInput) -> Promise<CreatedField> {
        let this = self.clone();
        self.0.queue.run(move || Ok(Promise::spawn(async move { this.place_field(input).await })))
    }

    /// `updateField(ref, patch)`.
    pub fn update_field(&self, field_ref: Value, patch: Value) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                await_engine(doc.call("forms.updateField", None, vec![field_ref, patch])).await?;
                this.refresh().await
            }))
        })
    }

    /// `deleteField(ref)`: the field and its widgets.
    pub fn delete_field(&self, field_ref: Value) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                let pages: Vec<PageRef> = field_by_key(&this.model(), &key_of(&field_ref))
                    .and_then(|f| get(&f, "widgets").and_then(Value::as_array).cloned())
                    .into_iter()
                    .flatten()
                    .filter(|w| truthy(get(w, "page")))
                    .filter_map(|w| page_ref_of(get(&w, "page")))
                    .collect();
                await_engine(doc.call("forms.deleteField", None, vec![field_ref])).await?;
                this.refresh().await?;
                let mut seen: Vec<u32> = Vec::new();
                for page in &pages {
                    if !seen.contains(&page.page_object_number) {
                        seen.push(page.page_object_number);
                        this.apply(Msg::ClearGeom {
                            page_object_number: Some(f64::from(page.page_object_number)),
                        });
                    }
                }
                this.nudge_annotations(&pages);
                Ok(())
            }))
        })
    }

    fn relink_widget(&self, path: &'static str, field_ref: Value, widget: Value) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                let page = get(&widget, "page").cloned().unwrap_or(Value::Null);
                await_engine(doc.call(path, None, vec![field_ref, widget])).await?;
                this.refresh().await?;
                this.apply(Msg::ClearGeom {
                    page_object_number: Some(num(&page, "pageObjectNumber").unwrap_or(f64::NAN)),
                });
                if let Some(page) = PageRef::from_json(&page) {
                    this.nudge_annotations(&[page]);
                }
                Ok(())
            }))
        })
    }

    /// `detachWidget(ref, widget)`: the widget stays as an inert
    /// annotation; the field survives.
    pub fn detach_widget(&self, field_ref: Value, widget: Value) -> Promise<()> {
        self.relink_widget("forms.detachWidget", field_ref, widget)
    }

    /// `attachWidget(ref, widget)`: link an inert widget annotation to a
    /// field.
    pub fn attach_widget(&self, field_ref: Value, widget: Value) -> Promise<()> {
        self.relink_widget("forms.attachWidget", field_ref, widget)
    }
}
