//! `host-contract.ts`: the host lens. What view layers need to paint: the
//! conforming render door and its source keys, the resolved paint settings,
//! a per-view tile surface whose reads are pure, and the owners' appearance
//! pictures. The TypeScript widens the public token's type for hosts; Rust
//! has one capability type, so the host methods sit on
//! [`RenderCapability`](crate::RenderCapability) beside the public ones, and
//! application code should keep to the public ones.
//!
//! [`RenderHost`] is the one thing here the TypeScript does not have: the
//! console the plugin writes to, as an input. Time (timers, frames) is the
//! kernel's clock (`ctx.clock`), not the host's.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::PluginErrorInfo;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::contract::{PageLayerOptions, ser_page};
use crate::raster_store::ReleaseImage;

/// `PaintSettings`: resolved config the view layers need. The same `Rc`
/// until a setting it reads changes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintSettings {
    /// Whether the tile plane is enabled at all (`tiles: false` opts out).
    pub tiles: bool,
}

/// `RenderSourceOptions` with the operation's `signal`.
#[derive(Clone, Default)]
pub struct RenderSourceOptions {
    /// Device px per PDF point (the page transform's `renderScale`).
    pub scale: f64,
    /// The view the picture is for (the page context's `view`).
    pub view: Option<String>,
    pub layers: PageLayerOptions,
    pub signal: Option<AbortSignal>,
}

/// `AppearanceFamily`: whose pictures, a page's annotations or its form
/// widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceFamily {
    Annotations,
    Widgets,
}

impl AppearanceFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            AppearanceFamily::Annotations => "annotations",
            AppearanceFamily::Widgets => "widgets",
        }
    }
}

/// `AppearanceItem`: one of an owner's items on a page, at the version of
/// its picture. `reference` is the item's `AnnotationRef` JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceItem {
    pub reference: Value,
    /// Moves when the engine draws the item again (`pictureVersion`).
    pub version: f64,
}

/// `AppearancesOptions`.
#[derive(Clone)]
pub struct AppearancesOptions {
    pub family: AppearanceFamily,
    /// Device px per PDF point, conformed through `get_appearance_scale`.
    pub scale: f64,
    /// The owner's items on the page whose pictures it wants.
    pub items: Vec<AppearanceItem>,
    pub signal: Option<AbortSignal>,
}

/// `AppearancePicture`: the engine's picture of an item in one of its
/// states (`AnnotationAppearanceImage`, as the engine answered it), and the
/// version of the item it shows. Serializes as the picture with `version`
/// added last, as the TypeScript's spread does.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearancePicture {
    pub picture: Value,
    pub version: f64,
}

impl AppearancePicture {
    /// The picture's `ref`.
    pub fn reference(&self) -> &Value {
        self.picture.get("ref").unwrap_or(&Value::Null)
    }
}

impl Serialize for AppearancePicture {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut picture = match &self.picture {
            Value::Object(members) => members.clone(),
            _ => serde_json::Map::new(),
        };
        picture.insert("version".into(), serde_json::json!(self.version));
        Value::Object(picture).serialize(s)
    }
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

/// The console the TypeScript writes to, as a host input.
///
/// `warn` and `debug` are `console.warn` and `console.debug`: the one-shot
/// misconfiguration hints and, with `debug: true`, the tile diagnostics. The
/// default writes both to stderr.
///
/// `release_image` has no TypeScript counterpart: a host that keeps each
/// engine image on its own side (behind a `$host` id) hears when the raster
/// store's last handle to one drops, with the handle's JSON, and lets go of
/// it. `None`, the default, keeps nothing to release.
#[derive(Clone)]
pub struct RenderHost {
    pub warn: Rc<dyn Fn(&str)>,
    pub debug: Rc<dyn Fn(&str)>,
    pub release_image: Option<ReleaseImage>,
}

impl Default for RenderHost {
    fn default() -> Self {
        RenderHost {
            warn: Rc::new(|message| eprintln!("{message}")),
            debug: Rc::new(|message| eprintln!("{message}")),
            release_image: None,
        }
    }
}
