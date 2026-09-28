//! `host-contract.ts`: the host lens. What view layers need to paint: the
//! conforming render door and its source keys, the resolved paint settings,
//! and a per-view tile surface whose reads are pure. The TypeScript widens
//! the public token's type for hosts; Rust has one capability type, so the
//! host methods sit on [`RenderCapability`](crate::RenderCapability) beside
//! the public ones, and application code should keep to the public ones.
//!
//! [`RenderHost`] is the one thing here the TypeScript does not have: the
//! browser globals the plugin reaches for, as inputs.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::PluginErrorInfo;
use serde::{Deserialize, Serialize};

use crate::contract::ser_page;
use crate::raster_store::ReleaseImage;

/// `PaintSettings`: resolved config the view layers need.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintSettings {
    /// Tile arrival cross-fade (ms); 0 is a hard pop.
    pub fade_ms: f64,
    /// Whether the tile plane is enabled at all.
    pub tiles: bool,
}

/// `RenderSourceOptions` with the operation's `signal`.
#[derive(Clone, Default)]
pub struct RenderSourceOptions {
    /// Device px per PDF point (the page transform's `renderScale`).
    pub scale: f64,
    /// Bake annotations; `None` is true.
    pub include_annotations: Option<bool>,
    pub signal: Option<AbortSignal>,
}

/// `RenderCompletedEvent`: a base raster resolved.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderCompletedEvent {
    #[serde(serialize_with = "ser_page")]
    pub page: PageRef,
    pub key: String,
}

/// `RenderFailedEvent`: a base raster failed (not an abort, not a close).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderFailedEvent {
    #[serde(serialize_with = "ser_page")]
    pub page: PageRef,
    pub key: String,
    pub error: PluginErrorInfo,
}

/// The browser globals the TypeScript reaches for, as host inputs.
///
/// `request_frame` is `requestAnimationFrame`: the controller coalesces tile
/// arrivals into one re-plan per page per frame. Where there is no
/// `requestAnimationFrame` the TypeScript falls back to `setTimeout(cb, 16)`,
/// and so does [`RenderHost::default`], on the kernel's clock. `warn` and
/// `debug` are `console.warn` and `console.debug`: the one-shot
/// misconfiguration hints and, with `debug: true`, the tile diagnostics. The
/// default writes both to stderr.
///
/// `release_image` has no TypeScript counterpart: a host that keeps each
/// engine image on its own side (the eval build's engine bridge, behind a
/// `$host` id) hears when the raster store's last handle to one drops, with
/// the handle's JSON, and lets go of it. `None`, the default, keeps nothing
/// to release.
#[derive(Clone)]
pub struct RenderHost {
    pub request_frame: Rc<dyn Fn(Box<dyn FnOnce()>)>,
    pub warn: Rc<dyn Fn(&str)>,
    pub debug: Rc<dyn Fn(&str)>,
    pub release_image: Option<ReleaseImage>,
}

impl Default for RenderHost {
    fn default() -> Self {
        RenderHost {
            request_frame: Rc::new(|frame| {
                epdf_kernel::rt::set_timeout(16.0, frame);
            }),
            warn: Rc::new(|message| eprintln!("{message}")),
            debug: Rc::new(|message| eprintln!("{message}")),
            release_image: None,
        }
    }
}
