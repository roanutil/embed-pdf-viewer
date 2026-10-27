//! Document scopes and locked documents through the native engine.

use std::sync::Arc;

use epdf_plugin_metadata::ffi::MetadataPatch;
use epdf_viewer_host::{DocumentStatus, SaveMode, ViewerConfig, ViewerHost};
use futures::executor::block_on;

mod common;
use common::{NullDelegate, fixture, options};

fn started(config: ViewerConfig) -> Arc<ViewerHost> {
    let host = ViewerHost::new(config, Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_locked_document_opens_locked_and_unlocks() {
    let host = started(ViewerConfig::default());
    let doc = block_on(host.open_document(fixture(test_support::ENCRYPTED), options())).unwrap();
    assert_eq!(doc.status, DocumentStatus::Locked);
    let wrong = block_on(host.unlock_document(doc.id.clone(), "nope".into()));
    assert!(wrong.is_err());
    block_on(host.unlock_document(doc.id.clone(), "hôtel".into())).unwrap();
    let info = block_on(host.documents()).unwrap().into_iter().find(|d| d.id == doc.id).unwrap();
    assert_eq!(info.status, DocumentStatus::Ready);
    assert!(info.page_count > 0);
    assert!(!block_on(host.pages(doc.id)).unwrap().is_empty());
}

#[test]
fn a_narrow_scope_denies_edits() {
    let host = started(ViewerConfig {
        metadata: true,
        scope: Some(vec!["doc.open".into(), "doc.render".into()]),
        ..Default::default()
    });
    let doc = block_on(host.open_document(fixture(test_support::REPORT), options())).unwrap();
    let metadata = block_on(host.metadata(doc.id)).unwrap();
    assert!(!block_on(metadata.can_update()).unwrap());
    let patch = MetadataPatch { title: Some("x".into()), ..Default::default() };
    let refused = block_on(metadata.update(patch, epdf_ffi::CancelToken::new()));
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn the_default_scope_allows_everything() {
    let host = started(ViewerConfig { metadata: true, ..Default::default() });
    let doc = block_on(host.open_document(fixture(test_support::REPORT), options())).unwrap();
    assert!(block_on(block_on(host.metadata(doc.id)).unwrap().can_update()).unwrap());
}

#[test]
fn a_scope_without_doc_download_refuses_a_save() {
    let host = started(ViewerConfig {
        scope: Some(vec!["doc.open".into(), "doc.render".into()]),
        ..Default::default()
    });
    let doc = block_on(host.open_document(fixture(test_support::REPORT), options())).unwrap();
    let refused = block_on(host.save(doc.id, SaveMode::Incremental, epdf_ffi::CancelToken::new()));
    match refused {
        Err(epdf_ffi::ViewerError::Kernel { code, .. }) => {
            assert_eq!(code.as_deref(), Some("permission-denied"))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_locked_document_refuses_a_save() {
    let host = started(ViewerConfig::default());
    let doc = block_on(host.open_document(fixture(test_support::ENCRYPTED), options())).unwrap();
    assert_eq!(doc.status, DocumentStatus::Locked);
    let refused = block_on(host.save(doc.id, SaveMode::Incremental, epdf_ffi::CancelToken::new()));
    assert!(refused.is_err(), "{refused:?}");
}
