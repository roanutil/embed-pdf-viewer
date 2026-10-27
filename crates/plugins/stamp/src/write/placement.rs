//! `write/placement.ts`: arm an asset on a document (the next click
//! places) or place it without the pointer. Form-backed stamps are
//! executable templates only until placement: they are evaluated on an
//! isolated copy under the target document's detached script realm, so
//! neither the canonical library nor its reusable per-page extraction
//! becomes target, user or time specific.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::java_script_program_from_action_tree;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::{DocStatus, DocumentMeta};
use epdf_kernel::{Error, undefined};
use epdf_plugin_actions::{
    ACTIONS, ActionOrigin, ActionsCapability, DetachedScriptRealm, PhasedUiEffect,
    ScriptCommitContext, ScriptCommitSurface, ScriptDocument, ScriptRealmKind, ScriptRealmTarget,
};
use epdf_plugin_annotation::write::stamps::{StampInput, StampPreview};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_form::engine::form_effect_ops;
use epdf_plugin_form::scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, ScriptStatus, TransactionBody,
    TransactionPort,
};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::js::{adopt, array, member, nullish, page_ref};
use crate::model::{arm_asset, disarm_documents};
use crate::services::asset_engine::{call, close, image_to_preview, page_call, render_options};
use crate::services::binaries::AssetBinary;
use crate::services::errors::{
    CAPABILITY, not_found, permission_denied, stamp_error, throw_if_cancelled, verb,
};
use crate::write::libraries::bytes_json;

/// What a placement places: the asset's own bytes and preview (the
/// template), or an evaluated copy.
#[derive(Clone)]
enum Placement {
    Template(AssetBinary),
    Materialized(AssetBinary),
}

impl Placement {
    fn binary(&self) -> &AssetBinary {
        match self {
            Placement::Template(binary) | Placement::Materialized(binary) => binary,
        }
    }
}

impl StampCapability {
    /// The annotation plugin owns the arm itself and drops it on its own (a
    /// tool switch, a closed document). Forget those arms here too, so
    /// `getArmedAsset` and `onArmChanged` follow: `ctx.watch(droppedArms,
    /// ...)`, over the ids joined by newlines, which `Object.is` compares as
    /// strings.
    pub(crate) fn watch_dropped_arms(&self) -> Result<(), Error> {
        let reader = self.clone();
        let dropped_arms = move || -> String {
            reader
                .state()
                .armed
                .keys()
                .filter(|id| !reader.annotation_is_armed(id))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };
        let updater = self.clone();
        self.cx().watch(
            move || Ok(dropped_arms()),
            move |dropped: &String, _| {
                if !dropped.is_empty() {
                    let ids: Vec<String> = dropped.split('\n').map(str::to_string).collect();
                    updater.update_state(|s| disarm_documents(s, &ids));
                }
                Ok(())
            },
            |a, b| a == b,
        )?;
        Ok(())
    }

    /// `ctx.tryForDocument(AnnotationHostToken, id)?.stamps.isArmed()`.
    fn annotation_is_armed(&self, document_id: &str) -> bool {
        matches!(
            self.cx().try_for_document(&ANNOTATION, document_id),
            Ok(Some(annotation)) if annotation.stamps_is_armed()
        )
    }

    /// `stampPayload(asset, placement, materialized)`: the one payload both
    /// entry points hand the annotation plugin: artwork, a resolution-aware
    /// ghost, true size, and the identity a placement writes (`/Name` the
    /// identifier, `/Subj` the subject override or label).
    fn stamp_payload(&self, asset: &StampAsset, placement: &Placement) -> StampInput {
        let binary = placement.binary();
        let cache_key = match placement {
            Placement::Template(_) => Some(asset.id.as_str()),
            Placement::Materialized(_) => None,
        };
        StampInput {
            source: binary.bytes.clone().map_or_else(undefined, bytes_json),
            target_width: None,
            name: Some(json!(asset.name)),
            subject: Some(json!(asset.subject.clone().unwrap_or_else(|| asset.label.clone()))),
            preview: Some(StampPreview::Provider(self.annotation_ghost(
                binary.bytes.clone(),
                binary.preview.clone(),
                cache_key,
            ))),
            intrinsic_size: Some((asset.size.width, asset.size.height)),
        }
    }

    /// `materializeForPlacement(documentId, asset, binary)`. The realm comes
    /// from the target document's actions plugin: a detached realm under
    /// that document's policy and environment, with its own sandbox and none
    /// of the viewer realm's globals. No actions plugin, scripting off, or
    /// `dynamic: false`: the template is armed as it is.
    fn materialize_for_placement(
        &self,
        document_id: &str,
        asset: &StampAsset,
        binary: &AssetBinary,
    ) -> Promise<Placement> {
        let this = self.clone();
        let (document_id, asset, binary) = (document_id.to_string(), asset.clone(), binary.clone());
        Promise::spawn(async move {
            if !this.get_settings().dynamic {
                return Ok(Placement::Template(binary));
            }
            let actions = this.cx().try_for_document(&ACTIONS, &document_id).ok().flatten();
            let Some(actions) = actions.filter(ActionsCapability::is_scripting_enabled) else {
                return Ok(Placement::Template(binary));
            };
            let documents = this.documents()?;
            let ready = documents.get(&document_id).is_some_and(|d| d.status == DocStatus::Ready);
            let meta = this.cx().core().documents.get(&document_id).cloned();
            let (true, Some(meta)) = (ready, meta) else {
                return Err(stamp_error(
                    code::NOT_FOUND,
                    format!("document '{document_id}' is not open"),
                ));
            };
            // What the scripts read of the document they stamp: its name,
            // page count and pages, as they were when the arm began.
            let target_document = Rc::new(DocumentMeta {
                pages: Rc::new(Value::Array(documents.list_pages(Some(&document_id)))),
                ..(*meta).clone()
            });

            // A page extraction does not retain catalog-owned AcroForm and
            // action structures. Canonical assets evaluate from the whole
            // library PDF, then extract only their selected page after
            // flatten.
            let canonical =
                this.0.binaries.library_binaries.borrow().get(&asset.library_id).cloned();
            let has_canonical = canonical.as_ref().is_some_and(Option::is_some);
            let source = match canonical.flatten() {
                Some(bytes) => bytes,
                None => binary.bytes.clone().unwrap_or_default(),
            };
            let doc = this.open_asset_document(source).await?;
            // Cleanup protection starts the moment the temporary document
            // exists: realm and controller failures must not leak it.
            let realm: Rc<RefCell<Option<DetachedScriptRealm>>> = Rc::default();
            let scripting: Rc<RefCell<Option<FormScriptingController>>> = Rc::default();
            let body = this
                .evaluate(
                    &doc,
                    &actions,
                    &asset,
                    &binary,
                    has_canonical,
                    target_document,
                    &realm,
                    &scripting,
                )
                .await;
            if let Some(scripting) = scripting.borrow_mut().take() {
                scripting.dispose();
            }
            if let Some(realm) = realm.borrow_mut().take() {
                realm.dispose();
            }
            close(&doc, body).await
        })
    }

    /// The body of `materializeForPlacement`'s `try`, on the opened copy.
    #[allow(clippy::too_many_arguments)]
    async fn evaluate(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        actions: &ActionsCapability,
        asset: &StampAsset,
        binary: &AssetBinary,
        has_canonical: bool,
        target_document: Rc<DocumentMeta>,
        realm_slot: &Rc<RefCell<Option<DetachedScriptRealm>>>,
        scripting_slot: &Rc<RefCell<Option<FormScriptingController>>>,
    ) -> Result<Placement, Error> {
        let layout = call(doc, "pages.list", Vec::new()).await?;
        let pages = member(&layout, "pages")?;
        let selected = pages.as_array().and_then(|pages| {
            pages
                .iter()
                .find(|page| {
                    page.get("ref").and_then(|r| r.get("objectNumber")).and_then(Value::as_f64)
                        == Some(f64::from(asset.page.page_object_number))
                })
                .cloned()
        });
        let Some(selected) = selected else {
            return Err(stamp_error(
                code::INVALID_INPUT,
                if has_canonical {
                    format!("canonical page {} no longer exists", asset.page.page_object_number)
                } else {
                    "a loose dynamic stamp asset must contain exactly one page".to_string()
                },
            ));
        };
        let page = selected.get("ref").cloned().unwrap_or_else(undefined);
        let page_number = page.get("objectNumber").and_then(Value::as_f64);
        let snapshot = call(doc, "forms.list", Vec::new()).await?;
        let fields = member(&snapshot, "fields")?;
        // `snapshot.fields.some((field) => field.widgets.some(...))`
        let mut has_selected_page_field = false;
        for field in array(&fields, "snapshot.fields", "some")? {
            let widgets = member(field, "widgets")?;
            let on_page = array(&widgets, "field.widgets", "some")?.iter().any(|widget| {
                widget
                    .get("page")
                    .filter(|p| !nullish(p))
                    .and_then(|p| p.get("objectNumber"))
                    .and_then(Value::as_f64)
                    == page_number
            });
            if on_page {
                has_selected_page_field = true;
                break;
            }
        }
        if !has_selected_page_field {
            return Ok(Placement::Template(binary.clone()));
        }
        if !doc.has("pages.flatten") || !doc.has("pages.extract") {
            return Err(stamp_error(
                code::UNSUPPORTED,
                "dynamic PDF stamps need an asset engine with pages.flatten and pages.extract",
            ));
        }

        // Scripts observe the target document's metadata (Acrobat's dynamic
        // stamp contract: `documentFileName` is the document being stamped)
        // and boot from the asset document's own name tree.
        let observed = target_document.clone();
        let booting = doc.clone();
        let target = ScriptRealmTarget {
            doc: doc.clone(),
            document: Rc::new(move || Some(ScriptDocument::from((*observed).clone()))),
            boot_sources: Rc::new(move || {
                // `doc.actions` is a required service of the engine
                // contract, so `doc.actions ? ... : null` always reads it.
                let read = booting.call("actions.get", None, Vec::new());
                Ok(Promise::spawn(async move {
                    let tree = read.await?;
                    if nullish(&tree) {
                        return Ok(Vec::new());
                    }
                    // `tree?.nameTreeScripts.map(...)`
                    let scripts = member(&tree, "nameTreeScripts")?;
                    array(&scripts, "tree.nameTreeScripts", "map")?
                        .iter()
                        .map(|entry| {
                            java_script_program_from_action_tree(
                                entry.get("action").unwrap_or(&Value::Null),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                }))
            }),
        };
        let Some(realm) = actions.create_detached_script_realm(target) else {
            return Ok(Placement::Template(binary.clone()));
        };
        *realm_slot.borrow_mut() = Some(realm.clone());
        let port = realm.clone();
        let transaction: TransactionPort =
            Rc::new(move |body: TransactionBody| port.transaction::<Rc<dyn Any>>(body));
        let document = target_document.clone();
        let scripting = FormScriptingController::new(FormScriptingControllerOptions {
            document: Rc::new(move || Some((*document).clone())),
            transaction,
            budget: Some(realm.budget().clone()),
        });
        *scripting_slot.borrow_mut() = Some(scripting.clone());
        // The copy is the stamp's own: what its scripts calculate is written
        // to it, in the same transaction, before it is flattened.
        let form = Rc::new(snapshot);
        let writer = doc.clone();
        let result = scripting
            .transaction(move |scripts| {
                Ok(Promise::spawn(async move {
                    let calculated = scripts.recalculate(form).await?;
                    if calculated.status == ScriptStatus::Accepted && !calculated.effects.is_empty()
                    {
                        let ops = form_effect_ops(&calculated.effects);
                        writer.call("apply", None, vec![json!({ "ops": ops })]).await?;
                    }
                    Ok(calculated)
                }))
            })
            .await?;
        actions.surface_script_commit(
            &ScriptCommitSurface {
                ui_effects: result
                    .ui_effects
                    .iter()
                    .map(|e| PhasedUiEffect { effect: e.effect.clone(), phase: e.phase })
                    .collect(),
                diagnostics: result.diagnostics.clone(),
                error: result.error.clone(),
            },
            &ScriptCommitContext {
                origin: ActionOrigin::User,
                realm: ScriptRealmKind::Detached,
                source: None,
            },
        )?;
        if result.status == ScriptStatus::Failed {
            let why = result
                .error
                .as_ref()
                .map_or_else(|| "native form effect failed".to_string(), |e| e.message.clone());
            return Err(stamp_error(
                code::OPERATION_FAILED,
                format!("dynamic stamp scripting failed: {why}"),
            ));
        }

        // All or nothing: a page that fails to flatten throws.
        call(doc, "pages.flatten", vec![json!([page]), json!({ "usage": "display" })]).await?;
        let bytes = crate::services::asset_engine::bytes_of(
            &call(doc, "pages.extract", vec![json!([page])]).await?,
        );
        let pon = page_ref(&page)?.page_object_number;
        let image = page_call(
            doc,
            "page.render.image",
            pon,
            vec![render_options(self.get_settings().preview_width, false)],
        )
        .await?;
        Ok(Placement::Materialized(AssetBinary { bytes, preview: Some(image_to_preview(&image)?) }))
    }

    /// `assetOf(assetId)`: an asset and its bytes, or `not-found`.
    fn asset_of(&self, asset_id: &str) -> Result<(StampAsset, AssetBinary), Error> {
        let asset = self.state().assets.get(asset_id).cloned();
        let binary = self.0.binaries.binaries.borrow().get(asset_id).cloned();
        match (asset, binary) {
            (Some(asset), Some(binary)) => Ok((asset, binary)),
            _ => Err(not_found("asset", asset_id)),
        }
    }

    /// `assertMayPlace(documentId, operation)`: placing is annotation work,
    /// refused where the document's annotation plugin refuses a create.
    fn assert_may_place(&self, document_id: &str, operation: &str) -> Result<(), Error> {
        if self.can_place(Some(document_id)) {
            Ok(())
        } else {
            Err(permission_denied("annotations:create", operation))
        }
    }

    /// `ctx.forDocument(AnnotationToken, documentId)`.
    fn annotation_of(&self, document_id: &str) -> Result<AnnotationCapability, Error> {
        self.cx().for_document(&ANNOTATION, document_id)
    }

    /// `armAsset(assetId, options)`: the next click on the document places
    /// it, with a preview under the pointer. Rejects `not-found` for an
    /// unknown asset, `permission-denied` without `annotations:create`.
    pub fn arm_asset(&self, asset_id: &str, options: StampArmOptions) -> Promise<()> {
        let this = self.clone();
        let asset_id = asset_id.to_string();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let document_id = this.target_of(options.document_id.as_deref())?;
            let (asset, binary) = this.asset_of(&asset_id)?;
            this.assert_may_place(&document_id, "stamp.armAsset")?;
            let placement = cancellable(
                CAPABILITY,
                signal.clone(),
                this.materialize_for_placement(&document_id, &asset, &binary),
            )
            .await?;
            throw_if_cancelled(signal.as_ref())?;
            // The placement itself remains one `stamps.arm` call: dynamic
            // evaluation has already produced an ephemeral static page and
            // matching preview at this boundary.
            let annotation = this.annotation_of(&document_id)?;
            let mut payload = this.stamp_payload(&asset, &placement);
            payload.target_width = options.target_width;
            annotation.stamps_arm(payload, signal).await?;
            this.update_state(|s| arm_asset(s, &document_id, &asset_id));
            Ok(())
        }))
    }

    /// `getArmedAsset(documentId)`: the stamp the next click on the document
    /// places. An arm the annotation plugin just dropped reads as nothing
    /// armed, even before the watch forgets it.
    pub fn get_armed_asset(&self, document_id: Option<&str>) -> Option<StampAsset> {
        let id = self.document_id_of(document_id)?;
        let state = self.state();
        let asset_id = state.armed.get(&id)?;
        if !self.annotation_is_armed(&id) {
            return None;
        }
        state.assets.get(asset_id).cloned()
    }

    /// `disarm(documentId)`: disarm the stamp without placing it.
    pub fn disarm(&self, document_id: Option<&str>) {
        let Some(id) = self.document_id_of(document_id) else { return };
        if let Ok(Some(annotation)) = self.cx().try_for_document(&ANNOTATION, &id) {
            annotation.stamps_disarm();
        }
        self.update_state(|s| disarm_documents(s, &[id]));
    }

    /// `placeOn(documentId, assetId, placement, signal)`: place on one
    /// document, its permission already checked.
    fn place_on(
        &self,
        document_id: &str,
        asset_id: &str,
        placement: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let this = self.clone();
        let (document_id, asset_id) = (document_id.to_string(), asset_id.to_string());
        Promise::spawn(async move {
            let (asset, binary) = this.asset_of(&asset_id)?;
            let materialized = cancellable(
                CAPABILITY,
                signal.clone(),
                this.materialize_for_placement(&document_id, &asset, &binary),
            )
            .await?;
            throw_if_cancelled(signal.as_ref())?;
            let annotation = this.annotation_of(&document_id)?;
            let payload = this.stamp_payload(&asset, &materialized);
            adopt(annotation.stamps_place(payload, &placement, signal)).await
        })
    }

    /// `placeAsset(assetId, placement, options)`: place a stamp without the
    /// pointer, its middle on `center`. Resolves `{ annotation }`.
    pub fn place_asset(
        &self,
        asset_id: &str,
        placement: Value,
        options: StampDocumentOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        let asset_id = asset_id.to_string();
        verb(Promise::spawn(async move {
            throw_if_cancelled(options.signal.as_ref())?;
            let document_id = this.target_of(options.document_id.as_deref())?;
            this.asset_of(&asset_id)?;
            this.assert_may_place(&document_id, "stamp.placeAsset")?;
            adopt(this.place_on(&document_id, &asset_id, placement, options.signal)).await
        }))
    }

    /// `placeAssetOnPages(assetId, pages, placement, options)`: one stamp at
    /// the same spot on several pages, or on all. Every page is resolved
    /// before the first placement; a cancel stops the batch, and what was
    /// placed stays placed.
    pub fn place_asset_on_pages(
        &self,
        asset_id: &str,
        pages: PageSelection,
        placement: Value,
        options: StampDocumentOptions,
    ) -> Promise<BatchResult> {
        let this = self.clone();
        let asset_id = asset_id.to_string();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let document_id = this.target_of(options.document_id.as_deref())?;
            this.asset_of(&asset_id)?;
            this.assert_may_place(&document_id, "stamp.placeAssetOnPages")?;
            let targets: Vec<Value> = match pages {
                PageSelection::All => this
                    .documents()?
                    .list_pages(Some(&document_id))
                    .iter()
                    .map(|page| page.get("ref").cloned().unwrap_or_else(undefined))
                    .collect(),
                PageSelection::Pages(pages) => pages
                    .into_iter()
                    .map(|page| {
                        let layout = this.page_of(&document_id, page)?;
                        Ok(layout.get("ref").cloned().unwrap_or_else(undefined))
                    })
                    .collect::<Result<_, Error>>()?,
            };
            let mut result = BatchResult::default();
            for page in targets {
                let mut at = match &placement {
                    Value::Object(object) => object.clone(),
                    _ => serde_json::Map::new(),
                };
                at.insert("page".into(), page.clone());
                match this
                    .place_on(&document_id, &asset_id, Value::Object(at), signal.clone())
                    .await
                {
                    Ok(placed) => result
                        .applied
                        .push(placed.get("annotation").cloned().unwrap_or_else(undefined)),
                    Err(error) => {
                        let refused = to_plugin_error(CAPABILITY, &error);
                        if refused.plugin_code() == Some(code::OPERATION_CANCELLED) {
                            return Err(refused);
                        }
                        result
                            .failed
                            .push(BatchFailure { page, error: refused.plugin_error_info() });
                    }
                }
            }
            Ok(result)
        }))
    }
}
