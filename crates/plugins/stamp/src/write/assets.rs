//! `write/assets.ts`: asset writes. Every asset is a page of its library's
//! PDF: a single-page PDF is inserted, a raster becomes a page with the
//! image flattened into it. Removal deletes the page; relabel renames the
//! registry entry. All through the per-library mutation queue.

use std::rc::Rc;

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, undefined};
use epdf_plugin_annotation::binary::{WireResource, resolve_binary_source, sniff_binary_metadata};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::convention::*;
use crate::js::{adopt, member, page_ref};
use crate::model::{add_asset, remove_asset, set_asset};
use crate::services::asset_engine::{bytes_of, call, close, page_call};
use crate::services::binaries::AssetBinary;
use crate::services::errors::{
    CAPABILITY, not_found, permission_denied, stamp_error, throw_if_cancelled, verb,
};
use crate::write::libraries::bytes_json;

/// JavaScript truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        other => !epdf_kernel::is_undefined(other),
    }
}

/// What an append commits once its document has closed.
#[derive(Clone)]
struct Appended {
    asset: StampAsset,
    bytes: Option<Vec<u8>>,
    preview: StampAssetPreview,
    canonical: Option<Vec<u8>>,
}

/// `createAsset`'s inputs once checked, for the queued mutation.
struct Checked {
    resolved: WireResource,
    is_pdf: bool,
    name: String,
    label: String,
    raster_size: Option<Value>,
    supplied_preview: Option<StampAssetPreview>,
    input: AddAssetInput,
}

impl StampCapability {
    /// `createAsset(input, options)` without the `verb` wrapper: the moves,
    /// copies and stamps from annotations go through it.
    pub(crate) fn create_asset_now(
        &self,
        input: AddAssetInput,
        options: OperationOptions,
    ) -> Promise<StampAsset> {
        let this = self.clone();
        Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            if let Some(id) = input.library_id.as_deref().filter(|id| !id.is_empty())
                && !this.state().libraries.contains_key(id)
            {
                return Err(not_found("library", id));
            }
            let resolved =
                cancellable(CAPABILITY, signal.clone(), this.resolve_asset_source(&input)).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(stamp_error(
                    code::INVALID_INPUT,
                    "asset source must be PNG, JPEG, or single-page PDF bytes",
                ));
            };
            let is_pdf = meta.mime_type == "application/pdf";
            let name = match &input.name {
                Some(name) => name.clone(),
                None => custom_stamp_name(&crate::console::random),
            };
            let label = input.label.clone().unwrap_or_else(|| name.clone());
            if name.is_empty() || name.contains('=') {
                return Err(stamp_error(
                    code::INVALID_INPUT,
                    format!(
                        "invalid stamp identifier '{name}': must be non-empty and contain no '='"
                    ),
                ));
            }
            let raster_size: Option<(f64, f64, Value)> = match input.size {
                Some(size) => Some((size.width, size.height, json!(size))),
                None => meta.size.map(|(w, h)| (w, h, json!({ "width": w, "height": h }))),
            };
            if !is_pdf && raster_size.as_ref().is_none_or(|(w, h, _)| *w <= 0.0 || *h <= 0.0) {
                return Err(stamp_error(
                    code::INVALID_INPUT,
                    "a raster asset needs a positive `size` (its page size in points)",
                ));
            }
            let mut supplied_preview = None;
            if let Some(preview) = input.preview.as_ref().filter(|p| truthy(p)) {
                let preview = resolve_binary_source(preview).await?;
                supplied_preview = Some(StampAssetPreview {
                    bytes: Some(bytes_json(preview.bytes)),
                    mime_type: Some(Value::String(
                        preview.mime_type.unwrap_or_else(|| "image/png".to_string()),
                    )),
                });
            }
            // No library named: one of its own, named after the label.
            let library_id = match &input.library_id {
                Some(id) => id.clone(),
                None => {
                    let options =
                        CreateLibraryOptions { signal: signal.clone(), ..Default::default() };
                    this.create_library_now(&label, options).await?.id
                }
            };
            let checked = Checked {
                resolved,
                is_pdf,
                name,
                label,
                raster_size: raster_size.map(|(_, _, size)| size),
                supplied_preview,
                input,
            };
            let queued = {
                let (this, id, signal, queued) =
                    (this.clone(), library_id.clone(), signal.clone(), signal.clone());
                this.clone().mutate_library(
                    &library_id,
                    move || Promise::spawn(async move { this.append(&id, checked, signal).await }),
                    queued,
                )
            };
            adopt(queued).await
        })
    }

    /// `createAsset`'s mutation: the asset becomes a page of the library.
    async fn append(
        &self,
        library_id: &str,
        checked: Checked,
        signal: Option<AbortSignal>,
    ) -> Result<StampAsset, Error> {
        let Some(live) = self.state().libraries.get(library_id).cloned() else {
            return Err(stamp_error(
                code::NOT_FOUND,
                format!("library '{library_id}' no longer exists"),
            ));
        };
        let asset_id = asset_id_for(&live.id, &checked.name);
        if self.state().assets.contains_key(&asset_id) {
            return Err(stamp_error(
                code::INVALID_INPUT,
                format!("library '{}' already has a stamp named '{}'", live.name, checked.name),
            ));
        }
        let canonical = self.0.binaries.library_binaries.borrow().get(&live.id).cloned().flatten();
        let Some(canonical) = canonical else {
            return Err(stamp_error(
                code::OPERATION_FAILED,
                format!("canonical bytes are missing for library '{}'", live.id),
            ));
        };
        let doc =
            cancellable(CAPABILITY, signal.clone(), self.open_asset_document(canonical)).await?;
        let body = self.append_page(&doc, &live, asset_id, &checked).await;
        let appended = close(&doc, body).await?;
        throw_if_cancelled(signal.as_ref())?;
        self.0.binaries.library_binaries.borrow_mut().insert(live.id.clone(), appended.canonical);
        self.0.binaries.binaries.borrow_mut().insert(
            appended.asset.id.clone(),
            AssetBinary { bytes: appended.bytes, preview: Some(appended.preview) },
        );
        self.update_state(|s| add_asset(s, &appended.asset));
        self.0.events.library_changed.emit(&StampLibraryChangedEvent {
            library_id: live.id.clone(),
            reason: StampLibraryChangeReason::AssetAdded,
        });
        self.0.events.asset_created.emit(&StampAssetEvent {
            asset_id: appended.asset.id.clone(),
            library_id: live.id.clone(),
            asset: Some(appended.asset.clone()),
        });
        Ok(appended.asset)
    }

    /// The body of the append's `try`, on the library's opened document.
    async fn append_page(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        live: &StampLibrary,
        asset_id: String,
        checked: &Checked,
    ) -> Result<Appended, Error> {
        let bytes = checked.resolved.bytes.clone();
        let page: Value = if checked.is_pdf {
            let result = call(doc, "pages.insert", vec![bytes_json(bytes)]).await?;
            let inserted = member(&result, "insertedPages")?;
            let inserted = inserted.as_array().cloned().unwrap_or_default();
            if inserted.len() != 1 {
                return Err(stamp_error(
                    code::INVALID_INPUT,
                    "a library asset must be a single-page PDF",
                ));
            }
            inserted[0].clone()
        } else {
            // A raster becomes a page: a blank page the image's size, the
            // image placed to fill it, flattened into content. From here on
            // it is a page like any other.
            if !doc.has("pages.flatten") {
                return Err(stamp_error(
                    code::UNSUPPORTED,
                    "raster assets need an asset engine with pages.flatten",
                ));
            }
            let size = checked.raster_size.clone().unwrap_or_else(undefined);
            let blank = call(doc, "pages.insertBlank", vec![json!({ "size": size })]).await?;
            let page = member(&blank, "insertedPages")?
                .as_array()
                .and_then(|p| p.first())
                .cloned()
                .unwrap_or_else(undefined);
            let pon = page_ref(&page)?.page_object_number;
            let width = size.get("width").cloned().unwrap_or_else(undefined);
            let height = size.get("height").cloned().unwrap_or_else(undefined);
            page_call(
                doc,
                "page.annotations.create",
                pon,
                vec![
                    json!({
                        "subtype": "stamp",
                        "box": { "x": 0, "y": 0, "width": width, "height": height },
                        "fit": "fill",
                    }),
                    json!({ "resources": { "appearance": bytes_json(bytes) } }),
                ],
            )
            .await?;
            let flattened =
                call(doc, "pages.flatten", vec![json!([page]), json!({ "usage": "display" })])
                    .await?;
            let results = member(&flattened, "results")?;
            let failed = results
                .as_array()
                .into_iter()
                .flatten()
                .any(|result| result.get("status").and_then(Value::as_str) != Some("applied"));
            if failed {
                return Err(stamp_error(
                    code::OPERATION_FAILED,
                    "flattening the raster into its page failed",
                ));
            }
            page
        };
        let listed = call(doc, "pages.list", Vec::new()).await?;
        let number = page.get("objectNumber").and_then(Value::as_f64);
        let layout = member(&listed, "pages")?.as_array().and_then(|pages| {
            pages
                .iter()
                .find(|candidate| {
                    candidate.get("ref").and_then(|r| r.get("objectNumber")).and_then(Value::as_f64)
                        == number
                })
                .cloned()
        });
        let Some(layout) = layout else {
            return Err(stamp_error(
                code::OPERATION_FAILED,
                "the new page is missing from the library layout",
            ));
        };
        let pon = page_ref(&page)?.page_object_number;
        // Insert copies the page only: register it in the same mutation.
        call(
            doc,
            "pages.setName",
            vec![json!({ "name": stamp_key(&checked.name, &checked.label), "page": page })],
        )
        .await?;
        let size = layout.get("size");
        let n = |k: &str| size.and_then(|s| s.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let asset = StampAsset::new(
            asset_id,
            live.id.clone(),
            checked.input.kind.unwrap_or(StampAssetKind::Stamp),
            checked.name.clone(),
            checked.label.clone(),
            StampSize { width: n("width"), height: n("height") },
            PageRef::new(pon),
            checked.input.subject.clone(),
            checked.input.categories.clone(),
        );
        page_call(
            doc,
            "page.pieceInfo.update",
            pon,
            vec![
                json!(STAMP_PIECEINFO_APP),
                stamp_piece_info(asset.kind, asset.subject.as_deref(), asset.categories.as_deref()),
            ],
        )
        .await?;
        let bytes = bytes_of(&call(doc, "pages.extract", vec![json!([page])]).await?);
        let preview = match &checked.supplied_preview {
            Some(preview) => preview.clone(),
            None => self.render_thumbnail(doc, pon).await?,
        };
        let canonical = bytes_of(&call(doc, "download", Vec::new()).await?);
        Ok(Appended { asset, bytes, preview, canonical })
    }

    /// `createAsset(input, options)`: a stamp from bytes or a mark. Rejects
    /// `not-found` for an unknown library, `invalid-input` for bytes that
    /// aren't a picture or a taken identifier. Fires `onAssetCreated`.
    pub fn create_asset(
        &self,
        input: AddAssetInput,
        options: OperationOptions,
    ) -> Promise<StampAsset> {
        verb(self.create_asset_now(input, options))
    }

    /// `createAssetFromAnnotations(page, refs, input, options)`: annotations
    /// on a page of a document become one stamp, exactly as they look.
    /// Rejects `permission-denied` without `doc.download`, `not-found` for a
    /// page that isn't in the document.
    pub fn create_asset_from_annotations(
        &self,
        page: PageTarget,
        refs: Vec<Value>,
        input: AddAssetInput,
        options: StampDocumentOptions,
    ) -> Promise<StampAsset> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let document_id = this.target_of(options.document_id.as_deref())?;
            if !this.can_create_from_annotations(Some(&document_id)) {
                return Err(permission_denied("doc.download", "stamp.createAssetFromAnnotations"));
            }
            let Some(doc) = this.cx().document_handle(Some(&document_id)) else {
                return Err(stamp_error(
                    code::NOT_FOUND,
                    format!("document '{document_id}' is not open"),
                ));
            };
            let layout = this.page_of(&document_id, page)?;
            let pon =
                page_ref(&layout.get("ref").cloned().unwrap_or_else(undefined))?.page_object_number;
            // The engine flattens the selection into a fresh single-page PDF:
            // the same placement whole-page flatten uses, aimed at a new page.
            let exported = cancellable(
                CAPABILITY,
                signal.clone(),
                doc.call("page.annotations.exportAppearance", Some(pon), vec![Value::Array(refs)]),
            )
            .await?;
            let input = AddAssetInput { source: Some(exported), ..input };
            adopt(this.create_asset_now(input, OperationOptions { signal })).await
        }))
    }

    /// `deleteAsset(assetId)`: remove a stamp from its library. Fires
    /// `onAssetDeleted`.
    pub fn delete_asset(&self, asset_id: &str, options: OperationOptions) -> Promise<()> {
        verb(self.delete_asset_now(asset_id, options))
    }

    fn delete_asset_now(&self, asset_id: &str, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        let id = asset_id.to_string();
        Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let Some(initial) = this.state().assets.get(&id).cloned() else { return Ok(()) };
            let queued = {
                let (this, id, signal, queued) =
                    (this.clone(), id.clone(), signal.clone(), signal.clone());
                this.clone().mutate_library(
                    &initial.library_id,
                    move || Promise::spawn(async move { this.remove_page(&id, signal).await }),
                    queued,
                )
            };
            adopt(queued).await
        })
    }

    /// `deleteAsset`'s mutation. The page goes; the engine drops its
    /// registry entry inside the delete. The unregistered blank keeps the
    /// file valid when this was the last stamp.
    async fn remove_page(&self, id: &str, signal: Option<AbortSignal>) -> Result<(), Error> {
        let Some(asset) = self.state().assets.get(id).cloned() else { return Ok(()) };
        let Some(library) = self.state().libraries.get(&asset.library_id).cloned() else {
            return Ok(());
        };
        let canonical =
            self.0.binaries.library_binaries.borrow().get(&library.id).cloned().flatten();
        let Some(canonical) = canonical else {
            return Err(stamp_error(
                code::OPERATION_FAILED,
                format!("canonical bytes are missing for library '{}'", library.id),
            ));
        };
        let doc =
            cancellable(CAPABILITY, signal.clone(), self.open_asset_document(canonical)).await?;
        let body = async {
            call(&doc, "pages.delete", vec![json!([asset.page])]).await?;
            Ok(bytes_of(&call(&doc, "download", Vec::new()).await?))
        }
        .await;
        let rewritten = close(&doc, body).await?;
        throw_if_cancelled(signal.as_ref())?;
        self.0.binaries.library_binaries.borrow_mut().insert(library.id.clone(), rewritten);
        self.0.binaries.binaries.borrow_mut().remove(id);
        self.0.binaries.ghost_renders.borrow_mut().remove(id);
        self.update_state(|s| remove_asset(s, id));
        self.0.events.library_changed.emit(&StampLibraryChangedEvent {
            library_id: library.id.clone(),
            reason: StampLibraryChangeReason::AssetRemoved,
        });
        self.0.events.asset_deleted.emit(&StampAssetEvent {
            asset_id: id.to_string(),
            library_id: library.id.clone(),
            asset: None,
        });
        Ok(())
    }

    /// `updateAsset(assetId, changes)`: change a stamp's label, subject or
    /// categories; its identifier never changes. Rejects `not-found`.
    pub fn update_asset(
        &self,
        asset_id: &str,
        patch: StampAssetPatch,
        options: OperationOptions,
    ) -> Promise<StampAsset> {
        let this = self.clone();
        let id = asset_id.to_string();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let Some(initial) = this.state().assets.get(&id).cloned() else {
                return Err(not_found("asset", &id));
            };
            let queued = {
                let (this, id, signal, queued) =
                    (this.clone(), id.clone(), signal.clone(), signal.clone());
                this.clone().mutate_library(
                    &initial.library_id,
                    move || Promise::spawn(async move { this.relabel(&id, &patch, signal).await }),
                    queued,
                )
            };
            adopt(queued).await
        }))
    }

    /// `updateAsset`'s mutation.
    async fn relabel(
        &self,
        id: &str,
        patch: &StampAssetPatch,
        signal: Option<AbortSignal>,
    ) -> Result<StampAsset, Error> {
        let asset = self.state().assets.get(id).cloned();
        let library =
            asset.as_ref().and_then(|asset| self.state().libraries.get(&asset.library_id).cloned());
        let (Some(asset), Some(library)) = (asset, library) else {
            return Err(not_found("asset", id));
        };
        let mut next = asset.clone();
        if let Some(label) = &patch.label {
            next.label = label.clone();
        }
        if let Some(categories) = &patch.categories {
            next.categories = Some(categories.clone());
            next.members.ensure("categories");
        }
        match &patch.subject {
            Some(None) => {
                next.subject = None;
                next.members.remove("subject");
            }
            Some(Some(subject)) => {
                next.subject = Some(subject.clone());
                next.members.ensure("subject");
            }
            None => {}
        }
        let canonical =
            self.0.binaries.library_binaries.borrow().get(&library.id).cloned().flatten();
        let Some(canonical) = canonical else {
            return Err(stamp_error(
                code::OPERATION_FAILED,
                format!("canonical bytes are missing for library '{}'", library.id),
            ));
        };
        let doc =
            cancellable(CAPABILITY, signal.clone(), self.open_asset_document(canonical)).await?;
        let body = async {
            if next.label != asset.label {
                // A relabel is a registry rename: one job, the identifier
                // untouched.
                call(
                    &doc,
                    "pages.setName",
                    vec![json!({
                        "name": stamp_key(&asset.name, &next.label),
                        "page": asset.page,
                        "replace": stamp_key(&asset.name, &asset.label),
                    })],
                )
                .await?;
            }
            page_call(
                &doc,
                "page.pieceInfo.update",
                asset.page.page_object_number,
                vec![
                    json!(STAMP_PIECEINFO_APP),
                    stamp_piece_info(
                        next.kind,
                        next.subject.as_deref(),
                        next.categories.as_deref(),
                    ),
                ],
            )
            .await?;
            Ok(bytes_of(&call(&doc, "download", Vec::new()).await?))
        }
        .await;
        let rewritten = close(&doc, body).await?;
        throw_if_cancelled(signal.as_ref())?;
        self.0.binaries.library_binaries.borrow_mut().insert(library.id.clone(), rewritten);
        self.update_state(|s| set_asset(s, &next));
        self.0.events.library_changed.emit(&StampLibraryChangedEvent {
            library_id: library.id.clone(),
            reason: StampLibraryChangeReason::AssetUpdated,
        });
        self.0.events.asset_updated.emit(&StampAssetEvent {
            asset_id: next.id.clone(),
            library_id: library.id.clone(),
            asset: Some(next.clone()),
        });
        Ok(next)
    }

    /// The input a copy of an asset is made from: its bytes, kind, label,
    /// subject, categories and preview.
    fn copy_input(
        &self,
        asset: &StampAsset,
        binary: &AssetBinary,
        library_id: String,
        label: String,
    ) -> AddAssetInput {
        AddAssetInput {
            library_id: Some(library_id),
            source: Some(bytes_json(binary.bytes.clone().unwrap_or_default())),
            kind: Some(asset.kind),
            label: Some(label),
            subject: asset.subject.clone(),
            categories: asset.categories.clone(),
            // `binary.preview ? { preview: binary.preview.bytes } : {}`
            preview: binary.preview.as_ref().and_then(|p| p.bytes.clone()),
            ..AddAssetInput::default()
        }
    }

    /// `moveAsset(assetId, to)`: a copy there, with a new id, then the
    /// original goes.
    pub fn move_asset(
        &self,
        asset_id: &str,
        to_library_id: &str,
        options: OperationOptions,
    ) -> Promise<StampAsset> {
        let this = self.clone();
        let id = asset_id.to_string();
        let to = to_library_id.to_string();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let asset = this.state().assets.get(&id).cloned();
            let binary = this.0.binaries.binaries.borrow().get(&id).cloned();
            let (Some(asset), Some(binary)) = (asset, binary) else {
                return Err(not_found("asset", &id));
            };
            if to == asset.library_id {
                return Ok(asset);
            }
            let input = this.copy_input(&asset, &binary, to, asset.label.clone());
            let copy = this.create_asset_now(input, OperationOptions { signal }).await?;
            // The copy is in place: the original goes even if the caller
            // cancels now.
            this.delete_asset_now(&id, OperationOptions::default()).await?;
            Ok(copy)
        }))
    }

    /// `duplicateAsset(assetId, {label})`: copy a stamp within its library.
    pub fn duplicate_asset(
        &self,
        asset_id: &str,
        label: Option<String>,
        options: OperationOptions,
    ) -> Promise<StampAsset> {
        let this = self.clone();
        let id = asset_id.to_string();
        verb(Promise::spawn(async move {
            throw_if_cancelled(options.signal.as_ref())?;
            let asset = this.state().assets.get(&id).cloned();
            let binary = this.0.binaries.binaries.borrow().get(&id).cloned();
            let (Some(asset), Some(binary)) = (asset, binary) else {
                return Err(not_found("asset", &id));
            };
            let label = label.unwrap_or_else(|| format!("{} copy", asset.label));
            let input = this.copy_input(&asset, &binary, asset.library_id.clone(), label);
            adopt(this.create_asset_now(input, options)).await
        }))
    }
}
