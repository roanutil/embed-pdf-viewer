//! `types.ts`: the ledger state, its actions, and the capability's argument
//! and result shapes. [`RenderHost`] is the one thing here the TypeScript
//! does not have: the browser globals the plugin reaches for, as inputs.

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_kernel::signal::AbortSignal;
use serde::{Deserialize, Serialize};

/// `InvalidateScope`: `content` (the page itself changed, everything is
/// stale) or `annotations` (only baked appearances changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvalidateScope {
    Content,
    Annotations,
}

/// `RenderState`: per-page versions of the two raster products, and the
/// tile wake-up counter. Keys are page object numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderState {
    /// Base-raster versions, bumped by content facts.
    pub content_epochs: BTreeMap<u32, u64>,
    /// Appearance versions, bumped by annotation facts.
    pub annotated_epochs: BTreeMap<u32, u64>,
    /// Tile paint-plan wake-ups; the value carries no meaning.
    pub paint_versions: BTreeMap<u32, u64>,
}

/// `RenderAction`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RenderAction {
    #[serde(rename = "INVALIDATE")]
    Invalidate { scope: InvalidateScope, pons: Vec<u32> },
    #[serde(rename = "PAINT_ADVANCED")]
    PaintAdvanced { pon: u32 },
}

/// `PaintSettings`: what the view layers need.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintSettings {
    /// Tile arrival cross-fade (ms); 0 is a hard pop.
    pub fade_ms: f64,
    /// Whether the tile plane is enabled at all.
    pub tiles: bool,
}

/// `RenderPageOptions`.
#[derive(Clone, Default)]
pub struct RenderPageOptions {
    /// Device px per PDF point.
    pub scale: f64,
    /// Bake annotations into the bitmap; `None` is true.
    pub include_annotations: Option<bool>,
    /// Abort this consumer (camera moved, layer unmounted).
    pub signal: Option<AbortSignal>,
}

/// `invalidate`'s options: `pons: None` is every page, `scope: None` is
/// `content`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InvalidateOptions {
    #[serde(default)]
    pub pons: Option<Vec<u32>>,
    #[serde(default)]
    pub scope: Option<InvalidateScope>,
}

/// The browser globals the TypeScript reaches for, as host inputs.
///
/// `request_frame` is `requestAnimationFrame`: the capability coalesces tile
/// arrivals into one `PAINT_ADVANCED` per page per frame. Where there is no
/// `requestAnimationFrame` the TypeScript falls back to `setTimeout(cb, 16)`,
/// and so does [`RenderHost::default`], on the kernel's clock. `warn` and
/// `debug` are `console.warn` and `console.debug`: the one-shot
/// misconfiguration hints and, with `debug: true`, the tile diagnostics. The
/// default writes both to stderr.
#[derive(Clone)]
pub struct RenderHost {
    pub request_frame: Rc<dyn Fn(Box<dyn FnOnce()>)>,
    pub warn: Rc<dyn Fn(&str)>,
    pub debug: Rc<dyn Fn(&str)>,
}

impl Default for RenderHost {
    fn default() -> Self {
        RenderHost {
            request_frame: Rc::new(|frame| {
                epdf_kernel::rt::set_timeout(16.0, frame);
            }),
            warn: Rc::new(|message| eprintln!("{message}")),
            debug: Rc::new(|message| eprintln!("{message}")),
        }
    }
}
