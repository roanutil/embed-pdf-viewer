use epdf_capi::*;

fn create() -> u64 {
    let mut engine = u64::MAX;
    let mut error = u64::MAX;
    let status = unsafe {
        epdf_engine_create(std::ptr::null(), 0, std::ptr::null(), 0, &mut engine, &mut error)
    };
    assert_eq!(status, 0);
    assert_ne!(engine, 0);
    assert_eq!(error, 0);
    engine
}

fn result_bytes(result: u64) -> Vec<u8> {
    let mut ptr = std::ptr::null();
    let mut len = 0;
    assert_eq!(unsafe { epdf_result_view(result, &mut ptr, &mut len) }, 0);
    assert!(!ptr.is_null());
    unsafe { std::slice::from_raw_parts(ptr, len as usize).to_vec() }
}

#[test]
fn invalid_input_clears_outputs_and_returns_an_owned_error() {
    let mut engine = u64::MAX;
    let mut error = u64::MAX;
    let status = unsafe {
        epdf_engine_create(std::ptr::null(), 1, std::ptr::null(), 0, &mut engine, &mut error)
    };
    assert_eq!(status, 2);
    assert_eq!(engine, 0);
    assert_ne!(error, 0);
    let bytes = result_bytes(error);
    let json_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[4..4 + json_len]).unwrap();
    assert!(json.get("error").is_some());
    assert_eq!(epdf_result_release(error), 0);
    assert_eq!(epdf_result_release(error), 3);
}

#[test]
fn wrong_handle_kinds_are_rejected_and_views_are_zeroed() {
    let engine = create();
    let mut ptr = std::ptr::dangling::<u8>();
    let mut len = u64::MAX;
    assert_eq!(unsafe { epdf_result_view(engine, &mut ptr, &mut len) }, 3);
    assert!(ptr.is_null());
    assert_eq!(len, 0);
    assert_eq!(epdf_job_cancel(engine), 3);
    assert_eq!(epdf_job_release(engine), 3);
    assert_eq!(epdf_engine_release(engine), 0);
    assert_eq!(epdf_engine_release(engine), 3);
}

#[test]
fn close_is_repeatable_and_rejects_later_submissions() {
    let engine = create();
    assert_eq!(epdf_engine_close(engine, u32::MAX), 0);
    assert_eq!(epdf_engine_close(engine, 0), 0);
    let mut job = u64::MAX;
    let mut error = u64::MAX;
    let status = unsafe {
        epdf_engine_submit(
            engine,
            0,
            1,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            &mut job,
            &mut error,
        )
    };
    assert_eq!(status, 4);
    assert_eq!(job, 0);
    if error != 0 {
        assert_eq!(epdf_result_release(error), 0);
    }
    assert_eq!(epdf_engine_release(engine), 0);
}

#[test]
fn a_job_transfers_its_result_once_and_the_result_outlives_the_engine() {
    let engine = create();
    let mut job = 0;
    let mut error = 0;
    let payloads =
        [1u32.to_le_bytes().as_slice(), 9u32.to_le_bytes().as_slice(), b"not-a-pdf"].concat();
    assert_eq!(
        unsafe {
            epdf_engine_submit(
                engine,
                0,
                1,
                std::ptr::null(),
                0,
                payloads.as_ptr(),
                payloads.len() as u64,
                &mut job,
                &mut error,
            )
        },
        0
    );
    assert_ne!(job, 0);
    assert_eq!(error, 0);
    let mut reply = Reply { result: 0, document: 0 };
    assert_eq!(unsafe { epdf_job_wait(job, u32::MAX, &mut reply) }, 0);
    assert_ne!(reply.result, 0);
    assert_eq!(reply.document, 0);
    let result = reply.result;
    let bytes = result_bytes(result);
    assert_eq!(unsafe { epdf_job_wait(job, 0, &mut reply) }, 9);
    assert_eq!(reply.result, 0);
    assert_eq!(reply.document, 0);
    assert_eq!(epdf_job_release(job), 0);
    assert_eq!(epdf_job_release(job), 3);
    assert_eq!(epdf_engine_close(engine, u32::MAX), 0);
    assert_eq!(epdf_engine_release(engine), 0);
    assert_eq!(result_bytes(result), bytes);
    assert_eq!(epdf_result_release(result), 0);
}

#[test]
fn unknown_handles_fail_without_waiting_or_accessing_state() {
    assert_eq!(epdf_abi_version(), 0x0001_0000);
    assert_eq!(epdf_engine_close(u64::MAX, u32::MAX), 3);
    assert_eq!(epdf_engine_release(u64::MAX), 3);
    assert_eq!(epdf_job_cancel(u64::MAX), 3);
    assert_eq!(epdf_job_release(u64::MAX), 3);
    assert_eq!(epdf_result_release(u64::MAX), 3);
}

#[test]
fn excessive_lengths_are_refused_without_reading_the_pointer() {
    let mut engine = u64::MAX;
    let mut error = u64::MAX;
    let status = unsafe {
        epdf_engine_create(
            std::ptr::dangling(),
            u64::MAX,
            std::ptr::null(),
            0,
            &mut engine,
            &mut error,
        )
    };
    assert_eq!(status, 6);
    assert_eq!(engine, 0);
    if error != 0 {
        assert_eq!(epdf_result_release(error), 0);
    }
}

#[test]
fn invalid_limit_configuration_is_rejected_before_starting_an_engine() {
    let options = br#"{"limits":{"maxPendingJobs":0}}"#;
    let mut engine = u64::MAX;
    let mut error = 0;
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
        2
    );
    assert_eq!(engine, 0);
    assert_ne!(error, 0);
    assert_eq!(epdf_result_release(error), 0);
}

#[test]
fn startup_font_payloads_are_registered_before_creation_completes() {
    let font = include_bytes!("../../../packages/engine/main/test/fixtures/Roboto-Regular.ttf");
    let options = br#"{"startupFonts":[{"key":"body","payloadIndex":0,"fallback":true}]}"#;
    let mut payloads = Vec::new();
    payloads.extend_from_slice(&1u32.to_le_bytes());
    payloads.extend_from_slice(&(font.len() as u32).to_le_bytes());
    payloads.extend_from_slice(font);
    let mut engine = 0;
    let mut error = 0;
    assert_eq!(
        unsafe {
            epdf_engine_create(
                options.as_ptr(),
                options.len() as u64,
                payloads.as_ptr(),
                payloads.len() as u64,
                &mut engine,
                &mut error,
            )
        },
        0
    );
    assert_ne!(engine, 0);
    assert_eq!(error, 0);
    assert_eq!(epdf_engine_close(engine, u32::MAX), 0);
    assert_eq!(epdf_engine_release(engine), 0);
}

const ROBOTO: &[u8] =
    include_bytes!("../../../packages/engine/main/test/fixtures/Roboto-Regular.ttf");

fn frame(payloads: &[&[u8]]) -> Vec<u8> {
    let mut bytes = (payloads.len() as u32).to_le_bytes().to_vec();
    for payload in payloads {
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
    }
    bytes
}

fn create_with(options: &[u8], payloads: &[u8]) -> (u32, u64, u64) {
    let mut engine = u64::MAX;
    let mut error = u64::MAX;
    let status = unsafe {
        epdf_engine_create(
            options.as_ptr(),
            options.len() as u64,
            payloads.as_ptr(),
            payloads.len() as u64,
            &mut engine,
            &mut error,
        )
    };
    (status, engine, error)
}

fn error_message(error: u64) -> String {
    let bytes = result_bytes(error);
    let json_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[4..4 + json_len]).unwrap();
    assert_eq!(epdf_result_release(error), 0);
    json["error"]["message"].as_str().unwrap().to_owned()
}

#[test]
fn a_startup_font_failing_after_earlier_registrations_fails_creation_with_its_error() {
    let options = br#"{"startupFonts":[
        {"key":"first","payloadIndex":0,"fallback":true},
        {"key":"broken","payloadIndex":1},
        {"key":"never","payloadIndex":2}
    ]}"#;
    let payloads = frame(&[ROBOTO, b"not-a-font", ROBOTO]);
    let (status, engine, error) = create_with(options, &payloads);
    assert_eq!(status, 8);
    assert_eq!(engine, 0);
    assert_ne!(error, 0);
    let message = error_message(error);
    assert!(message.contains("broken"), "{message}");
}

#[test]
fn aggregate_startup_font_bytes_over_the_retained_budget_are_refused_before_startup() {
    let payloads = frame(&[ROBOTO, ROBOTO]);
    let total = payloads.len() as u64;
    assert!((ROBOTO.len() as u64) < total - 1);
    let options = |limit: u64| {
        format!(
            r#"{{"limits":{{"maxRetainedInputBytes":{limit}}},"startupFonts":[
                {{"key":"a","payloadIndex":0}},{{"key":"b","payloadIndex":1}}
            ]}}"#
        )
    };

    let (status, engine, error) = create_with(options(total - 1).as_bytes(), &payloads);
    assert_eq!(status, 6);
    assert_eq!(engine, 0);
    assert_ne!(error, 0);
    assert_eq!(error_message(error), "startup fonts exceed the retained-input budget");

    let (status, engine, error) = create_with(options(total).as_bytes(), &payloads);
    assert_eq!(status, 0);
    assert_ne!(engine, 0);
    assert_eq!(error, 0);
    assert_eq!(epdf_engine_close(engine, u32::MAX), 0);
    assert_eq!(epdf_engine_release(engine), 0);
}
