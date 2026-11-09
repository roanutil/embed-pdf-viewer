#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_commands as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::commands_plugin(core::CommandsConfig {
                disabled_categories: Some(vec!["edit".into()]),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| CommandsHandle::new(k.capability(&core::COMMANDS).unwrap(), post)),
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
        CommandsHandle::new(k.kernel.capability(&core::COMMANDS, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.disabled_categories, vec!["edit".to_string()]);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || CommandsSettingsPatch {
        disabled_commands: Some(vec!["print".into()]),
        ..Default::default()
    };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.disabled_commands, vec!["print".to_string()]);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.disabled_categories, settings.disabled_categories);
    block_on(handle.update_settings(CommandsSettingsPatch {
        app_state_json: Some("{\"selection\":7}".into()),
        not_allowed: Some(NotAllowedMode::Disable),
        ..Default::default()
    }))
    .unwrap();
    let state = block_on(handle.settings()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&state.app_state_json).unwrap(),
        serde_json::json!({"selection":7})
    );
    assert!(
        block_on(handle.update_settings(CommandsSettingsPatch {
            app_state_json: Some("{".into()),
            ..Default::default()
        }))
        .is_err()
    );
    assert_eq!(block_on(handle.settings()).unwrap(), state);
    block_on(handle.update_settings(CommandsSettingsPatch {
        app_state_json: Some("null".into()),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.app_state_json, "null");
    assert_eq!(cleared.not_allowed, NotAllowedMode::Disable);
    assert_eq!(cleared.disabled_commands, changed.disabled_commands);
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    assert_eq!(block_on(other.settings()).unwrap(), settings);
    block_on(thread.call(|k| plugin_conformance::test_kernel::settle(&k.kernel.destroy())))
        .unwrap()
        .unwrap();
    let error = block_on(handle.settings()).unwrap_err();
    assert!(
        matches!(error, epdf_ffi::ViewerError::Kernel { code: Some(ref code), .. } if code == "instance-closed")
    );
    assert!(block_on(handle.update_settings(patch())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}

#[test]
fn scalar_patches_and_reset_keep_registered_command_runners() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let thread = KernelThread::spawn(move |_| {
        let mut command = core::CommandDef::new("test-runner", "test-runner");
        command.run = Some(std::rc::Rc::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }));
        TestKernel::open(
            vec![core::commands_plugin(core::CommandsConfig {
                commands: Some(vec![command.into()]),
                ..Default::default()
            })],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| CommandsHandle::new(k.capability(&core::COMMANDS).unwrap(), post)),
    )
    .unwrap();
    block_on(handle.update_settings(CommandsSettingsPatch {
        app_state_json: Some("null".into()),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(
        block_on(handle.execute("test-runner".into(), None, None)).unwrap(),
        CommandExecuteResult::Executed
    );
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(
        block_on(handle.execute("test-runner".into(), None, None)).unwrap(),
        CommandExecuteResult::Executed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
