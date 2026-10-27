//! `write/stamps.ts`: stamps. Arming a payload for the next click, and the
//! one engine write both the armed and the click-to-place paths funnel
//! through. A stamp source is a `BinarySource` as JSON (`{"$bytes": …}`, or
//! `{ data, mimeType?, name? }`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::geometry::fit_stamp_box;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{Id, ModelAnnotation, Point};
use epdf_geometry::Size;
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_interaction::INTERACTION;
use serde_json::{Map, Value, json};

use crate::binary::{BinaryMetadata, resolve_binary_source, sniff_binary_metadata};
use crate::controller::{CAPABILITY, Core};
use crate::engine::to_page_ref;
use crate::host_contract::{StampPreviewProvider, preview_bucket};
use crate::js::{Object, num, number};
use crate::model::set_ghost_at;
use crate::services::staging::StoreChange;
use crate::services::store::Applied;
use crate::tools::definitions::ARMED_STAMP_TOOL_ID;
use crate::write::outcomes::applied_annotation_of;

/// The armed stamp-tool payload: the bytes the next click places, and its
/// placement size.
pub struct ArmedStamp {
    pub source: Value,
    pub width: f64,
    pub height: f64,
    pub preview: Option<StampPreviewProvider>,
    pub name: Option<Value>,
    pub subject: Option<Value>,
}

#[derive(Default)]
pub struct StampsState {
    pub(crate) armed: RefCell<Option<Rc<ArmedStamp>>>,
    /// The look of each stamp this session placed whose create the engine hasn't answered.
    looks: RefCell<HashMap<Id, StampPreviewProvider>>,
    armed_info: RefCell<Option<Value>>,
}

fn not_a_picture() -> Error {
    plugin_error(
        code::INVALID_INPUT,
        CAPABILITY,
        "a stamp must be PNG, JPEG, or one-page PDF bytes",
        None,
        None,
    )
}

/// The bytes of a stamp source; its declared type and name don't matter.
fn bytes_of(source: &Value) -> Value {
    if Bytes::from_json(source).is_some() || source.get("data").is_none() {
        source.clone()
    } else {
        source["data"].clone()
    }
}

/// Fixed bytes as a provider: the same image at every size.
fn fixed_preview(bytes: Vec<u8>, mime_type: Option<String>) -> StampPreviewProvider {
    let mut preview = Map::new();
    preview.insert("bytes".into(), Bytes(bytes).to_json());
    if let Some(m) = mime_type {
        preview.insert("mimeType".into(), json!(m));
    }
    let preview = Value::Object(preview);
    Rc::new(move |_| Promise::resolved(Some(preview.clone())))
}

/// `provider`, asked once per size bucket. A render that fails is logged, and is none.
fn cached_preview(
    provider: StampPreviewProvider,
    log: Rc<dyn Fn(&str, Value)>,
) -> StampPreviewProvider {
    let renders: Rc<RefCell<HashMap<u64, Promise<Option<Value>>>>> = Rc::default();
    Rc::new(move |width| {
        let bucket = preview_bucket(width, 4096.0);
        if let Some(render) = renders.borrow().get(&bucket.to_bits()) {
            return render.clone();
        }
        let asked = provider(bucket);
        let log = log.clone();
        let render = Promise::spawn(async move {
            match asked.await {
                Ok(v) => Ok(v),
                Err(error) => {
                    log("[annotation] stamp preview failed:", error.to_canonical());
                    Ok(None)
                }
            }
        });
        renders.borrow_mut().insert(bucket.to_bits(), render.clone());
        render
    })
}

/// `desiredStampSize(meta, targetWidth, intrinsic)`.
fn desired_stamp_size(
    meta: &BinaryMetadata,
    target_width: Option<f64>,
    intrinsic: Option<(f64, f64)>,
) -> (f64, f64) {
    let intrinsic = match intrinsic {
        Some((w, h)) if w > 0.0 && h > 0.0 => (w, h),
        _ => match meta.size {
            Some((w, h)) if w > 0.0 => (w, h),
            _ => {
                let side = target_width.unwrap_or(150.0);
                (side, side)
            }
        },
    };
    match target_width {
        None => intrinsic,
        Some(width) => (width, width * (intrinsic.1 / intrinsic.0)),
    }
}

/// A stamp input's members (`StampInput`).
pub struct StampInput {
    pub source: Value,
    pub target_width: Option<f64>,
    pub name: Option<Value>,
    pub subject: Option<Value>,
    pub preview: Option<StampPreview>,
    pub intrinsic_size: Option<(f64, f64)>,
}

/// `StampInput.preview`: fixed bytes, or a provider.
pub enum StampPreview {
    Source(Value),
    Provider(StampPreviewProvider),
}

impl StampInput {
    /// A stamp input read from JSON; `preview` given as a source.
    pub fn read(value: &Value) -> StampInput {
        let present = |k: &str| value.get(k).filter(|v| !epdf_kernel::is_undefined(v));
        StampInput {
            source: present("source").cloned().unwrap_or_else(epdf_kernel::undefined),
            target_width: present("targetWidth").and_then(number),
            name: present("name").cloned(),
            subject: present("subject").cloned(),
            // `if (input.preview)`: a falsy preview is no preview.
            preview: present("preview")
                .filter(|p| epdf_annotation_core::json::truthy(Some(p)))
                .map(|p| StampPreview::Source(p.clone())),
            intrinsic_size: present("intrinsicSize").and_then(|s| {
                Some((s.get("width").and_then(number)?, s.get("height").and_then(number)?))
            }),
        }
    }
}

impl Core {
    fn console_error(&self) -> Rc<dyn Fn(&str, Value)> {
        let me = self.weak();
        Rc::new(move |message, error| {
            if let Some(core) = me.upgrade() {
                core.log_error(message, error);
            }
        })
    }

    /// `console.error('[annotation] …', error)`: the host's console.
    pub(crate) fn log_error(&self, message: &str, error: Value) {
        crate::console::error(message, error);
    }

    /// `previewOf(input, bytes, mimeType)`: what a stamp source looks like before the engine draws it.
    fn preview_of(
        &self,
        preview: Option<&StampPreview>,
        bytes: &[u8],
        mime: &str,
    ) -> Promise<Option<StampPreviewProvider>> {
        match preview {
            Some(StampPreview::Provider(p)) => {
                Promise::resolved(Some(cached_preview(p.clone(), self.console_error())))
            }
            Some(StampPreview::Source(source)) => {
                let resolved = resolve_binary_source(source);
                Promise::spawn(async move {
                    let resolved = resolved.await?;
                    Ok(Some(fixed_preview(resolved.bytes, resolved.mime_type)))
                })
            }
            None if mime == "application/pdf" => Promise::resolved(None),
            None => Promise::resolved(Some(fixed_preview(bytes.to_vec(), Some(mime.to_string())))),
        }
    }

    /// What a stamp looks like before the engine's picture of it exists; none once it does.
    pub(crate) fn look_of(&self, record: &ModelAnnotation) -> Option<StampPreviewProvider> {
        if record.unconfirmed == Some(true) {
            return self.stamps.looks.borrow().get(&record.id).cloned();
        }
        self.stamps.looks.borrow_mut().remove(&record.id);
        None
    }

    fn arm_changed(&self) {
        self.update_state(|s| set_ghost_at(s, None));
        self.cx.notify();
    }

    /// `stamps.arm(stamp, options)`.
    pub fn stamps_arm(&self, input: StampInput, signal: Option<AbortSignal>) -> Promise<()> {
        let resolved = self.cancellable(signal, resolve_binary_source(&input.source));
        let me = self.weak();
        Promise::spawn(async move {
            let resolved = resolved.await?;
            let meta = sniff_binary_metadata(&resolved.bytes).ok_or_else(not_a_picture)?;
            let Some(core) = me.upgrade() else { return Ok(()) };
            let preview =
                core.preview_of(input.preview.as_ref(), &resolved.bytes, meta.mime_type).await?;
            let (width, height) =
                desired_stamp_size(&meta, input.target_width, input.intrinsic_size);
            *core.stamps.armed.borrow_mut() = Some(Rc::new(ArmedStamp {
                source: input.source.clone(),
                width,
                height,
                preview,
                name: input.name.clone(),
                subject: input.subject.clone(),
            }));
            let mut info = Map::new();
            info.insert("width".into(), num(width));
            info.insert("height".into(), num(height));
            if let Some(n) = &input.name {
                info.insert("name".into(), n.clone());
            }
            if let Some(s) = &input.subject {
                info.insert("subject".into(), s.clone());
            }
            *core.stamps.armed_info.borrow_mut() = Some(Value::Object(info));
            core.arm_changed();
            if let Ok(Some(interaction)) = core.cx.try_get(&INTERACTION) {
                interaction.activate_tool(ARMED_STAMP_TOOL_ID)?;
            }
            Ok(())
        })
    }

    /// `stamps.disarm()`.
    pub fn stamps_disarm(&self) {
        if self.stamps.armed.borrow().is_none() {
            return;
        }
        *self.stamps.armed.borrow_mut() = None;
        *self.stamps.armed_info.borrow_mut() = None;
        self.arm_changed();
    }

    /// `stamps.isArmed()`.
    pub fn stamps_is_armed(&self) -> bool {
        self.stamps.armed.borrow().is_some()
    }

    /// `createStampAt(...)`: the one engine write both paths funnel through.
    #[allow(clippy::too_many_arguments)]
    fn create_stamp_at(
        &self,
        page: f64,
        point: Point,
        source: &Value,
        desired: (f64, f64),
        look: Option<StampPreviewProvider>,
        rot_cw: f64,
        name: Option<Value>,
        subject: Option<Value>,
        select: bool,
    ) -> Option<Promise<Applied>> {
        self.doc()?;
        let size = self.size_of(page)?;
        let rect = fit_stamp_box(
            point,
            Size { width: desired.0, height: desired.1 },
            Size { width: size.0, height: size.1 },
            rot_cw,
        );
        let mut draft = Object::new();
        draft.insert("subtype".into(), json!("stamp"));
        draft.insert("box".into(), rect.json());
        draft.insert(
            "rotation".into(),
            if rot_cw != 0.0 && !rot_cw.is_nan() { num(rot_cw) } else { Value::Null },
        );
        draft.insert("fit".into(), json!("contain"));
        if let Some(n) = name {
            draft.insert("name".into(), n);
        }
        if let Some(s) = subject {
            draft.insert("subject".into(), s);
        }
        let placed = self.apply_when_numbered(
            vec![StoreChange::Create {
                page: to_page_ref(page),
                draft,
                resources: Some(json!({ "appearance": bytes_of(source) })),
            }],
            select,
        );
        if let Some(look) = look {
            let me = self.weak();
            let placed = placed.clone();
            epdf_kernel::rt::spawn(async move {
                if let Ok(applied) = placed.await
                    && let Some(id) = applied.ids.first()
                    && let Some(core) = me.upgrade()
                {
                    core.stamps.looks.borrow_mut().insert(id.clone(), look);
                    core.cx.notify();
                }
            });
        }
        Some(placed)
    }

    /// The click path's fire-and-forget wrapper.
    #[allow(clippy::too_many_arguments)]
    fn stage_stamp_at(
        &self,
        page: f64,
        point: Point,
        source: &Value,
        desired: (f64, f64),
        look: Option<StampPreviewProvider>,
        rot_cw: f64,
        name: Option<Value>,
        subject: Option<Value>,
    ) -> bool {
        let Some(placed) =
            self.create_stamp_at(page, point, source, desired, look, rot_cw, name, subject, false)
        else {
            return false;
        };
        let tool_id = self.active_tool().map(|t| t.id.clone());
        let me = self.weak();
        epdf_kernel::rt::spawn(async move {
            let outcome = async {
                let applied = placed.await?;
                if let Some(core) = me.upgrade() {
                    core.after_create_placed(tool_id.as_deref(), &applied.ids);
                }
                applied_annotation_of(&applied, 0).await
            }
            .await;
            if let (Err(error), Some(core)) = (outcome, me.upgrade()) {
                core.log_error("[annotation] stamp placement failed:", error.to_canonical());
            }
        });
        true
    }

    /// `stamps.place(stamp, placement, options)`: placement from code.
    pub fn stamps_place(
        &self,
        input: StampInput,
        placement: &Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.stamps.place") {
            return Promise::rejected(e);
        }
        let page = match self.page_ref_of(placement.get("page").unwrap_or(&Value::Null)) {
            Ok(p) => p,
            Err(e) => return Promise::rejected(e),
        };
        let resolved = self.cancellable(signal.clone(), resolve_binary_source(&input.source));
        let me = self.weak();
        let placement = placement.clone();
        Promise::spawn(async move {
            let resolved = resolved.await?;
            let meta = sniff_binary_metadata(&resolved.bytes).ok_or_else(not_a_picture)?;
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            let look = core
                .cancellable(
                    signal.clone(),
                    core.preview_of(input.preview.as_ref(), &resolved.bytes, meta.mime_type),
                )
                .await?;
            let number = crate::js::page_object_number(&page);
            let center = placement
                .get("center")
                .and_then(epdf_annotation_core::json::point)
                .unwrap_or_default();
            let target = placement.get("targetWidth").and_then(number_of).or(input.target_width);
            let rotation = placement.get("rotation").and_then(number_of).unwrap_or(0.0);
            let select = placement.get("select").and_then(Value::as_bool).unwrap_or(false);
            let placed = core.create_stamp_at(
                number,
                center,
                &input.source,
                desired_stamp_size(&meta, target, input.intrinsic_size),
                look,
                rotation,
                input.name.clone(),
                input.subject.clone(),
                select,
            );
            let Some(placed) = placed else {
                return Err(plugin_error(
                    code::NOT_READY,
                    CAPABILITY,
                    format!(
                        "page {} isn't laid out yet",
                        epdf_annotation_core::js::number_to_string(number)
                    ),
                    None,
                    None,
                ));
            };
            let annotated = Promise::spawn(async move {
                let applied = placed.await?;
                applied_annotation_of(&applied, 0).await
            });
            let annotation = core.cancellable(signal, annotated);
            drop(core);
            match annotation.await? {
                Some(annotation) => Ok(json!({ "annotation": annotation })),
                None => Err(plugin_error(
                    code::OPERATION_FAILED,
                    CAPABILITY,
                    "the stamp could not be placed",
                    None,
                    None,
                )),
            }
        })
    }

    /// `placeArmedStamp(page, point, displayRotation)`.
    pub fn place_armed_stamp(
        &self,
        page: f64,
        point: Point,
        display_rotation: Option<f64>,
    ) -> bool {
        let Some(payload) = self.stamps.armed.borrow().clone() else { return false };
        self.stage_stamp_at(
            page,
            point,
            &payload.source,
            (payload.width, payload.height),
            payload.preview.clone(),
            self.upright_rot_for(display_rotation),
            payload.name.clone(),
            payload.subject.clone(),
        )
    }

    /// `placeStampSource(page, point, source, rotCW, targetWidth)`.
    fn place_stamp_source(&self, page: f64, point: Point, source: Value, rot_cw: f64) {
        let resolved = resolve_binary_source(&source);
        let me = self.weak();
        epdf_kernel::rt::spawn(async move {
            let Ok(resolved) = resolved.await else { return };
            let Some(core) = me.upgrade() else { return };
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                core.log_error("[annotation]", json!(not_a_picture().message));
                return;
            };
            let Ok(look) = core.preview_of(None, &resolved.bytes, meta.mime_type).await else {
                return;
            };
            core.stage_stamp_at(
                page,
                point,
                &source,
                desired_stamp_size(&meta, None, None),
                look,
                rot_cw,
                None,
                None,
            );
        });
    }

    /// `requestStampAt(page, point, displayRotation)`: click-to-place by the
    /// active tool's source spec.
    pub fn request_stamp_at(&self, page: f64, point: Point, display_rotation: Option<f64>) -> bool {
        let Some(tool) = self.active_tool() else { return false };
        let Some(spec) = tool.source.clone() else { return false };
        let rot_cw = self.upright_rot_for(if tool.upright { display_rotation } else { None });
        if spec.get("kind").and_then(Value::as_str) == Some("bytes") {
            self.place_stamp_source(
                page,
                point,
                spec.get("source").cloned().unwrap_or(Value::Null),
                rot_cw,
            );
            return true;
        }
        let me = self.weak();
        self.prompt_at(
            &tool,
            page,
            point,
            Rc::new(move |picked: Value| {
                if let Some(core) = me.upgrade() {
                    core.place_stamp_source(
                        page,
                        point,
                        picked.get("data").cloned().unwrap_or(Value::Null),
                        rot_cw,
                    );
                }
            }),
        )
    }

    /// `getArmedStamp()`: a new object on every arm, `null` when nothing is armed.
    pub fn get_armed_stamp(&self) -> Value {
        self.stamps.armed_info.borrow().clone().unwrap_or(Value::Null)
    }

    /// `renderArmedStampPreview(devicePixelWidth)`.
    pub fn render_armed_stamp_preview(&self, width: Option<f64>) -> Promise<Option<Value>> {
        let armed = self.stamps.armed.borrow().clone();
        match armed.and_then(|a| a.preview.clone()) {
            Some(preview) => preview(width.unwrap_or(0.0)),
            None => Promise::resolved(None),
        }
    }
}

fn number_of(value: &Value) -> Option<f64> {
    if epdf_kernel::is_undefined(value) { None } else { number(value) }
}
