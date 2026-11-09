mod common;
trait ErrorCode {
    fn code(&self) -> Option<&str>;
}
impl ErrorCode for epdf_ffi::ViewerError {
    fn code(&self) -> Option<&str> {
        match self {
            Self::Kernel { code, .. } => code.as_deref(),
            Self::Closed => Some("instance-closed"),
        }
    }
}

use epdf_ffi::{CancelToken, PageRef};
use epdf_viewer_host::{ViewerConfig, ViewerHost, ViewerSettingsField, ViewerSettingsPatch};
use futures::executor::block_on;
use std::sync::Arc;

fn started(scope: Option<Vec<String>>) -> Arc<ViewerHost> {
    let host = ViewerHost::new(
        ViewerConfig { scope, ..Default::default() },
        Arc::new(common::NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn document_controls_keep_order_activation_and_page_identity_in_the_kernel() {
    let host = started(None);
    let first = block_on(
        host.open_document(test_support::fixture(test_support::REPORT), common::options()),
    )
    .unwrap();
    let second = block_on(host.open_document(
        test_support::fixture(test_support::REPORT),
        epdf_viewer_host::OpenDocumentOptions { activate: false, ..common::options() },
    ))
    .unwrap();
    block_on(host.rename_document(first.id.clone(), String::new())).unwrap();
    assert_eq!(
        block_on(host.document(Some(first.id.clone()))).unwrap().unwrap().name.as_deref(),
        Some("")
    );
    assert_eq!(block_on(host.active_document()).unwrap().unwrap().id, first.id);
    block_on(host.set_active_document(second.id.clone())).unwrap();
    assert_eq!(block_on(host.active_document_id()).unwrap(), Some(second.id.clone()));
    let page = block_on(host.page_at(0, None)).unwrap().unwrap();
    assert_eq!(page.index, 0);
    assert_eq!(
        block_on(host.page(page.page, Some(second.id.clone()))).unwrap(),
        Some(page.clone())
    );
    assert_eq!(block_on(host.page_index(page.page, None)).unwrap(), 0);
    assert_eq!(block_on(host.page_revision(None)).unwrap(), 0);
    assert_eq!(block_on(host.page_at(-1, None)).unwrap(), None);
    assert_eq!(
        block_on(host.page_index(PageRef { page_object_number: u32::MAX }, None)).unwrap(),
        -1
    );
    block_on(host.move_document(second.id.clone(), -5)).unwrap();
    assert_eq!(block_on(host.document_order()).unwrap(), vec![second.id.clone(), first.id.clone()]);
    block_on(host.swap_documents(first.id.clone(), second.id.clone())).unwrap();
    assert_eq!(block_on(host.document_order()).unwrap(), vec![first.id.clone(), second.id.clone()]);
    assert_eq!(
        block_on(host.set_document_order(vec![first.id.clone()])).unwrap_err().code(),
        Some("invalid-input")
    );
    block_on(host.set_document_order(vec![second.id.clone(), first.id.clone()])).unwrap();
    block_on(host.close_document(second.id.clone())).unwrap();
    assert_eq!(block_on(host.document(Some(second.id.clone()))).unwrap(), None);
    assert_eq!(block_on(host.page(page.page, Some(second.id.clone()))).unwrap(), None);
    assert_eq!(block_on(host.page_index(page.page, Some(second.id.clone()))).unwrap(), -1);
    assert!(!block_on(host.can_download(Some(second.id.clone()))).unwrap());
    assert_eq!(block_on(host.page_revision(Some(second.id))).unwrap(), -1);
    assert_eq!(block_on(host.document(Some("missing".into()))).unwrap(), None);
    assert!(block_on(host.has_document(first.id.clone())).unwrap());
    assert_eq!(block_on(host.document_count()).unwrap(), 1);
    block_on(host.close_all_documents()).unwrap();
    assert!(block_on(host.documents()).unwrap().is_empty());
    assert_eq!(block_on(host.active_document()).unwrap(), None);
}

#[test]
fn download_checks_and_layer_save_keep_engine_authorization() {
    let host = started(Some(vec!["doc.open".into()]));
    let doc = block_on(
        host.open_document(test_support::fixture(test_support::REPORT), common::options()),
    )
    .unwrap();
    assert!(!block_on(host.can_download(Some(doc.id.clone()))).unwrap());
    assert!(!block_on(host.allows("doc.download".into(), Some(doc.id.clone()))).unwrap());
    assert_eq!(
        block_on(host.save_layer(doc.id.clone(), CancelToken::new())).unwrap_err().code(),
        Some("permission-denied")
    );
    assert_eq!(
        block_on(host.save(doc.id, epdf_viewer_host::SaveMode::Incremental, CancelToken::new()))
            .unwrap_err()
            .code(),
        Some("permission-denied")
    );
}

#[test]
fn a_saved_layer_restores_over_its_base_and_cancelled_save_has_no_effect() {
    let host = ViewerHost::new(
        ViewerConfig { metadata: true, ..Default::default() },
        Arc::new(common::NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(
        host.open_document(test_support::fixture(test_support::REPORT), common::options()),
    )
    .unwrap();
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        block_on(host.save_layer(doc.id.clone(), token)).unwrap_err().code(),
        Some("operation-cancelled")
    );
    let metadata = block_on(host.metadata(doc.id.clone())).unwrap();
    block_on(metadata.update(
        epdf_plugin_metadata::ffi::MetadataPatch {
            title: Some("Saved layer title".into()),
            ..Default::default()
        },
        CancelToken::new(),
    ))
    .unwrap();
    let bytes = block_on(host.save_layer(doc.id, CancelToken::new())).unwrap();
    let reopened = block_on(host.open_layer_document(
        test_support::fixture(test_support::REPORT),
        bytes,
        common::options(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(reopened.page_count, 14);
    let metadata = block_on(host.metadata(reopened.id)).unwrap();
    block_on(metadata.ensure_loaded(CancelToken::new())).unwrap();
    assert_eq!(
        block_on(metadata.snapshot()).unwrap().unwrap().title.as_deref(),
        Some("Saved layer title")
    );
    block_on(host.close_all_documents()).unwrap();
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
}

#[test]
fn presentation_settings_merge_reset_and_notify_without_changing_authorization() {
    let host = started(Some(vec!["doc.open".into()]));
    let initial = block_on(host.viewer_settings()).unwrap();
    let before = host.state_revision();
    block_on(host.update_viewer_settings(ViewerSettingsPatch {
        accent: Some("#123456".into()),
        page_background: Some("transparent".into()),
        ..Default::default()
    }))
    .unwrap();
    let changed = block_on(host.viewer_settings()).unwrap();
    assert_eq!(changed.accent.as_deref(), Some("#123456"));
    assert_eq!(changed.page_background.as_deref(), Some("transparent"));
    assert_eq!(changed.page_shadow, initial.page_shadow);
    assert!(host.state_revision() > before);
    block_on(host.update_viewer_settings(ViewerSettingsPatch {
        clear: vec![ViewerSettingsField::Accent],
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(host.viewer_settings()).unwrap();
    assert_eq!(cleared.accent, None);
    assert_eq!(cleared.page_background.as_deref(), Some("transparent"));
    assert_eq!(
        block_on(host.update_viewer_settings(ViewerSettingsPatch {
            accent: Some("red".into()),
            clear: vec![ViewerSettingsField::Accent],
            ..Default::default()
        }))
        .unwrap_err()
        .code(),
        Some("invalid-input")
    );
    block_on(host.reset_viewer_settings()).unwrap();
    assert_eq!(block_on(host.viewer_settings()).unwrap(), initial);
    let doc = block_on(
        host.open_document(test_support::fixture(test_support::REPORT), common::options()),
    )
    .unwrap();
    assert!(!block_on(host.can_download(Some(doc.id))).unwrap());
}

#[test]
fn failed_byte_and_layer_sources_retry_the_original_bytes_and_release_on_close() {
    let host = started(None);
    let first = block_on(host.open_document(b"not a pdf".to_vec(), common::options())).unwrap_err();
    let tab = block_on(host.documents()).unwrap().remove(0);
    let retried = block_on(host.retry_document(tab.id)).unwrap_err();
    assert_eq!(first.code(), Some("operation-failed"));
    assert_eq!(retried.code(), first.code());
    let epdf_ffi::ViewerError::Kernel { detail, .. } = retried else { panic!("closed viewer") };
    assert_eq!(detail, "failed to open base document");
    block_on(host.close_all_documents()).unwrap();
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        block_on(host.open_layer_document(
            test_support::fixture(test_support::REPORT),
            vec![],
            common::options(),
            token
        ))
        .unwrap_err()
        .code(),
        Some("operation-cancelled")
    );
    assert!(block_on(host.documents()).unwrap().is_empty());
    let error = block_on(host.open_layer_document(
        test_support::fixture(test_support::REPORT),
        b"invalid artifact".to_vec(),
        common::options(),
        CancelToken::new(),
    ))
    .unwrap_err();
    let tab = block_on(host.documents()).unwrap().remove(0);
    let retried = block_on(host.retry_document(tab.id)).unwrap_err();
    assert_eq!(retried.code(), error.code());
    let epdf_ffi::ViewerError::Kernel { detail: first_detail, .. } = error else {
        panic!("closed viewer")
    };
    let epdf_ffi::ViewerError::Kernel { detail, .. } = retried else { panic!("closed viewer") };
    assert_eq!(detail, first_detail);
    block_on(host.close_all_documents()).unwrap();
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
}
