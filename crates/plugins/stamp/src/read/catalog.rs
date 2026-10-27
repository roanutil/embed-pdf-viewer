//! `read/catalog.ts`: reads over the state and the binaries resource:
//! libraries, assets, previews, the library's PDF, and the checks.

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::ANNOTATION;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::services::errors::{CAPABILITY, not_found, throw_if_cancelled, verb};

impl StampCapability {
    /// `listLibraries(filter)`: in display order, of the kinds asked for.
    pub fn list_libraries(&self, filter: Option<&StampLibraryFilter>) -> Vec<StampLibrary> {
        let state = self.state();
        let kinds = filter.and_then(|f| f.kind.as_ref());
        state
            .library_order
            .iter()
            .filter_map(|id| state.libraries.get(id))
            .filter(|library| kinds.is_none_or(|kinds| kinds.contains(&library.kind)))
            .cloned()
            .collect()
    }

    /// `getLibrary(libraryId)`.
    pub fn get_library(&self, library_id: &str) -> Option<StampLibrary> {
        self.state().libraries.get(library_id).cloned()
    }

    /// `listAssets(filter)`: in the order a picker shows them, by library,
    /// then within it. An empty `libraryId` filters nothing, as the
    /// TypeScript's falsy check has it.
    pub fn list_assets(&self, filter: Option<&StampAssetFilter>) -> Vec<StampAsset> {
        let state = self.state();
        let library_ids: Vec<String> =
            match filter.and_then(|f| f.library_id.clone()).filter(|id| !id.is_empty()) {
                Some(id) => vec![id],
                None => state.library_order.clone(),
            };
        let kind = filter.and_then(|f| f.kind);
        let category =
            filter.and_then(|f| f.category.as_deref()).filter(|category| !category.is_empty());
        library_ids
            .iter()
            .filter_map(|id| state.libraries.get(id))
            .flat_map(|library| library.asset_ids.iter().filter_map(|id| state.assets.get(id)))
            .filter(|asset| kind.is_none_or(|kind| asset.kind == kind))
            .filter(|asset| {
                category.is_none_or(|category| {
                    asset.categories.as_ref().is_some_and(|c| c.iter().any(|c| c == category))
                })
            })
            .cloned()
            .collect()
    }

    /// `getAsset(assetId)`.
    pub fn get_asset(&self, asset_id: &str) -> Option<StampAsset> {
        self.state().assets.get(asset_id).cloned()
    }

    /// `getAssetPreview(assetId)`: the cached thumbnail, or `None`.
    pub fn get_asset_preview(&self, asset_id: &str) -> Option<StampAssetPreview> {
        self.0.binaries.binaries.borrow().get(asset_id).and_then(|b| b.preview.clone())
    }

    /// `renderAssetPreview(assetId, {width, signal})`: a preview at any
    /// width, rendered by the asset engine and cached per width. Rejects
    /// `not-found`.
    pub fn render_asset_preview(
        &self,
        asset_id: &str,
        width: f64,
        options: OperationOptions,
    ) -> Promise<Option<StampAssetPreview>> {
        let this = self.clone();
        let asset_id = asset_id.to_string();
        verb(Promise::spawn(async move {
            throw_if_cancelled(options.signal.as_ref())?;
            let binary = this.0.binaries.binaries.borrow().get(&asset_id).cloned();
            let Some(binary) = binary else { return Err(not_found("asset", &asset_id)) };
            let provider = this.ghost_provider(binary.bytes, binary.preview, Some(&asset_id));
            let preview = cancellable(CAPABILITY, options.signal, provider(width)).await?;
            Ok(preview.map(|preview| StampAssetPreview {
                bytes: preview.bytes,
                // `preview.mimeType ?? 'image/png'`
                mime_type: Some(
                    preview
                        .mime_type
                        .filter(|m| !m.is_null())
                        .unwrap_or_else(|| Value::String("image/png".into())),
                ),
            }))
        }))
    }

    /// `readAssetBytes(assetId)`: a copy of the asset's placement bytes (a
    /// one-page PDF), or `None`.
    pub fn read_asset_bytes(&self, asset_id: &str) -> Option<Vec<u8>> {
        self.0.binaries.binaries.borrow().get(asset_id).and_then(|b| b.bytes.clone())
    }

    /// `exportLibrary(libraryId)`: the library as the PDF it is. Rejects
    /// `not-found`. The TypeScript leaves this one out of `verb`.
    pub fn export_library(&self, library_id: &str, options: OperationOptions) -> Promise<Vec<u8>> {
        let this = self.clone();
        let library_id = library_id.to_string();
        Promise::spawn(async move {
            throw_if_cancelled(options.signal.as_ref())?;
            let bytes =
                this.0.binaries.library_binaries.borrow().get(&library_id).cloned().flatten();
            bytes.ok_or_else(|| not_found("library", &library_id))
        })
    }

    /// `canPlace(documentId)`: placing is annotation work, so the document's
    /// annotation plugin answers.
    pub fn can_place(&self, document_id: Option<&str>) -> bool {
        let Some(id) = self.document_id_of(document_id) else { return false };
        match self.cx().try_for_document(&ANNOTATION, &id) {
            Ok(Some(annotation)) => annotation.can_create(),
            _ => false,
        }
    }

    /// `canCreateFromAnnotations(documentId)`: a stamp made from
    /// annotations copies them out of the document, as a download does.
    pub fn can_create_from_annotations(&self, document_id: Option<&str>) -> bool {
        let Some(id) = self.document_id_of(document_id) else { return false };
        self.documents().is_ok_and(|documents| documents.allows("doc.download", Some(&id)))
    }
}

/// `{ bytes, mimeType }` of a preview for a caller that prints it, `null`
/// for none.
pub fn preview_json(preview: Option<&StampAssetPreview>) -> Value {
    preview.map_or(Value::Null, StampAssetPreview::to_json)
}

/// The byte copy a reader takes, as JSON.
pub fn bytes_json(bytes: Option<Vec<u8>>) -> Value {
    bytes.map_or(Value::Null, |bytes| json!(epdf_kernel::bytes::Bytes(bytes).to_json()))
}
