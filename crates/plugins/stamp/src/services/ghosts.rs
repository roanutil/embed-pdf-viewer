//! `services/ghosts.ts`: ghost renders, one per size bucket per asset,
//! rendered lazily on the asset engine the first time the ghost is shown at
//! that size and kept until the asset goes away. Materialized (dynamic)
//! placements cache only for their own arm (no cache key).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_annotation::{ArmedStampPreview, StampPreviewProvider};
use serde_json::Value;

use crate::contract::StampAssetPreview;
use crate::controller::{GhostCache, StampCapability};
use crate::promise::await_engine;
use crate::services::asset_engine::{call, close, image_to_preview, page_call, render_options};
use crate::value::{get, num};

impl StampCapability {
    /// `renderGhost(bytes, devicePixelWidth)`: the first page at that width.
    async fn render_ghost(
        &self,
        bytes: Vec<u8>,
        device_pixel_width: f64,
    ) -> Result<Option<StampAssetPreview>, Error> {
        let doc = rt::returned(self.open_asset_document(bytes)).await?;
        let body = async {
            let layout = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let first =
                get(&layout, "pages").and_then(Value::as_array).and_then(|p| p.first()).cloned();
            let Some(first) = first else { return Ok(None) };
            let pon =
                get(&first, "ref").and_then(|r| num(r, "pageObjectNumber")).unwrap_or(f64::NAN);
            doc.page_index(pon as u32)?;
            let render = page_call(
                &doc,
                "page.render.image",
                pon as u32,
                vec![render_options(device_pixel_width, true)],
            );
            Ok(Some(image_to_preview(&await_engine(render).await?)?))
        }
        .await;
        close(&doc, body).await
    }

    /// The render for one width, from `local` or started now: a failure is
    /// reported and answers the fallback.
    pub(crate) fn ghost_render(
        &self,
        local: &GhostCache,
        bytes: &Option<Vec<u8>>,
        fallback: &Option<StampAssetPreview>,
        device_pixel_width: f64,
    ) -> Promise<Option<StampAssetPreview>> {
        let key = device_pixel_width.to_bits();
        if let Some(pending) = local.borrow().get(&key) {
            return pending.clone();
        }
        let rendering = {
            let (this, bytes) = (self.clone(), bytes.clone().unwrap_or_default());
            Promise::spawn(async move { this.render_ghost(bytes, device_pixel_width).await })
        };
        let (this, fallback) = (self.clone(), fallback.clone());
        // `.catch((error) => { warn(...); return fallback; })`
        let pending = Promise::spawn(async move {
            match rendering.await {
                Ok(preview) => Ok(preview),
                Err(error) => {
                    this.warn("[stamp] ghost render failed, using the thumbnail:", Some(&error));
                    Ok(fallback)
                }
            }
        });
        local.borrow_mut().insert(key, pending.clone());
        pending
    }

    /// The cache under `cache_key`, or a fresh one for a key-less provider.
    pub(crate) fn ghost_cache(&self, cache_key: Option<&str>) -> GhostCache {
        match cache_key {
            None => Rc::new(RefCell::new(HashMap::new())),
            Some(key) => {
                let local = self
                    .0
                    .ghost_renders
                    .borrow()
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| Rc::new(RefCell::new(HashMap::new())));
                self.0.ghost_renders.borrow_mut().insert(key.to_string(), local.clone());
                local
            }
        }
    }

    /// `ghostProvider(bytes, fallback, cacheKey)`: a resolution-aware ghost
    /// source for placement bytes.
    pub(crate) fn ghost_provider(
        &self,
        bytes: Option<Vec<u8>>,
        fallback: Option<StampAssetPreview>,
        cache_key: Option<&str>,
    ) -> StampPreviewProvider {
        let local = self.ghost_cache(cache_key);
        let this = self.clone();
        Rc::new(move |device_pixel_width: f64| {
            let pending = this.ghost_render(&local, &bytes, &fallback, device_pixel_width);
            let (out, resolver) = Promise::pending();
            pending.on_settle(move |result| {
                resolver.settle(result.clone().map(|preview| {
                    preview.map(|p| ArmedStampPreview {
                        bytes: p.bytes.unwrap_or_default(),
                        mime_type: p.mime_type,
                    })
                }))
            });
            out
        })
    }
}
