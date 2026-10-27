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

impl AnnotationFilePromptRequest {
    /// A `FilePromptRequest` the plugin wrote as JSON.
    fn read(r: &Value) -> Result<Self, epdf_kernel::Error> {
        let s = |k: &str| r.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |p: &Value, k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let point = r.get("point").unwrap_or(&Value::Null);
        Ok(AnnotationFilePromptRequest {
            tool_id: s("toolId").unwrap_or_default(),
            subtype: s("subtype").unwrap_or_default(),
            accept: s("accept"),
            page: super::page_out(r.get("page").unwrap_or(&Value::Null))?,
            point: Point { x: n(point, "x"), y: n(point, "y") },
        })
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
    Rc::new(move |request: Value| {
        let request = match AnnotationFilePromptRequest::read(&request) {
            Ok(request) => request,
            Err(error) => return Promise::rejected(error),
        };
        let (promise, resolver) = Promise::<Option<Value>>::pending();
        let settle = epdf_ffi::ForeignSettle::new(resolver, post.clone(), "the file picker");
        let picker = picker.clone();
        std::thread::spawn(move || {
            let picked = futures::executor::block_on(picker.pick(request));
            // `settle` posts to the kernel thread, where the JSON is built.
            settle.settle(move || {
                Ok(match picked {
                    None => None,
                    Some(file) => {
                        let mut source = json!({ "data": Bytes(file.bytes).to_json() });
                        if let Some(name) = file.name {
                            source["name"] = name.into();
                        }
                        if let Some(mime) = file.mime_type {
                            source["mimeType"] = mime.into();
                        }
                        Some(source)
                    }
                })
            });
        });
        promise
    })
}
