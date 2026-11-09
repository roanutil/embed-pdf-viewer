#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_signature as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![
                epdf_plugin_interaction::interaction_plugin(Default::default()),
                epdf_plugin_form::form_plugin(None),
                core::signature_plugin(core::SignatureConfig {
                    mode: Some(Some(core::SignatureMode::Visual)),
                    ..Default::default()
                }),
            ],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SignatureHandle::new(k.capability(&core::SIGNATURE).unwrap(), post)),
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
        SignatureHandle::new(k.kernel.capability(&core::SIGNATURE, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.mode, Some(SignatureMode::Visual));
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || SignatureSettingsPatch { allow_certify: Some(true), ..Default::default() };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert!(changed.allow_certify);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.mode, settings.mode);
    block_on(
        handle.update_settings(SignatureSettingsPatch { clear_mode: true, ..Default::default() }),
    )
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.mode, None);
    assert!(cleared.allow_certify);
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
fn scalar_patches_and_reset_preserve_registered_signer_and_trust_ports() {
    struct Trust;
    impl epdf_signature_core::trust::TrustPort for Trust {
        fn anchors(&self) -> epdf_kernel::Promise<Vec<Vec<u8>>> {
            epdf_kernel::Promise::resolved(vec![vec![7, 8, 9]])
        }
    }
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                epdf_plugin_interaction::interaction_plugin(Default::default()),
                epdf_plugin_form::form_plugin(None),
                core::signature_plugin(core::SignatureConfig {
                    signing_key: Some(Some(core::SigningKey::Thunk(std::rc::Rc::new(|| {
                        epdf_kernel::Promise::rejected(epdf_kernel::Error::new("registered signer"))
                    })))),
                    trust: Some(Some(std::rc::Rc::new(Trust))),
                    ..Default::default()
                }),
            ],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SignatureHandle::new(k.capability(&core::SIGNATURE).unwrap(), post)),
    )
    .unwrap();
    for reset in [false, true] {
        if reset {
            block_on(handle.reset_settings()).unwrap();
        } else {
            block_on(handle.update_settings(SignatureSettingsPatch {
                mode: Some(SignatureMode::Ask),
                allow_certify: Some(true),
                ..Default::default()
            }))
            .unwrap();
        }
        block_on(thread.call(|k| {
            let cap = k.capability(&core::SIGNATURE).unwrap();
            assert_eq!(cap.retained_ports(), 2);
            let settings = cap.get_settings();
            let core::SigningKey::Thunk(resolve) = settings.signing_key.unwrap() else {
                panic!("registered signer replaced")
            };
            assert_eq!(resolve().peek().unwrap().err().unwrap().message, "registered signer");
            assert_eq!(
                settings.trust.unwrap().anchors().peek().unwrap().unwrap(),
                vec![vec![7, 8, 9]]
            );
        }))
        .unwrap();
    }
}
