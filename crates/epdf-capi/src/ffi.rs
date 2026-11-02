use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::host::Host;

use crate::abi::{Failure, Reply, Status, response_error};
use crate::buffers::{ByteBudget, OwnedResult, Reservation, unpack_payloads};
use crate::config::Options;
use crate::engine::Engine;
use crate::handles::{Kind, Registry};
use crate::helpers;
use crate::jobs::{DocumentTransfer, Job};
use crate::operations;
use crate::startup::initialize_fonts;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

struct RuntimeState {
    host: Host,
    _fonts: Reservation,
    input_budget: Arc<ByteBudget>,
    documents: BTreeMap<u64, (String, Reservation)>,
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

fn next_identity() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl RuntimeState {
    fn close(&mut self, id: u64) {
        if let Some((name, _retained)) = self.documents.remove(&id) {
            operations::close(&mut self.host, &name);
        }
        let _ = documents().lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }
}

impl Drop for RuntimeState {
    fn drop(&mut self) {
        for id in self.documents.keys() {
            let _ = documents().lock().unwrap_or_else(|p| p.into_inner()).remove(*id);
        }
    }
}

fn discard_document(id: u64, engine: &std::sync::Weak<NativeEngine>) {
    let _ = documents().lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    if let Some(engine) = engine.upgrade() {
        engine.cleanup(move |state| state.close(id));
    }
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
                Ok(RuntimeState {
                    host,
                    _fonts: retained,
                    input_budget: budget,
                    documents: BTreeMap::new(),
                })
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
            let bytes = unsafe { span(payloads, payloads_len, engine.limits.max_payload_bytes) }?;
            let payloads = unpack_payloads(bytes, engine.limits.max_payload_bytes as usize)?;
            operations::validate(operation, &args, payloads.len())?;
            let document_name = if operation == 1 {
                format!("native-document-{}", next_identity())
            } else {
                documents()
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_for_parent(document, engine_id)?
                    .clone()
            };
            let limits = engine.limits.clone();
            let weak = Arc::downgrade(&engine);
            let close_accepted = Arc::new(AtomicBool::new(false));
            let close_flag = close_accepted.clone();
            let close_guard = (operation == 2).then(|| {
                let weak = weak.clone();
                DocumentTransfer::new(document, move || {
                    if close_accepted.load(Ordering::Acquire) {
                        discard_document(document, &weak);
                    }
                })
            });
            let job = engine.submit_operation(
                admission,
                operation == 6,
                move || {
                    if operation != 1 {
                        let mut registry = documents().lock().unwrap_or_else(|p| p.into_inner());
                        registry.get_for_parent(document, engine_id)?;
                        if operation == 2 {
                            registry.remove(document)?;
                            close_flag.store(true, Ordering::Release);
                        }
                    }
                    Ok(())
                },
                move |state, token, capacity| {
                    let mut limits = limits;
                    limits.max_result_bytes = limits.max_result_bytes.min(capacity);
                    let request = operations::request(
                        operation,
                        args,
                        &document_name,
                        next_identity(),
                        &limits,
                    )?;
                    let retained = if operation == 1 {
                        let size = payloads
                            .iter()
                            .try_fold(0u64, |size, payload| size.checked_add(payload.len() as u64))
                            .ok_or_else(|| {
                                EngineError::with_message(
                                    EngineErrorCode::PayloadTooLarge,
                                    "input byte sum overflow",
                                )
                            })?;
                        Some(state.input_budget.reserve(size).map_err(|_| {
                            EngineError::with_message(
                                EngineErrorCode::PayloadTooLarge,
                                "retained PDF input budget exceeded",
                            )
                        })?)
                    } else {
                        None
                    };
                    let response =
                        operations::call(&mut state.host, operation, request, payloads, token)?;
                    if operation == 2 {
                        state.close(document);
                        if let Some(guard) = close_guard {
                            guard.transfer();
                        }
                    }
                    let successful = serde_json::from_str::<serde_json::Value>(&response.json)
                        .map_err(|_| {
                            EngineError::with_message(
                                EngineErrorCode::WireFormat,
                                "invalid Host response",
                            )
                        })?
                        .get("error")
                        .is_none();
                    let transfer = if operation == 1 && successful {
                        let inserted = documents()
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(engine_id, document_name.clone());
                        let id = match inserted {
                            Ok(id) => id,
                            Err(_) => {
                                operations::close(&mut state.host, &document_name);
                                return Err(EngineError::with_message(
                                    EngineErrorCode::PayloadTooLarge,
                                    "document registry is full",
                                ));
                            }
                        };
                        state.documents.insert(id, (document_name, retained.unwrap()));
                        Some(DocumentTransfer::new(id, move || discard_document(id, &weak)))
                    } else {
                        None
                    };
                    Ok((response, transfer))
                },
            )?;
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
            let (status, transferred) = collect_job(&job, timeout_ms, store_result)?;
            unsafe { reply.write(transferred) };
            Ok(status)
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
        let outcome = unsafe { span(args, args_len, helpers::MAX_ARGS_BYTES) }
            .map_err(|s| Failure::new(s, "invalid helper input span"))
            .and_then(|args| helpers::call(operation, args))
            .and_then(|response| {
                OwnedResult::from_response(&response, helpers::MAX_RESULT_BYTES, None)
                    .and_then(store_result)
                    .map_err(|s| Failure::new(s, "helper result exceeds the result budget"))
            });
        match outcome {
            Ok(id) => {
                unsafe { result.write(id) };
                Status::Ok
            }
            Err(failure) => unsafe { report(failure, result) },
        }
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

fn collect_job(
    job: &Job,
    timeout_ms: u32,
    register: impl FnOnce(Arc<OwnedResult>) -> Result<u64, Status>,
) -> Result<(Status, Reply), Status> {
    let mut result = 0;
    let mut completion = job.take_after(timeout_ms, |completion| {
        result = completion
            .result
            .as_ref()
            .map(|value| register(value.clone()))
            .transpose()?
            .unwrap_or(0);
        Ok(())
    })?;
    if let Some(document) = completion.owned_document.take() {
        document.transfer();
    }
    Ok((completion.status, Reply { result, document: completion.document }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Completion;

    #[test]
    fn a_refused_result_registration_preserves_the_entire_completion_for_retry() {
        let job = Job::new();
        let result = OwnedResult::from_response(
            &epdf_engine::host::HostResponse { json: "{\"result\":null}".into(), payloads: vec![] },
            1024,
            None,
        )
        .unwrap();
        let observed = result.clone();
        job.complete(Completion {
            status: Status::Ok,
            result: Some(result),
            document: 17,
            owned_document: None,
        });
        assert!(matches!(
            collect_job(&job, 0, |_| Err(Status::LimitExceeded)),
            Err(Status::LimitExceeded)
        ));
        let (status, reply) = collect_job(&job, 0, |result| {
            assert!(Arc::ptr_eq(&result, &observed));
            Ok(23)
        })
        .expect("refused handoff must remain collectable");
        assert_eq!(status, Status::Ok);
        assert_eq!(reply.document, 17);
        assert_eq!(reply.result, 23);
        assert!(matches!(collect_job(&job, 0, |_| Ok(24)), Err(Status::AlreadyTaken)));
    }
    fn native_test_engine(options: &str) -> u64 {
        let (mut engine, mut error) = (0, 0);
        assert_eq!(
            unsafe {
                epdf_engine_create(
                    options.as_ptr(),
                    options.len() as u64,
                    std::ptr::null(),
                    0,
                    &mut engine,
                    &mut error,
                )
            },
            0
        );
        assert_eq!(error, 0);
        engine
    }

    fn submit_test_open(engine: u64) -> u64 {
        let pdf =
            include_bytes!("../../../tooling/engine-conformance/fixtures/annotation_kinds.pdf");
        let mut payload = 1u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&(pdf.len() as u32).to_le_bytes());
        payload.extend_from_slice(pdf);
        let (mut job, mut error) = (0, 0);
        assert_eq!(
            unsafe {
                epdf_engine_submit(
                    engine,
                    0,
                    1,
                    std::ptr::null(),
                    0,
                    payload.as_ptr(),
                    payload.len() as u64,
                    &mut job,
                    &mut error,
                )
            },
            0
        );
        assert_eq!(error, 0);
        job
    }

    fn owner_document_count(id: u64) -> usize {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        engine(id).unwrap().cleanup(move |state| {
            assert_eq!(state.documents.len(), state.host.sessions().len());
            sender.send((state.documents.len(), std::thread::current().id())).unwrap();
        });
        let (count, owner) = receiver.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_ne!(owner, std::thread::current().id());
        count
    }

    #[test]
    fn abandoned_ready_opens_are_closed_on_the_owner_and_release_input_capacity() {
        let pdf =
            include_bytes!("../../../tooling/engine-conformance/fixtures/annotation_kinds.pdf");
        let id = native_test_engine(&format!(
            r#"{{"limits":{{"maxRetainedInputBytes":{}}}}}"#,
            pdf.len()
        ));
        for _ in 0..3 {
            let job = submit_test_open(id);
            assert_eq!(owner_document_count(id), 1);
            assert_eq!(epdf_job_release(job), 0);
            assert_eq!(owner_document_count(id), 0);
        }
        assert_eq!(epdf_engine_close(id, u32::MAX), 0);
        assert_eq!(epdf_engine_release(id), 0);
    }

    #[test]
    fn failed_result_framing_closes_an_open_before_its_bytes_can_be_retained() {
        let id = native_test_engine(r#"{"limits":{"maxResultBytes":1}}"#);
        let job = submit_test_open(id);
        let mut reply = Reply::default();
        assert_eq!(
            unsafe { epdf_job_wait(job, u32::MAX, &mut reply) },
            Status::LimitExceeded as u32
        );
        assert_eq!(reply.document, 0);
        assert_eq!(reply.result, 0);
        owner_document_count(id);
        assert_eq!(owner_document_count(id), 0);
        assert_eq!(epdf_job_release(job), 0);
        assert_eq!(epdf_engine_close(id, u32::MAX), 0);
        assert_eq!(epdf_engine_release(id), 0);
    }

    #[test]
    fn cancelling_an_admitted_close_still_releases_its_document() {
        let id = native_test_engine("{}");
        let open = submit_test_open(id);
        let mut reply = Reply::default();
        assert_eq!(unsafe { epdf_job_wait(open, u32::MAX, &mut reply) }, 0);
        assert_ne!(reply.document, 0);
        epdf_result_release(reply.result);
        epdf_job_release(open);
        let (started, ready) = std::sync::mpsc::sync_channel(1);
        let (release, blocked) = std::sync::mpsc::sync_channel(1);
        engine(id).unwrap().cleanup(move |_| {
            started.send(()).unwrap();
            blocked.recv().unwrap();
        });
        ready.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let (mut close, mut error) = (0, 0);
        assert_eq!(
            unsafe {
                epdf_engine_submit(
                    id,
                    reply.document,
                    2,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    &mut close,
                    &mut error,
                )
            },
            0
        );
        assert_eq!(epdf_job_cancel(close), 0);
        release.send(()).unwrap();
        assert_eq!(unsafe { epdf_job_wait(close, u32::MAX, &mut reply) }, 0);
        epdf_result_release(reply.result);
        epdf_job_release(close);
        owner_document_count(id);
        assert_eq!(owner_document_count(id), 0);
        epdf_engine_close(id, u32::MAX);
        epdf_engine_release(id);
    }
}
