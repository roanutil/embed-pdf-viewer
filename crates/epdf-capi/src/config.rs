use serde::Deserialize;

use crate::abi::Status;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Limits {
    pub max_pending_jobs: u32,
    pub max_json_bytes: u64,
    pub max_payload_bytes: u64,
    pub max_queued_input_bytes: u64,
    pub max_retained_input_bytes: u64,
    pub max_result_bytes: u64,
    pub max_retained_result_bytes: u64,
    pub max_output_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pending_jobs: 16,
            max_json_bytes: 8 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
            max_queued_input_bytes: 256 * 1024 * 1024,
            max_retained_input_bytes: 1024 * 1024 * 1024,
            max_result_bytes: 256 * 1024 * 1024,
            max_retained_result_bytes: 512 * 1024 * 1024,
            max_output_pixels: 16_777_216,
        }
    }
}

impl Limits {
    pub(crate) fn validate(&self) -> Result<(), Status> {
        let bytes = [
            self.max_json_bytes,
            self.max_payload_bytes,
            self.max_queued_input_bytes,
            self.max_retained_input_bytes,
            self.max_result_bytes,
            self.max_retained_result_bytes,
        ];
        if self.max_pending_jobs == 0
            || self.max_pending_jobs > u16::MAX.into()
            || bytes.iter().any(|n| *n == 0 || *n > u32::MAX.into())
            || self.max_output_pixels == 0
            || self.max_output_pixels > u64::from(u32::MAX) / 4
        {
            return Err(Status::InvalidArgument);
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub limits: Limits,
    pub startup_fonts: Vec<StartupFont>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct StartupFont {
    pub key: String,
    #[serde(default)]
    pub family_name: String,
    #[serde(default)]
    pub weight: i32,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub fallback: bool,
    pub payload_index: usize,
}
