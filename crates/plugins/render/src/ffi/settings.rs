//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

use crate::contract::{SnapshotBoxLook, SnapshotConfig, TilesSetting};
use crate::paint_plan::{PrefetchOptions, RenderFormat, TilesSettings};
use serde_json::json;
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum RenderEncodingFormat {
    Png,
    Webp,
    Bmp,
}
impl From<RenderEncodingFormat> for RenderFormat {
    fn from(format: RenderEncodingFormat) -> Self {
        match format {
            RenderEncodingFormat::Png => Self::Png,
            RenderEncodingFormat::Webp => Self::Webp,
            RenderEncodingFormat::Bmp => Self::Bmp,
        }
    }
}
impl From<RenderFormat> for RenderEncodingFormat {
    fn from(format: RenderFormat) -> Self {
        match format {
            RenderFormat::Png => Self::Png,
            RenderFormat::Webp => Self::Webp,
            RenderFormat::Bmp => Self::Bmp,
        }
    }
}
impl From<Quantize> for RenderQuantize {
    fn from(quantize: Quantize) -> Self {
        match quantize {
            Quantize::Exact => Self::Exact,
            Quantize::Ladder(widths) => Self::Ladder { widths },
        }
    }
}
impl From<RenderQuantize> for Quantize {
    fn from(quantize: RenderQuantize) -> Self {
        match quantize {
            RenderQuantize::Exact => Self::Exact,
            RenderQuantize::Ladder { widths } => Self::Ladder(widths),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderPrefetchOptions {
    #[uniffi(default = None)]
    pub margin: Option<f64>,
    #[uniffi(default = None)]
    pub velocity_bias: Option<bool>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderTilesSettings {
    #[uniffi(default = None)]
    pub size: Option<f64>,
    #[uniffi(default = None)]
    pub quantize: Option<RenderQuantize>,
    #[uniffi(default = None)]
    pub max_scale: Option<f64>,
    #[uniffi(default = None)]
    pub bleed_px: Option<f64>,
    #[uniffi(default = None)]
    pub engage_ratio: Option<f64>,
    #[uniffi(default = None)]
    pub prefetch: Option<RenderPrefetchOptions>,
    #[uniffi(default = None)]
    pub settle_ms: Option<f64>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderTilesOptions {
    #[uniffi(default = None)]
    pub size: Option<f64>,
    #[uniffi(default = None)]
    pub quantize: Option<RenderQuantize>,
    #[uniffi(default = None)]
    pub max_scale: Option<f64>,
    #[uniffi(default = None)]
    pub bleed_px: Option<f64>,
    #[uniffi(default = None)]
    pub engage_ratio: Option<f64>,
    /// Clear the optional ratio; omission preserves it.
    #[uniffi(default = false)]
    pub clear_engage_ratio: bool,
    #[uniffi(default = None)]
    pub prefetch: Option<RenderPrefetchOptions>,
    #[uniffi(default = None)]
    pub settle_ms: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RenderTilesSetting {
    Off,
    On { options: RenderTilesSettings },
}
/// Changes to the tile plane: disable it, or merge the named strategy fields.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RenderTilesPatch {
    Off,
    On { options: RenderTilesOptions },
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderSnapshotBox {
    #[uniffi(default = None)]
    pub fill: Option<String>,
    #[uniffi(default = None)]
    pub stroke: Option<String>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderSnapshotSettings {
    pub scale: f64,
    pub box_look: RenderSnapshotBox,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderSnapshotPatch {
    #[uniffi(default = None)]
    pub scale: Option<f64>,
    /// A whole value: omitted colors in a supplied box clear to the accent.
    #[uniffi(default = None)]
    pub box_look: Option<RenderSnapshotBox>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderSettings {
    pub full_page: RenderFullPageOptions,
    pub tiles: RenderTilesSetting,
    /// Stored encoding preference. The native host returns raw pixels and does not encode this format.
    pub format: Option<RenderEncodingFormat>,
    /// Stored encoded-image quality. It does not change native raw pixels.
    pub quality: Option<f64>,
    pub slow_after_ms: f64,
    pub debug: bool,
    pub snapshot: RenderSnapshotSettings,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderSettingsPatch {
    #[uniffi(default = None)]
    pub full_page: Option<RenderFullPageOptions>,
    #[uniffi(default = None)]
    pub tiles: Option<RenderTilesPatch>,
    /// Stored encoding preference. The native host returns raw pixels and does not provide an encoder.
    #[uniffi(default = None)]
    pub format: Option<RenderEncodingFormat>,
    #[uniffi(default = false)]
    pub clear_format: bool,
    #[uniffi(default = None)]
    pub quality: Option<f64>,
    #[uniffi(default = false)]
    pub clear_quality: bool,
    #[uniffi(default = None)]
    pub slow_after_ms: Option<f64>,
    #[uniffi(default = None)]
    pub debug: Option<bool>,
    #[uniffi(default = None)]
    pub snapshot: Option<RenderSnapshotPatch>,
}
impl From<RenderTilesOptions> for TilesSettings {
    fn from(options: RenderTilesOptions) -> Self {
        Self {
            size: options.size,
            quantize: options.quantize.map(Into::into),
            max_scale: options.max_scale,
            bleed_px: options.bleed_px,
            engage_ratio: options.engage_ratio,
            settle_ms: options.settle_ms,
            prefetch: options.prefetch.map(|prefetch| PrefetchOptions {
                margin: prefetch.margin,
                velocity_bias: prefetch.velocity_bias,
            }),
        }
    }
}
impl From<TilesSettings> for RenderTilesSettings {
    fn from(options: TilesSettings) -> Self {
        Self {
            size: options.size,
            quantize: options.quantize.map(Into::into),
            max_scale: options.max_scale,
            bleed_px: options.bleed_px,
            engage_ratio: options.engage_ratio,
            settle_ms: options.settle_ms,
            prefetch: options.prefetch.map(|prefetch| RenderPrefetchOptions {
                margin: prefetch.margin,
                velocity_bias: prefetch.velocity_bias,
            }),
        }
    }
}
impl RenderSettingsPatch {
    fn into_json(self) -> Value {
        let clear_engage_ratio = matches!(&self.tiles, Some(RenderTilesPatch::On { options }) if options.clear_engage_ratio);
        let config = crate::RenderConfig {
            full_page: self.full_page.map(|options| FullPageSettings {
                max_width: options.max_width,
                quantize: options.quantize.map(Into::into),
            }),
            tiles: self.tiles.map(|tiles| match tiles {
                RenderTilesPatch::Off => TilesSetting::Off,
                RenderTilesPatch::On { options } => TilesSetting::On(options.into()),
            }),
            format: if self.clear_format {
                Some(None)
            } else {
                self.format.map(|format| Some(format.into()))
            },
            quality: if self.clear_quality { Some(None) } else { self.quality.map(Some) },
            slow_after_ms: self.slow_after_ms,
            debug: self.debug,
            snapshot: self.snapshot.map(|snapshot| SnapshotConfig {
                scale: snapshot.scale,
                box_look: snapshot
                    .box_look
                    .map(|look| SnapshotBoxLook { fill: look.fill, stroke: look.stroke }),
            }),
        };
        let mut changes = json!(config);
        if clear_engage_ratio {
            changes["tiles"]["engageRatio"] = Value::Null;
        }
        changes
    }
}
fn required<T>(value: Option<T>, field: &str) -> Result<T, Error> {
    value.ok_or_else(|| {
        Error::named("InvalidState", format!("[render] missing setting {field}"))
            .with("code", "invalid-state")
    })
}
impl TryFrom<crate::RenderSettings> for RenderSettings {
    type Error = Error;
    fn try_from(settings: crate::RenderSettings) -> Result<Self, Error> {
        let full_page = required(settings.full_page, "fullPage")?;
        let snapshot = required(settings.snapshot, "snapshot")?;
        let look = required(snapshot.box_look, "snapshot.box")?;
        Ok(Self {
            full_page: RenderFullPageOptions {
                max_width: full_page.max_width,
                quantize: full_page.quantize.map(Into::into),
            },
            tiles: match required(settings.tiles, "tiles")? {
                TilesSetting::Off => RenderTilesSetting::Off,
                TilesSetting::On(options) => RenderTilesSetting::On { options: options.into() },
            },
            format: settings.format.flatten().map(Into::into),
            quality: settings.quality.flatten(),
            slow_after_ms: required(settings.slow_after_ms, "slowAfterMs")?,
            debug: required(settings.debug, "debug")?,
            snapshot: RenderSnapshotSettings {
                scale: required(snapshot.scale, "snapshot.scale")?,
                box_look: RenderSnapshotBox { fill: look.fill, stroke: look.stroke },
            },
        })
    }
}
#[uniffi::export]
impl RenderHandle {
    pub async fn settings(&self) -> Result<RenderSettings, ViewerError> {
        self.bound.call(|bound| RenderSettings::try_from(open(bound)?.get_settings())).await
    }
    pub async fn update_settings(&self, patch: RenderSettingsPatch) -> Result<(), ViewerError> {
        self.bound
            .call(move |bound| {
                open(bound)?.update_settings_json(&patch.into_json());
                Ok(())
            })
            .await
    }
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.bound
            .call(|bound| {
                open(bound)?.reset_settings();
                Ok(())
            })
            .await
    }
}
