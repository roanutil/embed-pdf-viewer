/// Status values shared with the public C header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Ok = 0,
    Timeout = 1,
    InvalidArgument = 2,
    InvalidHandle = 3,
    Closed = 4,
    Busy = 5,
    LimitExceeded = 6,
    Panicked = 7,
    RuntimeUnavailable = 8,
    AlreadyTaken = 9,
}

/// Owned handles transferred by a completed job.
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct Reply {
    pub result: u64,
    pub document: u64,
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub status: Status,
    pub response: epdf_engine::host::HostResponse,
}

impl Failure {
    pub(crate) fn new(status: Status, message: &str) -> Self {
        use epdf_engine::error::{EngineError, EngineErrorCode};
        let code = match status {
            Status::LimitExceeded => EngineErrorCode::PayloadTooLarge,
            Status::Closed => EngineErrorCode::DocNotOpen,
            Status::Panicked | Status::RuntimeUnavailable => EngineErrorCode::RuntimeUnavailable,
            _ => EngineErrorCode::InvalidArg,
        };
        Self { status, response: response_error(EngineError::with_message(code, message)) }
    }
}

pub(crate) fn response_error(
    error: epdf_engine::error::EngineError,
) -> epdf_engine::host::HostResponse {
    epdf_engine::host::HostResponse {
        json: serde_json::json!({ "error": error }).to_string(),
        payloads: Vec::new(),
    }
}
