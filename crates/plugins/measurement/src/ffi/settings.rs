//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementSettings {
    pub default_scale: MeasurementDefaultScale,
    pub presets: Vec<ScalePreset>,
}
#[uniffi::export]
impl MeasurementHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<MeasurementSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                let settings = settings?;
                let default_scale = match settings.default_scale {
                    DefaultScale::Metric => MeasurementDefaultScale::Metric,
                    DefaultScale::Imperial => MeasurementDefaultScale::Imperial,
                    DefaultScale::Measure(value) => MeasurementDefaultScale::Measure {
                        measure: serde_json::from_value(value)
                            .map_err(|error| invalid_state("the default scale", error))?,
                    },
                };
                Ok(MeasurementSettings { default_scale, presets: settings.presets })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: MeasurementConfig) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(crate::MeasurementConfig::try_from(patch)?));
                Ok(())
            })
            .await
    }
    /// Restore the values supplied when the plugin was registered.
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.reset_settings();
                Ok(())
            })
            .await
    }
}
