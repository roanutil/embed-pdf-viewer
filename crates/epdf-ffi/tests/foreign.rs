use epdf_ffi::{ForeignSettle, HostError, KernelBound, KernelThread, ViewerError};
use epdf_kernel::rt::Promise;
use futures::executor::block_on;

fn thread() -> KernelThread<()> {
    KernelThread::spawn(|_| Ok(())).unwrap()
}

/// A pending promise parked on the kernel thread, and the settle for it.
fn pending(thread: &KernelThread<()>) -> (KernelBound<Promise<u32>>, ForeignSettle<u32>) {
    let post = thread.post();
    block_on(thread.call(move |_| {
        let (promise, resolver) = Promise::<u32>::pending();
        let settle = ForeignSettle::new(resolver, post.clone(), "a test port");
        (KernelBound::new(promise, post), settle)
    }))
    .unwrap()
}

fn wait(promise: &KernelBound<Promise<u32>>) -> Result<u32, ViewerError> {
    block_on(promise.call_promise(|p| Ok(p.clone()), |v| v))
}

#[test]
fn an_answer_from_another_thread_settles_the_promise() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    std::thread::spawn(move || settle.settle(|| Ok(7))).join().unwrap();
    assert_eq!(wait(&promise).unwrap(), 7);
}

#[test]
fn the_first_answer_wins() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    settle.settle(|| Ok(1));
    settle.settle(|| Ok(2));
    assert_eq!(wait(&promise).unwrap(), 1);
}

#[test]
fn a_settle_dropped_without_an_answer_rejects() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    drop(settle);
    match wait(&promise).unwrap_err() {
        ViewerError::Kernel { code, detail, .. } => {
            assert_eq!(code.as_deref(), Some("operation-failed"));
            assert!(detail.contains("a test port"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_callback_error_keeps_its_reason() {
    let host = HostError::from(uniffi::UnexpectedUniFFICallbackError { reason: "boom".into() });
    let kernel: epdf_kernel::Error = host.into();
    assert_eq!(kernel.name, "HostError");
    assert_eq!(kernel.code(), Some("operation-failed"));
    assert!(kernel.message.contains("boom"), "{}", kernel.message);
    let viewer = ViewerError::from(uniffi::UnexpectedUniFFICallbackError { reason: "boom".into() });
    match viewer {
        ViewerError::Kernel { name, code, detail } => {
            assert_eq!(
                (name.as_str(), code.as_deref(), detail.as_str()),
                ("HostError", Some("operation-failed"), "boom")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_host_failure_keeps_its_name_and_code_through_to_plugin_error() {
    let failure = epdf_ffi::HostFailure {
        name: "AbortError".into(),
        code: Some("operation-cancelled".into()),
        detail: "operation cancelled".into(),
    };
    let error = failure.into_error("commands");
    assert_eq!(error.name, "AbortError");
    let mapped = epdf_kernel::error::to_plugin_error("commands", &error);
    assert_eq!(mapped.code(), Some("operation-cancelled"));
    assert_eq!(mapped.message, "operation cancelled");
}

#[test]
fn a_host_failure_without_a_code_is_operation_failed() {
    let failure =
        epdf_ffi::HostFailure { name: "Error".into(), code: None, detail: "disk full".into() };
    let error = epdf_kernel::error::to_plugin_error("i18n", &failure.into_error("i18n"));
    assert_eq!(error.code(), Some("operation-failed"));
    assert_eq!(error.message, "disk full");
}

#[test]
fn a_failed_settle_rejects_with_the_failures_code() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    settle.fail(
        epdf_ffi::HostFailure {
            name: "PluginError".into(),
            code: Some("permission-denied".into()),
            detail: "no".into(),
        },
        "a test",
    );
    match wait(&promise).unwrap_err() {
        ViewerError::Kernel { code, .. } => assert_eq!(code.as_deref(), Some("permission-denied")),
        other => panic!("{other:?}"),
    }
}
