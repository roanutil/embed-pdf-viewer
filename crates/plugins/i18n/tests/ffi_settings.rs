#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_i18n as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::i18n_plugin(core::I18nConfig {
                locale: Some(Some("en".into())),
                fallback_locale: Some("es".into()),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| I18nHandle::new(k.capability(&core::I18N).unwrap(), post)))
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
        I18nHandle::new(k.kernel.capability(&core::I18N, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.fallback_locale, "es");
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || I18nSettingsPatch { fallback_locale: Some("fr".into()), ..Default::default() };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.fallback_locale, "fr");
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.locale, settings.locale);
    block_on(
        handle.update_settings(I18nSettingsPatch { clear_locale: true, ..Default::default() }),
    )
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.locale, None);
    assert_eq!(cleared.fallback_locale, "fr");
    assert_eq!(cleared.locales, settings.locales);
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
