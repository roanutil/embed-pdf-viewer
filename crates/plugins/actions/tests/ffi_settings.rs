#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_actions as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::actions_plugin(core::ActionsConfig {
                open_sequence: Some(core::OpenSequence::Off),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| ActionsHandle::new(k.capability(&core::ACTIONS).unwrap(), post)),
    )
    .unwrap();
    let post = thread.post();
    let other = block_on(thread.call(move |k| {
        let opened = plugin_conformance::test_kernel::settle(&k.kernel.documents().open(
            epdf_kernel::OpenSource::Input(
                serde_json::json!({"kind":"bytes","id":"b","bytes":{"$bytes":""}}),
            ),
            epdf_kernel::OpenOptions::default(),
        ))
        .unwrap();
        let id = opened["document"]["id"].as_str().unwrap().to_string();
        ActionsHandle::new(k.kernel.capability(&core::ACTIONS, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.open_sequence, OpenSequence::Off);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || ActionsSettingsPatch {
        triggers: Some(TriggerGates { page: Some(false), ..Default::default() }),
        ..Default::default()
    };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert!(!changed.triggers.page);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.open_sequence, settings.open_sequence);
    assert_eq!(changed.triggers.document, settings.triggers.document);
    assert_eq!(changed.triggers.annotation, settings.triggers.annotation);
    block_on(handle.update_settings(ActionsSettingsPatch {
        javascript: Some(ActionsJavaScriptSettingsPatch {
            identity: Some(ActionsScriptIdentity {
                name: Some("Ada".into()),
                email: Some("ada@example.test".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    block_on(handle.update_settings(ActionsSettingsPatch {
        javascript: Some(ActionsJavaScriptSettingsPatch {
            identity: Some(ActionsScriptIdentity {
                name: Some("Grace".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    let ActionsScriptIdentitySetting::Value { identity } =
        block_on(other.settings()).unwrap().javascript.identity.unwrap()
    else {
        panic!("expected scalar identity")
    };
    assert_eq!(identity.name.as_deref(), Some("Grace"));
    assert_eq!(identity.email.as_deref(), Some("ada@example.test"));
    block_on(handle.update_settings(ActionsSettingsPatch {
        javascript: Some(ActionsJavaScriptSettingsPatch {
            clear_identity: true,
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.javascript.identity, None);
    assert_eq!(cleared.javascript.enabled, settings.javascript.enabled);
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    assert_eq!(block_on(other.settings()).unwrap(), settings);

    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let error = block_on(handle.settings()).unwrap_err();
    assert!(
        matches!(error, epdf_ffi::ViewerError::Kernel { code: Some(ref code), .. } if code == "instance-closed")
    );
    assert!(block_on(handle.update_settings(patch())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}

#[test]
fn reading_settings_does_not_invoke_a_registered_identity_provider() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![core::actions_plugin(core::ActionsConfig {
                open_sequence: Some(core::OpenSequence::Off),
                javascript: Some(core::JavaScriptConfig {
                    identity: Some(Some(core::ActionsScriptIdentity::Function(std::rc::Rc::new(
                        move || {
                            called.fetch_add(1, Ordering::SeqCst);
                            epdf_acrojs::PartialScriptIdentity {
                                name: Some("provider".into()),
                                ..Default::default()
                            }
                        },
                    )))),
                    ..Default::default()
                }),
                ..Default::default()
            })],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| ActionsHandle::new(k.capability(&core::ACTIONS).unwrap(), post)),
    )
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(
        settings.javascript.identity,
        Some(ActionsScriptIdentitySetting::RegisteredProvider)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0, "a settings read must stay pure");
}
