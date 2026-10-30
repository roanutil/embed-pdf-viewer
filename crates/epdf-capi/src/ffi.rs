use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::host::Host;

use crate::abi::{Failure, Reply, Status, response_error};
use crate::buffers::{ByteBudget, OwnedResult, Reservation, unpack_payloads};
use crate::config::Options;
use crate::engine::Engine;
use crate::handles::{Kind, Registry};
use crate::jobs::Job;
use crate::startup::initialize_fonts;

struct RuntimeState {
    _host: Host,
    _fonts: Reservation,
    _input_budget: Arc<ByteBudget>,
}

type NativeEngine = Engine<RuntimeState>;

fn engines() -> &'static Mutex<Registry<Arc<NativeEngine>>> {
    static STORE: OnceLock<Mutex<Registry<Arc<NativeEngine>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Registry::new(Kind::Engine)))
}

fn jobs() -> &'static Mutex<Registry<Arc<Job>>> {
    static STORE: OnceLock<Mutex<Registry<Arc<Job>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Registry::new(Kind::Job)))
}

fn results() -> &'static Mutex<Registry<Arc<OwnedResult>>> {
    static STORE: OnceLock<Mutex<Registry<Arc<OwnedResult>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Registry::new(Kind::Result)))
}

fn documents() -> &'static Mutex<Registry<String>> {
    static STORE: OnceLock<Mutex<Registry<String>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Registry::new(Kind::Document)))
}

fn protect(call: impl FnOnce() -> Status) -> u32 {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or(Status::Panicked) as u32
}

unsafe fn span<'a>(ptr: *const u8, len: u64, limit: u64) -> Result<&'a [u8], Status> {
    if len > limit || len > isize::MAX as u64 {
        return Err(Status::LimitExceeded);
    }
    let len = usize::try_from(len).map_err(|_| Status::LimitExceeded)?;
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(Status::InvalidArgument);
    }
    // SAFETY: validated size and the export's readable-span contract.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn store_result(result: Arc<OwnedResult>) -> Result<u64, Status> {
    results().lock().unwrap_or_else(|p| p.into_inner()).insert(0, result)
}

unsafe fn report(failure: Failure, out: *mut u64) -> Status {
    if !out.is_null() {
        let result = OwnedResult::from_response(&failure.response, 8 * 1024 * 1024, None)
            .and_then(store_result);
        if let Ok(result) = result {
            // SAFETY: the export has initialized and validated this output.
            unsafe { out.write(result) };
        }
    }
    failure.status
}

fn engine(id: u64) -> Result<Arc<NativeEngine>, Status> {
    engines().lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
}

/// The packed ABI major/minor version, or zero if the boundary fails.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_abi_version() -> u32 {
    catch_unwind(|| 0x0001_0000).unwrap_or(0)
}

/// Creates an owner-thread engine with immutable startup font configuration.
///
/// # Safety
/// Input spans are readable for this call. Output pointers are valid, aligned,
/// distinct and do not overlap inputs. Null input pointers require zero length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_engine_create(
    options: *const u8,
    options_len: u64,
    payloads: *const u8,
    payloads_len: u64,
    out_engine: *mut u64,
    out_error: *mut u64,
) -> u32 {
    protect(|| {
        // SAFETY: the caller supplies valid output storage when non-null.
        unsafe {
            if !out_engine.is_null() {
                out_engine.write(0);
            }
            if !out_error.is_null() {
                out_error.write(0);
            }
        }
        let outcome = (|| {
            if out_engine.is_null() || out_error.is_null() {
                return Err(Failure::new(Status::InvalidArgument, "missing engine output"));
            }
            let bytes = unsafe { span(options, options_len, 8 * 1024 * 1024) }
                .map_err(|s| Failure::new(s, "invalid engine options span"))?;
            let options: Options = if bytes.is_empty() {
                Options::default()
            } else {
                serde_json::from_slice(bytes)
                    .map_err(|_| Failure::new(Status::InvalidArgument, "invalid engine options"))?
            };
            options.limits.validate().map_err(|s| Failure::new(s, "invalid engine limits"))?;
            if options_len > options.limits.max_json_bytes {
                return Err(Failure::new(
                    Status::LimitExceeded,
                    "engine options exceed the JSON budget",
                ));
            }
            let bytes = unsafe { span(payloads, payloads_len, options.limits.max_payload_bytes) }
                .map_err(|s| Failure::new(s, "invalid startup payload span"))?;
            let budget = ByteBudget::new(options.limits.max_retained_input_bytes);
            let retained = budget
                .reserve(payloads_len)
                .map_err(|s| Failure::new(s, "startup fonts exceed the retained-input budget"))?;
            let payloads = unpack_payloads(bytes, options.limits.max_payload_bytes as usize)
                .map_err(|s| Failure::new(s, "invalid startup payload frame"))?;
            let limits = options.limits.clone();
            let engine = Engine::start(limits, move || {
                let mut host = Host::new();
                initialize_fonts(&options, payloads, |request, payloads| {
                    host.call(request, payloads)
                })?;
                Ok(RuntimeState { _host: host, _fonts: retained, _input_budget: budget })
            })?;
            engines()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(0, engine)
                .map_err(|s| Failure::new(s, "engine registry is full"))
        })();
        match outcome {
            Ok(id) => {
                unsafe { out_engine.write(id) };
                Status::Ok
            }
            Err(error) => unsafe { report(error, out_error) },
        }
    })
}

/// Submits a bounded operation and transfers a job handle on acceptance.
///
/// # Safety
/// Input spans remain readable for the call. Output pointers are aligned,
/// distinct writable storage, without overlap with the input spans.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_engine_submit(
    engine_id: u64,
    document: u64,
    operation: u32,
    args: *const u8,
    args_len: u64,
    payloads: *const u8,
    payloads_len: u64,
    out_job: *mut u64,
    out_error: *mut u64,
) -> u32 {
    protect(|| {
        unsafe {
            if !out_job.is_null() {
                out_job.write(0);
            }
            if !out_error.is_null() {
                out_error.write(0);
            }
        }
        let outcome = (|| {
            if out_job.is_null() || out_error.is_null() {
                return Err(Status::InvalidArgument);
            }
            let engine = engine(engine_id)?;
            let size = args_len.checked_add(payloads_len).ok_or(Status::LimitExceeded)?;
            let admission = engine.reserve(size)?;
            if !(1..=7).contains(&operation) {
                return Err(Status::InvalidArgument);
            }
            if operation == 1 {
                if document != 0 {
                    return Err(Status::InvalidHandle);
                }
            } else {
                documents()
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_for_parent(document, engine_id)?;
            }
            let bytes = unsafe { span(args, args_len, engine.limits.max_json_bytes) }?;
            let args: serde_json::Value = if bytes.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_slice(bytes).map_err(|_| Status::InvalidArgument)?
            };
            let fields = args.as_object().ok_or(Status::InvalidArgument)?;
            for key in [
                "kind",
                "docId",
                "layerName",
                "authority",
                "effect",
                "scope",
                "identity",
                "jobId",
                "opId",
                "objectNumberFloor",
            ] {
                if fields.contains_key(key) {
                    return Err(Status::InvalidArgument);
                }
            }
            let bytes = unsafe { span(payloads, payloads_len, engine.limits.max_payload_bytes) }?;
            let payloads = unpack_payloads(bytes, engine.limits.max_payload_bytes as usize)?;
            let job = engine.submit(admission, move |_, _| {
                let _inputs = (args, payloads);
                Ok(response_error(EngineError::with_message(
                    EngineErrorCode::NotImplemented,
                    "engine operation is unavailable in this build",
                )))
            })?;
            let inserted =
                jobs().lock().unwrap_or_else(|p| p.into_inner()).insert(engine_id, job.clone());
            match inserted {
                Ok(id) => Ok(id),
                Err(status) => {
                    job.abandon();
                    Err(status)
                }
            }
        })();
        match outcome {
            Ok(id) => {
                unsafe { out_job.write(id) };
                Status::Ok
            }
            Err(status) => unsafe {
                report(Failure::new(status, "engine submission refused"), out_error)
            },
        }
    })
}

/// Collects a completion once; a timeout neither takes it nor cancels the job.
///
/// # Safety
/// `reply` is valid aligned writable storage for one Reply.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_job_wait(id: u64, timeout_ms: u32, reply: *mut Reply) -> u32 {
    protect(|| {
        if reply.is_null() {
            return Status::InvalidArgument;
        }
        unsafe { reply.write(Reply::default()) };
        let outcome = (|| {
            let job = jobs().lock().unwrap_or_else(|p| p.into_inner()).get(id)?.clone();
            let completion = job.wait(timeout_ms)?;
            let result = completion.result.map(store_result).transpose()?.unwrap_or(0);
            unsafe { reply.write(Reply { result, document: completion.document }) };
            Ok(completion.status)
        })();
        outcome.unwrap_or_else(|status| status)
    })
}

/// Signals cooperative cancellation without waiting for the owner queue.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_job_cancel(id: u64) -> u32 {
    protect(|| {
        let job = jobs().lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        match job {
            Ok(job) => {
                job.cancel();
                Status::Ok
            }
            Err(status) => status,
        }
    })
}

/// Releases interest in a job and cancels any work still outstanding.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_job_release(id: u64) -> u32 {
    protect(|| {
        let job = jobs().lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        match job {
            Ok(job) => {
                job.abandon();
                Status::Ok
            }
            Err(status) => status,
        }
    })
}

/// Seals admission and waits up to the timeout for owner-thread disposal.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_engine_close(id: u64, timeout_ms: u32) -> u32 {
    protect(|| engine(id).map(|engine| engine.close(timeout_ms)).unwrap_or_else(|s| s))
}

/// Invalidates the handle and requests disposal without joining the owner.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_engine_release(id: u64) -> u32 {
    protect(|| {
        let engine = engines().lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        match engine {
            Ok(engine) => {
                engine.request_shutdown();
                Status::Ok
            }
            Err(status) => status,
        }
    })
}

/// Calls a pure helper, returning an owned result or an owned error envelope.
///
/// # Safety
/// `args` is readable for args_len bytes and result is distinct aligned writable
/// storage; null args requires zero length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_helper_call(
    operation: u32,
    args: *const u8,
    args_len: u64,
    result: *mut u64,
) -> u32 {
    protect(|| {
        if result.is_null() {
            return Status::InvalidArgument;
        }
        unsafe { result.write(0) };
        let status = unsafe { span(args, args_len, 8 * 1024 * 1024) }.map(|_| ());
        let failure = match status {
            Err(status) => Failure::new(status, "invalid helper input span"),
            Ok(()) if !(1..=6).contains(&operation) => {
                Failure::new(Status::InvalidArgument, "unknown helper operation")
            }
            Ok(()) => Failure {
                status: Status::RuntimeUnavailable,
                response: response_error(EngineError::with_message(
                    EngineErrorCode::NotImplemented,
                    "helper is unavailable in this build",
                )),
            },
        };
        unsafe { report(failure, result) }
    })
}

/// Borrows immutable result bytes until release, independently of engine life.
///
/// # Safety
/// Output pointers are valid, aligned and distinct. Access through the returned
/// view must not race release of its result handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_result_view(id: u64, bytes: *mut *const u8, len: *mut u64) -> u32 {
    protect(|| {
        unsafe {
            if !bytes.is_null() {
                bytes.write(std::ptr::null());
            }
            if !len.is_null() {
                len.write(0);
            }
        }
        if bytes.is_null() || len.is_null() {
            return Status::InvalidArgument;
        }
        let result = results().lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        match result {
            Ok(result) => {
                unsafe {
                    bytes.write(result.bytes.as_ptr());
                    len.write(result.bytes.len() as u64);
                }
                Status::Ok
            }
            Err(status) => status,
        }
    })
}

/// Releases a result allocation; stale or repeated releases are rejected.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_result_release(id: u64) -> u32 {
    protect(|| {
        let result = results().lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        match result {
            Ok(result) => {
                drop(result);
                Status::Ok
            }
            Err(status) => status,
        }
    })
}
