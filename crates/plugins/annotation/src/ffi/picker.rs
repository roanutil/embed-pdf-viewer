use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{KernelPost, PageRef, Point};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

/// What the picker is asked for: a file for `tool_id` at a page point.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationFilePromptRequest {
    pub tool_id: String,
    pub subtype: String,
    /// A MIME filter (`image/png,image/jpeg,application/pdf`), when the tool has one.
    pub accept: Option<String>,
    pub page: PageRef,
    pub point: Point,
}

impl From<crate::FilePromptRequest> for AnnotationFilePromptRequest {
    fn from(r: crate::FilePromptRequest) -> Self {
        AnnotationFilePromptRequest {
            tool_id: r.tool_id,
            subtype: r.subtype,
            accept: r.accept,
            page: r.page.into(),
            point: r.point.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPickedFile {
    pub bytes: Vec<u8>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
}

/// The host's file picker: asked for a file when a tool places one (a
/// stamp image, an attachment). `None` is the user cancelling.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait AnnotationFilePicker: Send + Sync {
    async fn pick(&self, request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile>;
}

/// A `FilePickerProvider` over the foreign picker. Called on the kernel
/// thread; the pick runs on its own thread and its answer is posted back.
pub(crate) fn provider(
    picker: Arc<dyn AnnotationFilePicker>,
    post: KernelPost,
) -> crate::FilePickerProvider {
    Rc::new(move |request: crate::FilePromptRequest| {
        let (promise, resolver) = Promise::<Value>::pending();
        let settle = epdf_ffi::ForeignSettle::new(resolver, post.clone(), "the file picker");
        let picker = picker.clone();
        let request = AnnotationFilePromptRequest::from(request);
        std::thread::spawn(move || {
            let picked = futures::executor::block_on(picker.pick(request));
            // `settle` posts to the kernel thread, where the JSON is built.
            settle.settle(move || {
                Ok(match picked {
                    None => Value::Null,
                    Some(file) => {
                        let mut source = json!({ "data": Bytes(file.bytes).to_json() });
                        if let Some(name) = file.name {
                            source["name"] = name.into();
                        }
                        if let Some(mime) = file.mime_type {
                            source["mimeType"] = mime.into();
                        }
                        source
                    }
                })
            });
        });
        promise
    })
}
