//! `services/ghosts.ts`: ghost renders, one per size bucket per asset,
//! rendered lazily on the asset engine the first time the ghost is shown at
//! that size and kept until the asset goes away. Materialized (dynamic)
//! placements are user/time specific and cache only for their own arm (no
//! cache key).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::StampPreviewProvider;
use serde_json::Value;

use crate::contract::StampAssetPreview;
use crate::controller::StampCapability;
use crate::services::asset_engine::{
    call, close, image_to_preview, page_call, read_off, render_options,
};
use crate::services::binaries::{GhostCache, width_key};

impl StampCapability {
    /// `renderGhost(bytes, devicePixelWidth)`: the first page at that width.
    fn render_ghost(
        &self,
        bytes: Vec<u8>,
        device_pixel_width: f64,
    ) -> Promise<Option<StampAssetPreview>> {
        let opening = self.open_asset_document(bytes);
        Promise::spawn(async move {
            let doc = opening.await?;
            let body = async {
                let layout = call(&doc, "pages.list", Vec::new()).await?;
                if layout.is_null() || epdf_kernel::is_undefined(&layout) {
                    return Err(read_off(&layout, "pages"));
                }
                let pages = layout.get("pages").cloned().unwrap_or_else(epdf_kernel::undefined);
                if pages.is_null() || epdf_kernel::is_undefined(&pages) {
                    return Err(read_off(&pages, "0"));
                }
                // `if (!page) return null`
                let Some(first) = pages.as_array().and_then(|p| p.first()).cloned() else {
                    return Ok(None);
                };
                let page = first.get("ref").and_then(PageRef::from_json).ok_or_else(|| {
                    Error::named(
                        "TypeError",
                        "Cannot read properties of undefined (reading 'objectNumber')",
                    )
                })?;
                let image = page_call(
                    &doc,
                    "page.render.image",
                    page.page_object_number,
                    vec![render_options(device_pixel_width, true)],
                )
                .await?;
                Ok(Some(image_to_preview(&image)?))
            }
            .await;
            close(&doc, body).await
        })
    }

    /// `ghostProvider(bytes, fallback, cacheKey)`: a resolution-aware ghost
    /// source for placement bytes. The page renders at the requested width,
    /// cached per width under `cache_key` when given; a render that fails is
    /// reported and answers the thumbnail.
    pub(crate) fn ghost_provider(
        &self,
        bytes: Option<Vec<u8>>,
        fallback: Option<StampAssetPreview>,
        cache_key: Option<&str>,
    ) -> Rc<dyn Fn(f64) -> Promise<Option<StampAssetPreview>>> {
        let local: GhostCache = match cache_key {
            None => Rc::new(RefCell::new(HashMap::new())),
            Some(key) => {
                let local = self.0.binaries.ghost_renders.borrow().get(key).cloned();
                let local = local.unwrap_or_else(|| Rc::new(RefCell::new(HashMap::new())));
                self.0.binaries.ghost_renders.borrow_mut().insert(key.to_string(), local.clone());
                local
            }
        };
        let this = self.clone();
        Rc::new(move |device_pixel_width: f64| {
            let key = width_key(device_pixel_width);
            if let Some(pending) = local.borrow().get(&key) {
                return pending.clone();
            }
            // `new Uint8Array(bytes)`: an extraction that answered nothing is
            // an empty copy.
            let rendering =
                this.render_ghost(bytes.clone().unwrap_or_default(), device_pixel_width);
            let fallback = fallback.clone();
            // `.catch((error) => { console.warn(...); return fallback; })`
            let pending = Promise::spawn(async move {
                match rendering.reaction().await {
                    Ok(preview) => Ok(preview),
                    Err(error) => {
                        crate::console::warn(vec![
                            Value::String(
                                "[stamp] ghost render failed, using the thumbnail:".into(),
                            ),
                            error.to_canonical(),
                        ]);
                        Ok(fallback)
                    }
                }
            });
            local.borrow_mut().insert(key, pending.clone());
            pending
        })
    }

    /// [`ghost_provider`](Self::ghost_provider) as the annotation plugin
    /// takes it: the preview as JSON, settling in the turn the render does.
    pub(crate) fn annotation_ghost(
        &self,
        bytes: Option<Vec<u8>>,
        fallback: Option<StampAssetPreview>,
        cache_key: Option<&str>,
    ) -> StampPreviewProvider {
        let provider = self.ghost_provider(bytes, fallback, cache_key);
        Rc::new(move |width: f64| {
            provider(width).convert(|preview| Ok(preview.as_ref().map(StampAssetPreview::to_json)))
        })
    }
}
