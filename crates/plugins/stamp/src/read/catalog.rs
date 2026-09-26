//! `read/catalog.ts`: reads over the store and the binary sidecar:
//! libraries, assets, previews, the library's PDF, and the two twins.

use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::ANNOTATION;

use crate::contract::*;
use crate::controller::StampCapability;
use crate::services::errors::not_found;

impl StampCapability {
    /// `listLibraries(filter?)`: display order, optionally one kind or
    /// several.
    pub fn list_libraries(&self, filter: Option<&StampLibraryFilter>) -> Vec<StampLibrary> {
        let s = self.state();
        let kinds = filter.and_then(|f| f.kind.as_ref());
        s.library_order
            .iter()
            .filter_map(|id| s.libraries.get(id))
            .filter(|l| kinds.is_none_or(|kinds| kinds.contains(&l.kind)))
            .cloned()
            .collect()
    }

    /// `getLibrary(id)`.
    pub fn get_library(&self, id: &str) -> Option<StampLibrary> {
        self.state().libraries.get(id).cloned()
    }

    /// `listAssets(filter?)`: library order, then asset order.
    pub fn list_assets(&self, filter: Option<&StampAssetFilter>) -> Vec<StampAsset> {
        let s = self.state();
        let library_id = filter.and_then(|f| f.library_id.as_deref()).filter(|id| !id.is_empty());
        let scope: Vec<String> = match library_id {
            Some(id) => vec![id.to_string()],
            None => s.library_order.clone(),
        };
        let kind = filter.and_then(|f| f.kind);
        let category = filter.and_then(|f| f.category.as_deref()).filter(|c| !c.is_empty());
        scope
            .iter()
            .flat_map(|lid| s.libraries.get(lid).map(|l| l.asset_ids.clone()).unwrap_or_default())
            .filter_map(|id| s.assets.get(&id).cloned())
            .filter(|a| kind.is_none_or(|k| a.kind == k))
            .filter(|a| {
                category.is_none_or(|c| {
                    a.categories.as_ref().is_some_and(|cs| cs.iter().any(|x| x == c))
                })
            })
            .collect()
    }

    /// `getAsset(id)`.
    pub fn get_asset(&self, id: &str) -> Option<StampAsset> {
        self.state().assets.get(id).cloned()
    }

    /// `getAssetPreview(id)`: the cached thumbnail, or `None`.
    pub fn get_asset_preview(&self, id: &str) -> Option<StampAssetPreview> {
        self.0.binaries.borrow().get(id).and_then(|b| b.preview.clone())
    }

    /// `renderAssetPreview(id, {width})`: a preview at any width, cached per
    /// width; `not-found` for an unknown asset.
    pub fn render_asset_preview(&self, id: &str, width: f64) -> Promise<Option<StampAssetPreview>> {
        let Some(bin) = self.0.binaries.borrow().get(id).cloned() else {
            return Promise::rejected(not_found("asset", id));
        };
        let local = self.ghost_cache(Some(id));
        let pending = self.ghost_render(&local, &bin.bytes, &bin.preview, width);
        // `.then((preview) => preview ? {bytes, mimeType ?? 'image/png'} : null)`
        Promise::spawn(async move {
            Ok(pending.await?.map(|p| StampAssetPreview {
                bytes: p.bytes,
                mime_type: Some(p.mime_type.unwrap_or_else(|| "image/png".into())),
            }))
        })
    }

    /// `readAssetBytes(id)`: a copy of the placement bytes, or `None`.
    pub fn read_asset_bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.0.binaries.borrow().get(id).and_then(|b| b.bytes.clone())
    }

    /// `exportLibrary(id)`: the library as the PDF it is; `not-found` when
    /// there are no canonical bytes.
    pub fn export_library(&self, id: &str) -> Promise<Vec<u8>> {
        let bytes = self.0.library_binaries.borrow().get(id).cloned().flatten();
        let id = id.to_string();
        Promise::spawn(async move { bytes.ok_or_else(|| not_found("library", &id)) })
    }

    /// `canPlace(documentId?)`: the target's annotation create grant, the
    /// active document when none is named.
    pub fn can_place(&self, document_id: Option<&str>) -> bool {
        let id = match document_id {
            Some(id) => Some(id.to_string()),
            None => self.cx().core().active_id.clone(),
        };
        let Some(id) = id else { return false };
        self.cx()
            .try_for_document(&ANNOTATION, &id)
            .ok()
            .flatten()
            .is_some_and(|annotation| annotation.can_create())
    }
}
