#![cfg(feature = "uniffi")]
use epdf_plugin_attachment::ffi::{
    AttachedFile, AttachmentFileSource, AttachmentHome, AttachmentRef,
};
use serde_json::json;
#[test]
fn file_identity_metadata_and_bytes_cross_without_loss() {
    let wire = json!({"id":"doc:key","home":{"kind":"document","ref":{"kind":"key","key":"key"}},"name":"Résumé.txt","mimeType":null,"description":"hello","size":0,"checksum":null,"createdAt":null,"modifiedAt":null});
    let file = AttachedFile::from_json(&wire).unwrap();
    assert_eq!(file.home, AttachmentHome::Document { file: AttachmentRef { key: "key".into() } });
    assert_eq!(file.to_json(), wire);
    let source = AttachmentFileSource {
        data: vec![0, 255, 128],
        name: Some("bytes.bin".into()),
        mime_type: None,
        description: None,
    };
    assert_eq!(
        epdf_kernel::bytes::Bytes::from_json(&source.to_json()["data"]).unwrap().0,
        vec![0, 255, 128]
    );
}

#[test]
fn page_homes_remain_distinct_from_document_keys() {
    let wire = json!({"id":"page:file","home":{"kind":"page","ref":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":1},"objectNumber":2}},"name":"same.txt","mimeType":"text/plain","description":null,"size":3,"checksum":null,"createdAt":null,"modifiedAt":null});
    let file = AttachedFile::from_json(&wire).unwrap();
    assert!(matches!(file.home, AttachmentHome::Page { .. }));
    assert_eq!(file.to_json(), wire);
}

#[test]
fn a_held_download_rejects_cancellation_and_close_without_leaking_subscriptions() {
    use epdf_ffi::{CancelToken, KernelThread, ViewerError};
    use epdf_plugin_attachment::ffi::{AttachmentEvent, AttachmentEventListener, AttachmentHandle};
    use epdf_plugin_attachment::{ATTACHMENT, attachment_plugin};
    use futures::{FutureExt, executor::block_on};
    use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, one_document};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Listener(AtomicUsize);
    impl AttachmentEventListener for Listener {
        fn on_event(&self, _: AttachmentEvent) {}
        fn on_end(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    for close in [false, true] {
        let thread=KernelThread::spawn(|_|{
            let mut engine=one_document();
            engine["documents"][DOCUMENT]["responses"]=json!([{"path":"attachments.list","value":{"attachments":[]}},{"path":"attachments.download","defer":"read"}]);
            TestKernel::open(vec![attachment_plugin()],engine)
        }).unwrap();
        let post = thread.post();
        let handle = block_on(thread.call(move |kernel| {
            AttachmentHandle::new(kernel.capability(&ATTACHMENT).unwrap(), post)
        }))
        .unwrap();
        block_on(handle.ensure_loaded(CancelToken::new())).unwrap();
        let file=AttachedFile::from_json(&json!({"id":"doc:key","home":{"kind":"document","ref":{"kind":"key","key":"key"}},"name":"file.bin","mimeType":null,"description":null,"size":3,"checksum":null,"createdAt":null,"modifiedAt":null})).unwrap();
        let token = CancelToken::new();
        let mut download = Box::pin(handle.download(file, token.clone()));
        assert!(download.as_mut().now_or_never().is_none());
        block_on(thread.call(|_| ())).unwrap();
        let listener = Arc::new(Listener(AtomicUsize::new(0)));
        let _subscription = handle.subscribe(listener.clone());
        if close {
            block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
        } else {
            token.cancel();
        }
        assert!(
            matches!(block_on(download),Err(ViewerError::Kernel{code:Some(code),..}) if code==if close{"instance-closed"}else{"operation-cancelled"})
        );
        if !close {
            block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
        }
        assert_eq!(listener.0.load(Ordering::SeqCst), 1);
        let late = Arc::new(Listener(AtomicUsize::new(0)));
        let _late = handle.subscribe(late.clone());
        block_on(thread.call(|_| ())).unwrap();
        assert_eq!(late.0.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn held_add_can_be_cancelled_or_closed() {
    use epdf_ffi::{CancelToken, KernelThread, ViewerError};
    use epdf_plugin_attachment::ffi::AttachmentHandle;
    use epdf_plugin_attachment::{ATTACHMENT, attachment_plugin};
    use futures::{FutureExt, executor::block_on};
    use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, one_document};
    for close in [false, true] {
        let thread=KernelThread::spawn(|_|{
            let mut engine=one_document();
            engine["documents"][DOCUMENT]["responses"]=json!([{"path":"attachments.list","value":{"attachments":[]}},{"path":"apply","defer":"write"}]);
            TestKernel::open(vec![attachment_plugin()],engine)
        }).unwrap();
        let post = thread.post();
        let handle = block_on(thread.call(move |kernel| {
            AttachmentHandle::new(kernel.capability(&ATTACHMENT).unwrap(), post)
        }))
        .unwrap();
        block_on(handle.ensure_loaded(CancelToken::new())).unwrap();
        let token = CancelToken::new();
        let source = AttachmentFileSource {
            data: vec![0, 255],
            name: Some("data.bin".into()),
            mime_type: None,
            description: None,
        };
        let mut write = Box::pin(handle.add(vec![source], token.clone()));
        assert!(write.as_mut().now_or_never().is_none());
        block_on(thread.call(|_| ())).unwrap();
        if close {
            block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
        } else {
            token.cancel();
        }
        assert!(
            matches!(block_on(write),Err(ViewerError::Kernel{code:Some(code),..}) if code==if close{"instance-closed"}else{"operation-cancelled"})
        );
    }
}
