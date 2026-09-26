//! `write/stamps.ts`: arming a payload for the next click, and the one
//! engine write both the armed and the click-to-place paths funnel through.
//! The armed payload is transient tool state, deliberately not in the store.

use std::cell::RefCell;

use epdf_annotation_core::{
    AnnotationRef, Msg, PageObjectNumber, PageRef, Point, Size, Source, fit_stamp_box,
};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_interaction::INTERACTION;
use serde_json::{Map, Value, json};

use crate::binary::{BinaryMetadata, resolve_binary_source, sniff_binary_metadata};
use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::host_contract::preview_bucket;
use crate::model::AnnotationAction;
use crate::promise::{self, then};
use crate::repository::{annotation_key_of, box_geom_fields};
use crate::services::geometry::page_size_of;
use crate::tools::definitions::{ARMED_STAMP_TOOL_ID, StampSourceSpec};
use crate::value::get;

/// The armed payload: the bytes the next click places, the desired
/// placement size, and the ghost's preview with its per-bucket cache.
#[derive(Clone)]
pub(crate) struct ArmedStamp {
    source: Value,
    width: f64,
    height: f64,
    preview: Option<StampPreviewProvider>,
    preview_cache: std::rc::Rc<RefCell<Vec<(f64, Promise<Option<ArmedStampPreview>>)>>>,
    name: Option<String>,
    subject: Option<String>,
}

impl ArmedStamp {
    pub(crate) fn size(&self) -> Size {
        Size { width: self.width, height: self.height }
    }
}

/// `fixedPreview(bytes, mimeType)`: the same image at every size.
fn fixed_preview(bytes: Vec<u8>, mime_type: Option<String>) -> StampPreviewProvider {
    let preview = ArmedStampPreview { bytes, mime_type };
    std::rc::Rc::new(move |_| Promise::resolved(Some(preview.clone())))
}

const NOT_A_STAMP: &str = "[annotation] stamp source must be PNG, JPEG, or single-page PDF bytes";

/// `desiredStampSize(meta, targetWidth, intrinsicOverride)`.
fn desired_stamp_size(
    meta: &BinaryMetadata,
    target_width: Option<f64>,
    intrinsic_override: Option<Size>,
) -> Size {
    let intrinsic = match (intrinsic_override, meta.size) {
        (Some(o), _) if o.width > 0.0 && o.height > 0.0 => o,
        (_, Some((w, h))) if w > 0.0 => Size { width: w, height: h },
        _ => Size { width: target_width.unwrap_or(150.0), height: target_width.unwrap_or(150.0) },
    };
    match target_width {
        None => intrinsic,
        Some(tw) => Size { width: tw, height: tw * (intrinsic.height / intrinsic.width) },
    }
}

/// The name and subject a placement writes.
#[derive(Clone, Default)]
struct Identity {
    name: Option<String>,
    subject: Option<String>,
}

impl AnnotationCapability {
    pub(crate) fn armed(&self) -> Option<ArmedStamp> {
        self.0.armed.borrow().clone()
    }

    /// `armStamp(input)`: validate the payload now (a bad one fails at the
    /// button, not the click), keep its preview, and activate the stamp tool.
    pub fn arm_stamp(&self, input: StampToolInput) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let resolved = resolve_binary_source(&input.source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(Error::new(NOT_A_STAMP));
            };
            let preview = match &input.preview {
                Some(StampPreview::Provider(p)) => Some(p.clone()),
                Some(StampPreview::Source(p)) if crate::value::truthy(Some(p)) => {
                    let p = resolve_binary_source(p).await?;
                    Some(fixed_preview(p.bytes, p.mime_type))
                }
                _ if meta.mime_type != "application/pdf" => {
                    Some(fixed_preview(resolved.bytes.clone(), Some(meta.mime_type.to_string())))
                }
                _ => None,
            };
            let size = desired_stamp_size(&meta, input.target_width, input.intrinsic_size);
            *this.0.armed.borrow_mut() = Some(ArmedStamp {
                source: input.source,
                width: size.width,
                height: size.height,
                preview,
                preview_cache: Default::default(),
                name: input.name,
                subject: input.subject,
            });
            this.cx().dispatch(AnnotationAction::StampArmChanged);
            if let Some(ix) = this.cx().try_get(&INTERACTION)? {
                ix.activate_tool(ARMED_STAMP_TOOL_ID, None)?;
            }
            Ok(())
        })
    }

    /// `disarmStamp()`.
    pub fn disarm_stamp(&self) {
        if self.0.armed.borrow().is_none() {
            return;
        }
        *self.0.armed.borrow_mut() = None;
        self.cx().dispatch(AnnotationAction::StampArmChanged);
    }

    /// `createStampAt(pon, point, source, desired, rotCW, identity)`: fit the
    /// size to the page and place it centred on the point. `None` when the
    /// page or document isn't ready.
    fn create_stamp_at(
        &self,
        pon: PageObjectNumber,
        point: Point,
        source: Value,
        desired: Size,
        rot_cw: f64,
        identity: Identity,
    ) -> Option<Result<Promise<AnnotationRef>, Error>> {
        let (doc, crop) = (self.doc()?, self.crop_of(pon)?);
        let bx = fit_stamp_box(point, desired, page_size_of(crop), rot_cw);
        let mut draft = Map::new();
        draft.insert("subtype".into(), json!("stamp"));
        draft.extend(box_geom_fields(bx, rot_cw, crop));
        draft.insert("source".into(), source);
        draft.insert("fit".into(), json!("contain"));
        if let Some(name) = identity.name {
            draft.insert("name".into(), json!(name));
        }
        if let Some(subject) = identity.subject {
            draft.insert("subject".into(), json!(subject));
        }
        if let Err(error) = doc.page_index(pon) {
            return Some(Err(error));
        }
        let call = doc.call("page.annotations.create", Some(pon), vec![Value::Object(draft)]);
        let (out, resolver) = Promise::pending();
        let this = self.clone();
        then(call, move |result| match result {
            Ok(res) => {
                let created = get(&res, "created").cloned().unwrap_or(Value::Null);
                this.sync(&created, Source::Baked, false);
                let key = get(&created, "ref").and_then(annotation_key_of);
                if let Some(key) = key {
                    this.commit(Msg::Select { ids: vec![key], add: None });
                }
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
        Some(Ok(out))
    }

    /// `stageStampAt(...)`: the click path's fire-and-forget wrapper; a
    /// refused placement is logged, never thrown into the gesture.
    fn stage_stamp_at(
        &self,
        pon: PageObjectNumber,
        point: Point,
        source: Value,
        desired: Size,
        rot_cw: f64,
        identity: Identity,
    ) -> bool {
        match self.create_stamp_at(pon, point, source, desired, rot_cw, identity) {
            None => false,
            Some(Err(error)) => {
                self.console_error("[annotation] stamp placement failed:", Some(&error));
                true
            }
            Some(Ok(placed)) => {
                let this = self.clone();
                then(placed, move |result| {
                    if let Err(err) = result {
                        this.console_error("[annotation] stamp placement failed:", Some(&err));
                    }
                });
                true
            }
        }
    }

    /// `placeStamp(input, placement)`: programmatic placement, the same law
    /// as a click, awaited.
    pub fn place_stamp(
        &self,
        input: StampToolInput,
        placement: StampPlacement,
    ) -> Promise<AnnotationRef> {
        let this = self.clone();
        promise::spawn(async move {
            let resolved = resolve_binary_source(&input.source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(Error::new(NOT_A_STAMP));
            };
            let pon = placement.page.page_object_number;
            let placed = this.create_stamp_at(
                pon,
                placement.at,
                input.source.clone(),
                desired_stamp_size(
                    &meta,
                    placement.target_width.or(input.target_width),
                    input.intrinsic_size,
                ),
                placement.rotation.unwrap_or(0.0),
                Identity { name: input.name.clone(), subject: input.subject.clone() },
            );
            let Some(placed) = placed else {
                return Err(Error::new(format!(
                    "[annotation] cannot place a stamp on page {pon}: document or page not ready"
                )));
            };
            let placed = placed?;
            // An async function returning a promise adopts it.
            rt::tick().await;
            placed.await
        })
    }

    /// `placeArmedStamp(pon, point, displayRotation)`.
    pub(crate) fn place_armed_stamp_pon(
        &self,
        pon: PageObjectNumber,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        let Some(payload) = self.armed() else { return false };
        let rot = self.upright_rot_for(display_rotation);
        self.stage_stamp_at(
            pon,
            point,
            payload.source.clone(),
            payload.size(),
            rot,
            Identity { name: payload.name.clone(), subject: payload.subject.clone() },
        )
    }

    /// `placeStampSource(pon, point, source, rotCW, targetWidth)`: sniff and
    /// place at the intrinsic size.
    fn place_stamp_source(
        &self,
        pon: PageObjectNumber,
        point: Point,
        source: Value,
        rot_cw: f64,
    ) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let resolved = resolve_binary_source(&source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                this.console_error(NOT_A_STAMP, None);
                return Ok(());
            };
            this.stage_stamp_at(
                pon,
                point,
                source,
                desired_stamp_size(&meta, None, None),
                rot_cw,
                Identity::default(),
            );
            Ok(())
        })
    }

    /// `requestStampAt(pon, point, displayRotation)`: the active tool's
    /// source spec. Fixed bytes place now; a prompt asks the file picker.
    pub(crate) fn request_stamp_at_pon(
        &self,
        pon: PageObjectNumber,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        let Some(tool) = self.active_tool() else { return false };
        let Some(spec) = tool.source.clone() else { return false };
        let rot = self.upright_rot_for(if tool.upright { display_rotation } else { None });
        match spec {
            StampSourceSpec::Bytes { source } => {
                let _ = self.place_stamp_source(pon, point, source, rot);
                true
            }
            StampSourceSpec::Prompt(_) => self.prompt_at(&tool, pon, point, move |this, picked| {
                let data = get(&picked, "data").cloned().unwrap_or_else(epdf_kernel::undefined);
                let _ = this.place_stamp_source(pon, point, data, rot);
            }),
        }
    }

    /// `hasArmedStamp()`.
    pub fn has_armed_stamp(&self) -> bool {
        self.0.armed.borrow().is_some()
    }

    /// `placeArmedStamp(page, point, displayRotation)` (host).
    pub fn place_armed_stamp(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        self.place_armed_stamp_pon(page.page_object_number, point, display_rotation)
    }

    /// `requestStampAt(page, point, displayRotation)` (host).
    pub fn request_stamp_at(
        &self,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        self.request_stamp_at_pon(page.page_object_number, point, display_rotation)
    }

    /// `getArmedStampPreview(devicePixelWidth)` (host): one render per size
    /// bucket for the arm's lifetime; a failed render is logged and `null`.
    pub fn get_armed_stamp_preview(
        &self,
        device_pixel_width: Option<f64>,
    ) -> Promise<Option<ArmedStampPreview>> {
        let Some(payload) = self.armed() else { return Promise::resolved(None) };
        let Some(preview) = payload.preview.clone() else { return Promise::resolved(None) };
        let bucket = preview_bucket(device_pixel_width.unwrap_or(0.0), None);
        let cached = payload
            .preview_cache
            .borrow()
            .iter()
            .find(|(b, _)| *b == bucket)
            .map(|(_, p)| p.clone());
        if let Some(pending) = cached {
            return pending;
        }
        let render = preview(bucket);
        let this = self.clone();
        let pending = Promise::spawn(async move {
            match render.await {
                Ok(v) => Ok(v),
                Err(error) => {
                    this.console_error("[annotation] stamp ghost preview failed:", Some(&error));
                    Ok(None)
                }
            }
        });
        payload.preview_cache.borrow_mut().push((bucket, pending.clone()));
        pending
    }

    /// `getStampArmEpoch()` (host).
    pub fn get_stamp_arm_epoch(&self) -> f64 {
        self.state().stamp_arm_epoch
    }
}
