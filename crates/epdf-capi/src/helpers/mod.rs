//! The pure helpers behind `epdf_helper_call`: the engine's page-space text
//! layout and character map, and the page/view transform. Each takes owned
//! JSON arguments, validates them before calling an algorithm that assumes
//! valid data, and answers `{"result": ...}` or a refusal. Helpers run on the
//! caller's thread; they never reach a Host session or PDFium.

mod text;

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::host::HostResponse;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::abi::{Failure, Status};

/// The largest argument span a helper accepts.
pub(crate) const MAX_ARGS_BYTES: u64 = 8 * 1024 * 1024;
/// The largest framed result a helper returns.
pub(crate) const MAX_RESULT_BYTES: u64 = 256 * 1024 * 1024;

/// Runs one helper operation on its argument bytes.
pub(crate) fn call(operation: u32, args: &[u8]) -> Result<HostResponse, Failure> {
    match operation {
        1 => text::segments(args),
        2 => text::character_quad(args),
        3 => text::slice_text(args),
        4 => text::text_offsets_to_chars(args),
        5 => text::character_boundary_to_text(args),
        6 => Err(Failure {
            status: Status::RuntimeUnavailable,
            response: crate::abi::response_error(EngineError::with_message(
                EngineErrorCode::NotImplemented,
                "helper is unavailable in this build",
            )),
        }),
        _ => Err(Failure::new(Status::InvalidArgument, "unknown helper operation")),
    }
}

fn invalid(message: impl AsRef<str>) -> Failure {
    Failure::new(Status::InvalidArgument, message.as_ref())
}

fn parse<T: DeserializeOwned>(args: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(args).map_err(|e| invalid(format!("invalid helper arguments: {e}")))
}

fn answer<T: Serialize>(result: &T) -> Result<HostResponse, Failure> {
    #[derive(Serialize)]
    struct Answer<'a, T> {
        result: &'a T,
    }
    let json = serde_json::to_string(&Answer { result })
        .map_err(|e| invalid(format!("unserializable helper result: {e}")))?;
    Ok(HostResponse { json, payloads: Vec::new() })
}
