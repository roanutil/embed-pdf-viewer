//! `write/assets.ts`: asset writes. Every asset is a page of its library's
//! PDF: a single-page PDF is inserted, a raster becomes a page with the
//! image flattened into it. Removal deletes the page; a relabel renames the
//! registry entry. All through the per-library mutation queue.

use epdf_geometry::Size;
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_annotation::binary::{resolve_binary_source, sniff_binary_metadata};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::convention::*;
use crate::model::StampAction;
use crate::promise::{adopt, await_engine};
use crate::services::asset_engine::{bytes_of, call, close, page_call};
use crate::services::binaries::AssetBinary;
use crate::services::errors::not_found;
use crate::value::{get, str_of, truthy};
use crate::write::libraries::pon_of_ref;
use crate::write::marks::ResolvedSource;

fn number(x: f64) -> Value {
    epdf_plugin_annotation::value::number(x)
}

fn missing_bytes(library_id: &str) -> Error {
    Error::engine(
        "Unknown",
        format!("[stamp] canonical bytes are missing for library '{library_id}'"),
    )
}

/// What `addAsset` resolved before its mutation.
struct Prepared {
    library_id: String,
    name: String,
    label: String,
    kind: StampAssetKind,
    subject: Option<String>,
    categories: Option<Vec<String>>,
    resolved: ResolvedSource,
    is_pdf: bool,
    raster_size: Option<Size>,
    supplied_preview: Option<StampAssetPreview>,
}

impl StampCapability {
    /// `createAsset(input)`, the TypeScript's `addAsset`.
    pub fn create_asset(&self, input: AddAssetInput) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            if let Some(id) = input.library_id.as_deref().filter(|id| !id.is_empty())
                && !this.state().libraries.contains_key(id)
            {
                return Err(Error::engine("NotFound", format!("[stamp] unknown library '{id}'")));
            }
            let resolved = rt::returned(this.resolve_asset_source(&input)).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] asset source must be PNG, JPEG, or single-page PDF bytes",
                ));
            };
            let is_pdf = meta.mime_type == "application/pdf";
            let name = match &input.name {
                Some(name) => name.clone(),
                None => custom_stamp_name(&|| this.random()),
            };
            let label = input.label.clone().unwrap_or_else(|| name.clone());
            if name.is_empty() || name.contains('=') {
                return Err(Error::engine(
                    "InvalidArg",
                    format!(
                        "[stamp] invalid stamp identifier '{name}': must be non-empty and contain no '='"
                    ),
                ));
            }
            let raster_size =
                input.size.or(meta.size.map(|(width, height)| Size { width, height }));
            if !is_pdf && raster_size.is_none_or(|s| s.width <= 0.0 || s.height <= 0.0) {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] a raster asset needs a positive `size` (its page size in points)",
                ));
            }
            let mut supplied_preview = None;
            if let Some(preview) = input.preview.as_ref().filter(|p| truthy(Some(p))) {
                let preview = resolve_binary_source(preview).await?;
                supplied_preview = Some(StampAssetPreview {
                    bytes: Some(preview.bytes),
                    mime_type: Some(preview.mime_type.unwrap_or_else(|| "image/png".into())),
                });
            }
            // No library named: one of its own, named after the label.
            let library_id = match input.library_id.clone() {
                Some(id) => id,
                None => this.create_library(&label, CreateLibraryOptions::default()).await?,
            };
            let prepared = Prepared {
                library_id: library_id.clone(),
                name,
                label,
                kind: input.kind.unwrap_or(StampAssetKind::Stamp),
                subject: input.subject.clone(),
                categories: input.categories.clone(),
                resolved,
                is_pdf,
                raster_size,
                supplied_preview,
            };
            let mutating = this.clone();
            let queued = this.mutate_library(&library_id, move || {
                Promise::spawn(async move { mutating.append_asset(prepared).await })
            });
            adopt(queued).await
        })
    }

    /// The mutation of `addAsset`: the page appended to the library's PDF.
    async fn append_asset(&self, p: Prepared) -> Result<String, Error> {
        let Some(live) = self.state().libraries.get(&p.library_id).cloned() else {
            return Err(Error::engine(
                "NotFound",
                format!("[stamp] library '{}' no longer exists", p.library_id),
            ));
        };
        let asset_id = asset_id_for(&live.id, &p.name);
        if self.state().assets.contains_key(&asset_id) {
            return Err(Error::engine(
                "InvalidArg",
                format!("[stamp] library '{}' already has a stamp named '{}'", live.name, p.name),
            ));
        }
        let Some(canonical) = self.0.library_binaries.borrow().get(&live.id).cloned().flatten()
        else {
            return Err(missing_bytes(&live.id));
        };
        let doc = rt::returned(self.open_asset_document(canonical)).await?;
        let body = async {
            Self::require_canonical_services(&doc)?;
            let page: Value = if p.is_pdf {
                let result =
                    await_engine(call(&doc, "pages.insert", vec![Bytes(p.resolved.bytes.clone()).to_json()]))
                        .await?;
                let inserted =
                    get(&result, "insertedPages").and_then(Value::as_array).cloned().unwrap_or_default();
                if inserted.len() != 1 {
                    return Err(Error::engine(
                        "InvalidArg",
                        "[stamp] a library asset must be a single-page PDF",
                    ));
                }
                inserted[0].clone()
            } else {
                // Raster to page: a blank page the image's size, the image
                // placed to fill it, flattened into content.
                if !doc.has("pages.flatten") {
                    return Err(Error::engine(
                        "NotImplemented",
                        "[stamp] raster assets need an asset engine with pages.flatten",
                    ));
                }
                let size = p.raster_size.unwrap_or(Size { width: 0.0, height: 0.0 });
                let blank = await_engine(call(
                    &doc,
                    "pages.insertBlank",
                    vec![json!({ "size": { "width": number(size.width), "height": number(size.height) } })],
                ))
                .await?;
                let page = get(&blank, "insertedPages")
                    .and_then(Value::as_array)
                    .and_then(|p| p.first())
                    .cloned()
                    .unwrap_or(Value::Null);
                let pon = pon_of_ref(&page).unwrap_or(0);
                doc.page_index(pon)?;
                let draft = json!({
                    "subtype": "stamp",
                    "rect": { "left": 0, "bottom": 0, "right": number(size.width), "top": number(size.height) },
                    "source": Bytes(p.resolved.bytes.clone()).to_json(),
                    "fit": "fill",
                });
                await_engine(page_call(&doc, "page.annotations.create", pon, vec![draft])).await?;
                let flattened =
                    await_engine(call(&doc, "pages.flatten", vec![json!([page]), json!("display")]))
                        .await?;
                let failed = get(&flattened, "results")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|r| str_of(r, "status") != Some("applied"));
                if failed {
                    return Err(Error::engine(
                        "Unknown",
                        "[stamp] flattening the raster into its page failed",
                    ));
                }
                page
            };
            let pon = pon_of_ref(&page);
            let listed = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let layout = get(&listed, "pages").and_then(Value::as_array).and_then(|pages| {
                pages.iter().find(|c| get(c, "ref").and_then(pon_of_ref) == pon).cloned()
            });
            let pon = pon.unwrap_or(0);
            doc.page_index(pon)?;
            let (Some(layout), true) = (layout, doc.has("page.pieceInfo")) else {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] canonical PDF libraries need page layout and pieceInfo support",
                ));
            };
            // Insert copies the page only: register it in the same mutation.
            await_engine(call(
                &doc,
                "pages.setName",
                vec![json!({ "name": stamp_key(&p.name, &p.label), "page": page })],
            ))
            .await?;
            let size = get(&layout, "size").cloned().unwrap_or(Value::Null);
            let dim = |k| crate::value::num(&size, k).unwrap_or(f64::NAN);
            let asset = StampAsset {
                id: asset_id.clone(),
                library_id: live.id.clone(),
                kind: p.kind,
                name: p.name.clone(),
                label: p.label.clone(),
                size: Size { width: dim("width"), height: dim("height") },
                page: PageRef::from_json(&page).unwrap_or(PageRef::new(pon)),
                subject: p.subject.clone(),
                categories: p.categories.clone(),
            };
            let info = stamp_piece_info(asset.kind, asset.subject.as_deref(), asset.categories.as_deref());
            await_engine(page_call(
                &doc,
                "page.pieceInfo.update",
                pon,
                vec![json!(STAMP_PIECEINFO_APP), info],
            ))
            .await?;
            let bytes = await_engine(call(&doc, "pages.extract", vec![json!([page])])).await?;
            let preview = match &p.supplied_preview {
                Some(preview) => preview.clone(),
                None => rt::returned(self.render_thumbnail(&doc, pon)).await?,
            };
            let canonical = await_engine(call(&doc, "download", Vec::new())).await?;
            Ok((asset, AssetBinary { bytes: bytes_of(&bytes), preview: Some(preview) }, canonical))
        }
        .await;
        let (asset, binary, canonical) = close(&doc, body).await?;
        self.0.library_binaries.borrow_mut().insert(live.id.clone(), bytes_of(&canonical));
        self.0.binaries.borrow_mut().insert(asset.id.clone(), binary);
        self.dispatch(StampAction::AssetAdded { asset: asset.clone() });
        let events = &self.0.events;
        events.library_changed.emit(&StampLibraryChange {
            library_id: live.id.clone(),
            reason: StampLibraryChangeReason::AssetAdded,
        });
        events.asset_created.emit(&StampAssetEvent {
            asset_id: asset.id.clone(),
            library_id: live.id.clone(),
            asset: Some(asset.clone()),
        });
        Ok(asset.id)
    }

    /// `createAssetFromAnnotations(documentId, page, refs, input)`: the
    /// annotations' appearance, exported by the document's engine, becomes
    /// an asset.
    pub fn create_asset_from_annotations(
        &self,
        document_id: &str,
        page: PageRef,
        refs: Vec<Value>,
        input: AddAssetInput,
    ) -> Promise<String> {
        let this = self.clone();
        let document_id = document_id.to_string();
        Promise::spawn(async move {
            let Some(doc) = this.cx().document_handle(Some(&document_id)) else {
                return Err(Error::engine(
                    "NotFound",
                    format!("[stamp] target document '{document_id}' is not open"),
                ));
            };
            doc.page_index(page.page_object_number)?;
            if !doc.has("page.annotations.exportAppearance") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] this document's engine cannot export annotation appearances",
                ));
            }
            let exported = await_engine(page_call(
                &doc,
                "page.annotations.exportAppearance",
                page.page_object_number,
                vec![Value::Array(refs)],
            ))
            .await?;
            // `addAsset({ ...input, source })`: a `mark` or `size` the caller
            // passed rides along, as the spread carries it.
            let input = AddAssetInput { source: Some(exported), ..input };
            adopt(this.create_asset(input)).await
        })
    }

    /// `deleteAsset(id)`: its page goes from the library's PDF, then the
    /// descriptor. An unknown id resolves and does nothing.
    pub fn delete_asset(&self, id: &str) -> Promise<()> {
        let this = self.clone();
        let id = id.to_string();
        Promise::spawn(async move {
            let Some(initial) = this.state().assets.get(&id).cloned() else { return Ok(()) };
            let mutating = this.clone();
            let queued = this.mutate_library(&initial.library_id, move || {
                Promise::spawn(async move { mutating.delete_now(&id).await })
            });
            adopt(queued).await
        })
    }

    async fn delete_now(&self, id: &str) -> Result<(), Error> {
        let Some(asset) = self.state().assets.get(id).cloned() else { return Ok(()) };
        let Some(library) = self.state().libraries.get(&asset.library_id).cloned() else {
            return Ok(());
        };
        let Some(canonical) = self.0.library_binaries.borrow().get(&library.id).cloned().flatten()
        else {
            return Err(missing_bytes(&library.id));
        };
        let doc = rt::returned(self.open_asset_document(canonical)).await?;
        let body = async {
            await_engine(call(&doc, "pages.delete", vec![json!([asset.page.to_json()])])).await?;
            await_engine(call(&doc, "download", Vec::new())).await
        }
        .await;
        let rewritten = close(&doc, body).await?;
        self.0.library_binaries.borrow_mut().insert(library.id.clone(), bytes_of(&rewritten));
        self.0.binaries.borrow_mut().remove(id);
        self.0.ghost_renders.borrow_mut().remove(id);
        self.dispatch(StampAction::AssetRemoved { asset_id: id.to_string() });
        let events = &self.0.events;
        events.library_changed.emit(&StampLibraryChange {
            library_id: library.id.clone(),
            reason: StampLibraryChangeReason::AssetRemoved,
        });
        events.asset_deleted.emit(&StampAssetEvent {
            asset_id: id.to_string(),
            library_id: library.id,
            asset: None,
        });
        Ok(())
    }

    /// `updateAsset(id, {label, subject, categories})`: a relabel renames the
    /// registry entry; the page PieceInfo is rewritten.
    pub fn update_asset(&self, id: &str, patch: AssetPatch) -> Promise<()> {
        let this = self.clone();
        let id = id.to_string();
        Promise::spawn(async move {
            let Some(initial) = this.state().assets.get(&id).cloned() else {
                return Err(Error::engine("NotFound", format!("[stamp] unknown asset '{id}'")));
            };
            let mutating = this.clone();
            let queued = this.mutate_library(&initial.library_id, move || {
                Promise::spawn(async move { mutating.update_now(&id, patch).await })
            });
            adopt(queued).await
        })
    }

    async fn update_now(&self, id: &str, patch: AssetPatch) -> Result<(), Error> {
        let asset = self.state().assets.get(id).cloned();
        let library =
            asset.as_ref().and_then(|a| self.state().libraries.get(&a.library_id).cloned());
        let (Some(asset), Some(library)) = (asset, library) else {
            return Err(Error::engine("NotFound", format!("[stamp] unknown asset '{id}'")));
        };
        let mut next = asset.clone();
        if let Some(label) = patch.label {
            next.label = label;
        }
        if let Some(categories) = patch.categories {
            next.categories = Some(categories);
        }
        match patch.subject {
            Some(None) => next.subject = None,
            Some(Some(subject)) => next.subject = Some(subject),
            None => {}
        }
        let Some(canonical) = self.0.library_binaries.borrow().get(&library.id).cloned().flatten()
        else {
            return Err(missing_bytes(&library.id));
        };
        let doc = rt::returned(self.open_asset_document(canonical)).await?;
        let body = async {
            Self::require_canonical_services(&doc)?;
            if next.label != asset.label {
                // A relabel is a registry rename, the identifier untouched.
                let arg = json!({
                    "name": stamp_key(&asset.name, &next.label),
                    "page": asset.page.to_json(),
                    "replace": stamp_key(&asset.name, &asset.label),
                });
                await_engine(call(&doc, "pages.setName", vec![arg])).await?;
            }
            let pon = asset.page.page_object_number;
            doc.page_index(pon)?;
            if doc.has("page.pieceInfo") {
                let info = stamp_piece_info(
                    next.kind,
                    next.subject.as_deref(),
                    next.categories.as_deref(),
                );
                await_engine(page_call(
                    &doc,
                    "page.pieceInfo.update",
                    pon,
                    vec![json!(STAMP_PIECEINFO_APP), info],
                ))
                .await?;
            } else {
                // `await page.pieceInfo?.update(...)`: `await undefined`.
                rt::tick().await;
            }
            await_engine(call(&doc, "download", Vec::new())).await
        }
        .await;
        let rewritten = close(&doc, body).await?;
        self.0.library_binaries.borrow_mut().insert(library.id.clone(), bytes_of(&rewritten));
        self.dispatch(StampAction::AssetUpdated { asset: next.clone() });
        let events = &self.0.events;
        events.library_changed.emit(&StampLibraryChange {
            library_id: library.id.clone(),
            reason: StampLibraryChangeReason::AssetUpdated,
        });
        events.asset_updated.emit(&StampAssetEvent {
            asset_id: next.id.clone(),
            library_id: library.id,
            asset: Some(next),
        });
        Ok(())
    }

    /// The `addAsset` input that copies an asset: its page bytes, kind,
    /// subject, categories and preview, a fresh identifier.
    fn copy_input(
        &self,
        asset: &StampAsset,
        bin: &AssetBinary,
        library_id: &str,
        label: String,
    ) -> AddAssetInput {
        AddAssetInput {
            library_id: Some(library_id.to_string()),
            name: None,
            label: Some(label),
            kind: Some(asset.kind),
            subject: asset.subject.clone(),
            categories: asset.categories.clone(),
            source: Some(Bytes(bin.bytes.clone().unwrap_or_default()).to_json()),
            mark: None,
            preview: bin
                .preview
                .as_ref()
                .and_then(|p| p.bytes.clone())
                .map(|bytes| Bytes(bytes).to_json()),
            size: None,
        }
    }

    /// `moveAsset(id, {libraryId})`: a copy there, then the original goes.
    pub fn move_asset(&self, id: &str, to_library_id: &str) -> Promise<String> {
        let this = self.clone();
        let (id, to) = (id.to_string(), to_library_id.to_string());
        Promise::spawn(async move {
            let asset = this.state().assets.get(&id).cloned();
            let bin = this.0.binaries.borrow().get(&id).cloned();
            let (Some(asset), Some(bin)) = (asset, bin) else {
                return Err(not_found("asset", &id));
            };
            if to == asset.library_id {
                return Ok(id);
            }
            let input = this.copy_input(&asset, &bin, &to, asset.label.clone());
            let copy = this.create_asset(input).await?;
            this.delete_asset(&id).await?;
            Ok(copy)
        })
    }

    /// `duplicateAsset(id, {label})`: a copy in its own library.
    pub fn duplicate_asset(&self, id: &str, label: Option<String>) -> Promise<String> {
        let this = self.clone();
        let id = id.to_string();
        Promise::spawn(async move {
            let asset = this.state().assets.get(&id).cloned();
            let bin = this.0.binaries.borrow().get(&id).cloned();
            let (Some(asset), Some(bin)) = (asset, bin) else {
                return Err(not_found("asset", &id));
            };
            let label = label.unwrap_or_else(|| format!("{} copy", asset.label));
            let input = this.copy_input(&asset, &bin, &asset.library_id.clone(), label);
            adopt(this.create_asset(input)).await
        })
    }
}
