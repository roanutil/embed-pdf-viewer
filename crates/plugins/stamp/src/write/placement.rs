//! `write/placement.ts`: arm an asset on a document (the next click places)
//! or place it without the pointer. Form-backed stamps are executable
//! templates only until placement: they are evaluated on an isolated copy
//! under the target document's detached script realm, so neither the
//! canonical library nor its reusable page extraction becomes target, user
//! or time specific.

use std::rc::Rc;

use epdf_acrojs::java_script_program_from_action_tree;
use epdf_annotation_core::AnnotationRef;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::to_plugin_error;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::types::DocumentMeta;
use epdf_kernel::{Error, undefined};
use epdf_plugin_actions::{
    ACTIONS, ActionOrigin, ScriptCommitContext, ScriptRealmKind, ScriptRealmTarget,
};
use epdf_plugin_annotation::{ANNOTATION, StampPlacement, StampPreview, StampToolInput};
use epdf_plugin_form::services::scripting::commit_surface;
use epdf_plugin_form::{
    FormCommitStatus, FormScriptingController, FormScriptingControllerOptions, TransactionBody,
    TransactionPort,
};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::promise::{adopt, await_engine};
use crate::services::asset_engine::{
    bytes_of, call, close, image_to_preview, page_call, render_options,
};
use crate::services::binaries::AssetBinary;
use crate::value::{get, num, str_of};
use crate::write::libraries::pon_of_ref;

/// A JavaScript value in a template literal.
fn js_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            epdf_geometry::js::number_to_string(n.as_f64().unwrap_or(f64::NAN))
        }
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".to_string(),
        _ => "undefined".to_string(),
    }
}

/// The asset document's name-tree boot programs, read lazily once per realm.
fn boot_sources(
    doc: Rc<dyn DocumentHandle>,
) -> Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>> {
    Rc::new(move || {
        let read = doc.has("actions").then(|| doc.call("actions.read", None, Vec::new()));
        Ok(Promise::spawn(async move {
            let Some(read) = read else { return Ok(Vec::new()) };
            let tree = await_engine(read).await?;
            if tree.is_null() {
                return Ok(Vec::new());
            }
            get(&tree, "nameTreeScripts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|entry| {
                    java_script_program_from_action_tree(
                        get(entry, "action").unwrap_or(&Value::Null),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        }))
    })
}

/// What placement arms: the template, or a materialized page.
struct Placement {
    binary: AssetBinary,
    materialized: bool,
}

impl StampCapability {
    /// `stampPayload(asset, placement, materialized)`: the payload both entry
    /// points hand the annotation plugin.
    fn stamp_payload(&self, asset: &StampAsset, placement: &Placement) -> StampToolInput {
        let cache_key = (!placement.materialized).then_some(asset.id.as_str());
        StampToolInput {
            source: placement.binary.bytes.clone().map_or_else(undefined, |b| Bytes(b).to_json()),
            target_width: None,
            name: Some(asset.name.clone()),
            subject: Some(asset.subject.clone().unwrap_or_else(|| asset.label.clone())),
            preview: Some(StampPreview::Provider(self.ghost_provider(
                placement.binary.bytes.clone(),
                placement.binary.preview.clone(),
                cache_key,
            ))),
            intrinsic_size: Some(asset.size),
        }
    }

    /// `materializeForPlacement(documentId, asset, bin)`.
    async fn materialize_for_placement(
        &self,
        document_id: &str,
        asset: &StampAsset,
        bin: AssetBinary,
    ) -> Result<Placement, Error> {
        let template = || Placement { binary: bin.clone(), materialized: false };
        if self.0.config.dynamic == Some(false) {
            return Ok(template());
        }
        let actions = self.cx().try_for_document(&ACTIONS, document_id).ok().flatten();
        let Some(actions) = actions.filter(|a| a.is_scripting_enabled()) else {
            return Ok(template());
        };
        let Some(target_meta) = self.cx().core().documents.get(document_id).cloned() else {
            return Err(Error::engine(
                "NotFound",
                format!("[stamp] target document '{document_id}' is not open"),
            ));
        };
        // A page extraction does not keep catalog-owned AcroForm and action
        // structures: canonical assets evaluate from the whole library PDF.
        let canonical = self.0.library_binaries.borrow().get(&asset.library_id).cloned().flatten();
        let source = canonical.clone().or_else(|| bin.bytes.clone()).unwrap_or_default();
        let doc = rt::returned(self.open_asset_document(source)).await?;
        let mut realm = None;
        let mut scripting: Option<FormScriptingController> = None;
        let body = async {
            let layout = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let pages = get(&layout, "pages").and_then(Value::as_array).cloned().unwrap_or_default();
            let wanted = asset.page.page_object_number;
            let selected = pages.iter().find(|p| get(p, "ref").and_then(pon_of_ref) == Some(wanted)).cloned();
            let Some(selected) = selected else {
                let message = if canonical.is_some() {
                    format!("[stamp] canonical page {wanted} no longer exists")
                } else {
                    "[stamp] a loose dynamic stamp asset must contain exactly one page".to_string()
                };
                return Err(Error::engine("InvalidArg", message));
            };
            let selected_pon = get(&selected, "ref").and_then(|r| num(r, "pageObjectNumber"));
            let snapshot = await_engine(call(&doc, "forms.list", Vec::new())).await?;
            let fields = get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let has_selected_page_field = fields.iter().any(|field| {
                get(field, "widgets").and_then(Value::as_array).is_some_and(|widgets| {
                    widgets.iter().any(|w| {
                        get(w, "page").filter(|p| !p.is_null()).and_then(|p| num(p, "pageObjectNumber"))
                            == selected_pon
                    })
                })
            });
            if !has_selected_page_field {
                return Ok(template());
            }
            if !doc.has("pages.flatten") || !doc.has("pages.extract") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] dynamic PDF stamps need an asset engine with pages.flatten and pages.extract",
                ));
            }
            // Scripts observe the target document's metadata and boot from the
            // asset document's own name tree.
            let meta = target_meta.clone();
            let document: Rc<dyn Fn() -> Option<DocumentMeta>> = Rc::new(move || Some(meta.clone()));
            let minted = actions
                .create_detached_script_realm(ScriptRealmTarget {
                    doc: doc.clone(),
                    document: document.clone(),
                    boot_sources: boot_sources(doc.clone()),
                })
                .ok_or_else(|| Error::new("[stamp] scripting is off"))?;
            realm = Some(minted.clone());
            let port = minted.clone();
            let transaction: TransactionPort = Rc::new(move |body: TransactionBody| port.transaction(body));
            let controller = FormScriptingController::new(FormScriptingControllerOptions {
                doc: doc.clone(),
                document,
                transaction,
                budget: Some(minted.budget().clone()),
            });
            scripting = Some(controller.clone());
            let result = controller.recalculate().await?;
            actions.surface_script_commit(
                &commit_surface(&result),
                ScriptCommitContext { origin: ActionOrigin::User, realm: ScriptRealmKind::Detached },
            )?;
            if result.status == FormCommitStatus::Failed {
                let message = result
                    .error
                    .as_ref()
                    .map_or("native form effect failed".to_string(), |e| e.message.clone());
                return Err(Error::engine(
                    "Unknown",
                    format!("[stamp] dynamic stamp scripting failed: {message}"),
                ));
            }
            let page = get(&selected, "ref").cloned().unwrap_or(Value::Null);
            let flattened =
                await_engine(call(&doc, "pages.flatten", vec![json!([page]), json!("display")])).await?;
            let results = get(&flattened, "results").and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(failed) =
                results.iter().find(|r| matches!(str_of(r, "status"), Some("failed" | "skipped")))
            {
                let page_of = get(failed, "page").and_then(|p| get(p, "pageObjectNumber"));
                return Err(Error::engine(
                    "Unknown",
                    format!("[stamp] dynamic stamp flatten failed for page {}", js_string(page_of)),
                ));
            }
            let bytes = await_engine(call(&doc, "pages.extract", vec![json!([page])])).await?;
            let pon = pon_of_ref(&page).unwrap_or(0);
            doc.page_index(pon)?;
            let render =
                page_call(&doc, "page.render.image", pon, vec![render_options(self.preview_width(), false)]);
            let image = await_engine(render).await?;
            Ok(Placement {
                binary: AssetBinary { bytes: bytes_of(&bytes), preview: Some(image_to_preview(&image)?) },
                materialized: true,
            })
        }
        .await;
        if let Some(scripting) = &scripting {
            scripting.dispose();
        }
        if let Some(realm) = &realm {
            realm.dispose();
        }
        close(&doc, body).await
    }

    fn asset_and_binary(&self, asset_id: &str) -> Result<(StampAsset, AssetBinary), Error> {
        let asset = self.state().assets.get(asset_id).cloned();
        let bin = self.0.binaries.borrow().get(asset_id).cloned();
        match (asset, bin) {
            (Some(asset), Some(bin)) => Ok((asset, bin)),
            _ => Err(Error::engine("NotFound", format!("[stamp] unknown asset '{asset_id}'"))),
        }
    }

    /// `armAsset(documentId, assetId, {targetWidth})`: the next click on that
    /// document places the asset, through its annotation plugin.
    pub fn arm_asset(
        &self,
        document_id: &str,
        asset_id: &str,
        target_width: Option<f64>,
    ) -> Promise<()> {
        let this = self.clone();
        let (document_id, asset_id) = (document_id.to_string(), asset_id.to_string());
        Promise::spawn(async move {
            let (asset, bin) = this.asset_and_binary(&asset_id)?;
            let placement =
                rt::returned(this.materialize_for_placement(&document_id, &asset, bin)).await?;
            let annotation = this.cx().for_document(&ANNOTATION, &document_id)?;
            let input = StampToolInput { target_width, ..this.stamp_payload(&asset, &placement) };
            annotation.arm_stamp(input).await?;
            this.0.armed_by_document.borrow_mut().insert(document_id.clone(), asset_id.clone());
            this.0
                .events
                .arm_changed
                .emit(&StampArmChangedEvent { document_id, asset_id: Some(asset_id) });
            Ok(())
        })
    }

    /// `getArmedAsset(documentId)`: what this plugin armed, while the
    /// annotation plugin still holds a stamp.
    pub fn get_armed_asset(&self, document_id: &str) -> Option<StampAsset> {
        let asset_id = self.0.armed_by_document.borrow().get(document_id).cloned()?;
        let annotation = self.cx().try_for_document(&ANNOTATION, document_id).ok().flatten()?;
        if !annotation.has_armed_stamp() {
            return None;
        }
        self.state().assets.get(&asset_id).cloned()
    }

    /// `placeAsset(documentId, assetId, placement)`: place without the
    /// pointer; answers the placed annotation's ref.
    pub fn place_asset(
        &self,
        document_id: &str,
        asset_id: &str,
        placement: StampPlacement,
    ) -> Promise<AnnotationRef> {
        let this = self.clone();
        let (document_id, asset_id) = (document_id.to_string(), asset_id.to_string());
        Promise::spawn(async move {
            let (asset, bin) = this.asset_and_binary(&asset_id)?;
            let materialized =
                rt::returned(this.materialize_for_placement(&document_id, &asset, bin)).await?;
            let annotation = this.cx().for_document(&ANNOTATION, &document_id)?;
            adopt(annotation.place_stamp(this.stamp_payload(&asset, &materialized), placement))
                .await
        })
    }

    /// `placeAssetOnPages(documentId, assetId, pages, placement)`: one asset
    /// at the same page-relative placement on every page (`None`) or the
    /// ones listed; each failure is reported by its page.
    pub fn place_asset_on_pages(
        &self,
        document_id: &str,
        asset_id: &str,
        pages: Option<Vec<PageRef>>,
        at: epdf_geometry::Point,
        target_width: Option<f64>,
        rotation: Option<f64>,
    ) -> Promise<BatchResult> {
        let this = self.clone();
        let (document_id, asset_id) = (document_id.to_string(), asset_id.to_string());
        Promise::spawn(async move {
            let targets: Vec<PageRef> = match pages {
                Some(pages) => pages,
                None => this
                    .cx()
                    .core()
                    .documents
                    .get(&document_id)
                    .and_then(|d| d.pages.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| get(p, "ref").and_then(PageRef::from_json))
                    .collect(),
            };
            let mut out = BatchResult::default();
            for page in targets {
                let placement = StampPlacement {
                    page: epdf_annotation_core::PageRef::new(page.page_object_number),
                    at,
                    target_width,
                    rotation,
                };
                match this.place_asset(&document_id, &asset_id, placement).await {
                    Ok(placed) => out.applied.push(epdf_kernel::js::to_value(&placed)),
                    Err(error) => out.failed.push(BatchFailure {
                        page,
                        error: to_plugin_error("stamp", &error).plugin_error_info(),
                    }),
                }
            }
            Ok(out)
        })
    }

    /// `disarm(documentId)`: announces a change only when something was
    /// armed. `Err` is the kernel's missing-capability throw.
    pub fn disarm(&self, document_id: &str) -> Result<(), Error> {
        let was = self.0.armed_by_document.borrow_mut().remove(document_id);
        self.cx().for_document(&ANNOTATION, document_id)?.disarm_stamp();
        if was.is_some() {
            self.0.events.arm_changed.emit(&StampArmChangedEvent {
                document_id: document_id.to_string(),
                asset_id: None,
            });
        }
        Ok(())
    }
}
