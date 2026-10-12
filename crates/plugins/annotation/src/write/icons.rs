//! `write/icons.ts`: icon annotations (sticky notes, file attachments) at a
//! click, and the one click-to-place entry the place handler forwards every
//! down to.

use epdf_annotation_core::{
    AnnotationRef, Msg, PageObjectNumber, PageRef, Point, Source, defaults_for, fit_stamp_box,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::controller::AnnotationCapability;
use crate::promise::{self, then};
use crate::repository::{annotation_key_of, box_geom_fields};
use crate::services::events::origin_api;
use crate::services::geometry::page_size_of;
use crate::tools::definitions::ResolvedTool;
use crate::value::get;
use crate::write::placement::{ICON_PLACE_SIZE, icon_placement_draft, is_icon_place_kind};

impl AnnotationCapability {
    /// `createIcon(doc, pon, draft)`: engine create and model sync; selects
    /// the result and announces it.
    fn create_icon(&self, pon: PageObjectNumber, draft: Value) -> Promise<AnnotationRef> {
        let Some(doc) = self.doc() else {
            return Promise::rejected(Error::new("[annotation] no document bound"));
        };
        if let Err(error) = doc.page_index(pon) {
            return Promise::rejected(error);
        }
        let call = doc.call("page.annotations.create", Some(pon), vec![draft]);
        let (out, resolver) = Promise::pending();
        let this = self.clone();
        then(call, move |result| match result {
            Ok(res) => {
                let created = get(&res, "created").cloned().unwrap_or(Value::Null);
                if let Err(error) = this.sync(&created, Source::Baked, false) {
                    resolver.reject(error);
                    return;
                }
                if let Some(key) = get(&created, "ref").and_then(annotation_key_of) {
                    this.commit(Msg::Select { ids: vec![key], add: None });
                }
                this.announce_created(&created, origin_api());
                match get(&created, "ref").and_then(|r| serde_json::from_value(r.clone()).ok()) {
                    Some(r) => resolver.resolve(r),
                    None => resolver.reject(Error::named(
                        "TypeError",
                        "Cannot read properties of undefined (reading 'ref')",
                    )),
                }
            }
            Err(error) => resolver.reject(error),
        });
        out
    }

    /// `placeIconAt(tool, pon, point, rotCW, file)`.
    fn place_icon_at(
        &self,
        tool: &ResolvedTool,
        pon: PageObjectNumber,
        point: Point,
        rot_cw: f64,
        file: Option<Value>,
    ) -> bool {
        if self.doc().is_none() {
            return false;
        }
        let Some(crop) = self.crop_of(pon) else { return false };
        if !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let bx = fit_stamp_box(point, ICON_PLACE_SIZE, page_size_of(crop), rot_cw);
        let defaults = defaults_for(&self.model(), &tool.preset);
        let draft = match icon_placement_draft(
            &tool.subtype,
            box_geom_fields(bx, rot_cw, crop),
            &defaults,
            tool.flags.as_ref(),
            file,
        ) {
            Ok(d) => d,
            Err(error) => {
                self.console_error("[annotation] icon placement failed:", Some(&error));
                return true;
            }
        };
        let placed = self.create_icon(pon, Value::Object(draft));
        let this = self.clone();
        then(placed, move |result| {
            if let Err(err) = result {
                this.console_error("[annotation] icon placement failed:", Some(&err));
            }
        });
        true
    }

    /// `placeAt(pon, point, displayRotation)`: armed payload first, then the
    /// active tool's kind decides.
    pub(crate) fn place_at_pon(
        &self,
        pon: PageObjectNumber,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        if self.place_armed_stamp_pon(pon, point, display_rotation) {
            return true;
        }
        let Some(tool) = self.active_tool() else { return false };
        if tool.subtype == "stamp" {
            return self.request_stamp_at_pon(pon, point, display_rotation);
        }
        if !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let rot = self.upright_rot_for(if tool.upright { display_rotation } else { None });
        if tool.subtype == "text" {
            return self.place_icon_at(&tool, pon, point, rot, None);
        }
        let placing = tool.clone();
        self.prompt_at(&tool, pon, point, move |this, picked| {
            this.place_icon_at(&placing, pon, point, rot, Some(picked));
        })
    }

    /// `placeAt(page, point, displayRotation)` (host): the one
    /// click-to-place entry; whether the click was consumed.
    pub fn place_at(&self, page: PageRef, point: Point, display_rotation: Option<f64>) -> bool {
        self.place_at_pon(page.page_object_number, point, display_rotation)
    }

    /// `createAttachment(page, at, file)`: the attachment tool's icon at a
    /// page point with the given file.
    pub fn create_attachment(
        &self,
        page: PageRef,
        at: Point,
        file: Value,
    ) -> Promise<AnnotationRef> {
        let this = self.clone();
        promise::spawn(async move {
            this.assert_create()?;
            this.assert_page(page)?;
            let pon = page.page_object_number;
            let crop = this.crop_of(pon);
            let tool = this.tool("attachment");
            let (Some(_), Some(crop), Some(tool)) = (this.doc(), crop, tool) else {
                return Err(unsupported());
            };
            if !is_icon_place_kind(&tool.subtype) {
                return Err(unsupported());
            }
            let bx = fit_stamp_box(at, ICON_PLACE_SIZE, page_size_of(crop), 0.0);
            let draft = icon_placement_draft(
                &tool.subtype,
                box_geom_fields(bx, 0.0, crop),
                &defaults_for(&this.model(), &tool.preset),
                tool.flags.as_ref(),
                Some(file),
            )?;
            let placed = this.create_icon(pon, Value::Object(draft));
            // `.catch(rethrow)`: a derived promise, then the async return
            // adopts it.
            let caught = Promise::spawn(async move {
                placed.await.map_err(|error| to_plugin_error("annotation", &error))
            });
            epdf_kernel::rt::tick().await;
            caught.await
        })
    }

    /// `readAttachment(ref)`: the embedded file. An engine whose page
    /// service has no `downloadFile` refuses.
    pub fn read_attachment(&self, r: AnnotationRef) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = this.page_of(&r);
            doc.page_index(pon)?;
            if !doc.has("page.annotations.downloadFile") {
                return Err(Error::new(
                    "[annotation] this engine does not support attachment download",
                ));
            }
            let call = doc.call(
                "page.annotations.downloadFile",
                Some(pon),
                vec![serde_json::to_value(&r).unwrap_or_default()],
            );
            // An async function returning a thenable adopts it.
            epdf_kernel::rt::tick().await;
            call.await
        })
    }
}

fn unsupported() -> Error {
    plugin_error(code::UNSUPPORTED, "annotation", "no attachment tool is registered", None, None)
}
