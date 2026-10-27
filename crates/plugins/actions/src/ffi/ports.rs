//! The application ports as foreign traits: node executors, the UI adapter,
//! the submit handler and a save or print operation. Each is called on the
//! kernel thread. One that answers later does so through a completion
//! object, from any thread; a completion dropped unanswered fails its call.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    ForeignSettle, HostError, HostFailure, KernelPost, RegistrationHandle, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::Value;

use super::{ActionContext, ActionsHandle, PdfActionNode, open};
use crate::contract::{
    self as plugin, ActionOrigin, ActionTriggerEvent, ActionUiContext, Disposer, DocumentVerb,
    ScriptPhase,
};

/// Runs one node type. Never sees the tree or the capability.
#[uniffi::export(with_foreign)]
pub trait ActionExecutor: Send + Sync {
    fn execute(
        &self,
        node: Arc<PdfActionNode>,
        context: ActionContext,
        completion: Arc<ActionExecutorCompletion>,
    );
}

/// The answer to one `ActionExecutor::execute`. The first call wins.
#[derive(uniffi::Object)]
pub struct ActionExecutorCompletion {
    settle: ForeignSettle<plugin::ActionExecutorResult>,
}

#[uniffi::export]
impl ActionExecutorCompletion {
    pub fn executed(&self) {
        self.settle.settle(|| Ok(plugin::ActionExecutorResult::Executed));
    }

    /// Recognized, deliberately not acted on.
    pub fn inert(&self, reason: String) {
        self.settle.settle(move || Ok(plugin::ActionExecutorResult::Inert { reason }));
    }

    pub fn failed(&self, error: String) {
        self.settle.settle(move || Ok(plugin::ActionExecutorResult::Failed { error }));
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionUriOptions {
    pub is_map: bool,
    pub origin: ActionOrigin,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionAlertOptions {
    pub origin: ActionOrigin,
    pub phase: ScriptPhase,
    /// Acrobat's `app.alert` icon number.
    pub icon: f64,
    pub title: Option<String>,
}

/// Which optional UI members the handler implements. Without `alert`, a
/// script alert is suppressed with a diagnostic; without `goto_page`, a
/// script page change is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionUiSupport {
    #[uniffi(default = true)]
    pub alert: bool,
    #[uniffi(default = true)]
    pub goto_page: bool,
}

/// The UI a document's actions reach. A throw propagates into the dispatch.
#[uniffi::export(with_foreign)]
pub trait ActionUiHandler: Send + Sync {
    /// Already through the scheme allowlist (http, https, mailto, tel), but
    /// still document-supplied: treat it as untrusted.
    fn open_uri(&self, uri: String, options: ActionUriOptions) -> Result<(), HostError>;
    /// The Named Print verb and script `print()`.
    fn print(&self, context: Option<ActionUiContext>) -> Result<(), HostError>;
    fn alert(&self, message: String, options: ActionAlertOptions) -> Result<(), HostError>;
    /// `page` is what the script asked for, zero-based and unchecked.
    fn goto_page(&self, page: f64, context: ActionUiContext) -> Result<(), HostError>;
}

fn ui_adapter(
    handler: Arc<dyn ActionUiHandler>,
    support: ActionUiSupport,
) -> plugin::ActionUiAdapter {
    type Alert = Rc<dyn Fn(&str, &plugin::AlertOptions) -> Result<(), Error>>;
    type GotoPage = Rc<dyn Fn(f64, &ActionUiContext) -> Result<(), Error>>;
    let open_uri = {
        let h = handler.clone();
        Rc::new(move |uri: &str, options: &plugin::UriOptions| {
            let options = ActionUriOptions { is_map: options.is_map, origin: options.origin };
            h.open_uri(uri.to_string(), options).map_err(Error::from)
        })
    };
    let print = {
        let h = handler.clone();
        Rc::new(move |context: Option<&ActionUiContext>| {
            h.print(context.cloned()).map_err(Error::from)
        })
    };
    let alert = support.alert.then(|| {
        let h = handler.clone();
        Rc::new(move |message: &str, options: &plugin::AlertOptions| {
            let options = ActionAlertOptions {
                origin: options.origin,
                phase: options.phase,
                icon: options.icon,
                title: options.title.clone(),
            };
            h.alert(message.to_string(), options).map_err(Error::from)
        }) as Alert
    });
    let goto_page = support.goto_page.then(|| {
        let h = handler.clone();
        Rc::new(move |page: f64, context: &ActionUiContext| {
            h.goto_page(page, context.clone()).map_err(Error::from)
        }) as GotoPage
    });
    plugin::ActionUiAdapter { open_uri, print, alert, goto_page }
}

/// A submitted value: a string, a list (multi-select), or name only.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormSubmissionValue {
    Text { value: String },
    List { values: Vec<String> },
    NameOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FormSubmissionEntry {
    pub name: String,
    pub value: FormSubmissionValue,
}

/// The resolved dataset a submit handler receives.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionSubmitRequest {
    pub url: Option<String>,
    pub method: String,
    pub format: String,
    pub flags_raw: u32,
    pub char_set: Option<String>,
    pub entries: Vec<FormSubmissionEntry>,
    pub origin: ActionOrigin,
    pub event: ActionTriggerEvent,
}

impl TryFrom<&plugin::ActionSubmitRequest> for ActionSubmitRequest {
    type Error = Error;

    fn try_from(request: &plugin::ActionSubmitRequest) -> Result<Self, Error> {
        let entries = request
            .entries
            .iter()
            .map(|entry| {
                let value = match &entry.value {
                    Value::String(value) => FormSubmissionValue::Text { value: value.clone() },
                    Value::Null => FormSubmissionValue::NameOnly,
                    Value::Array(items) => FormSubmissionValue::List {
                        values: items
                            .iter()
                            .map(|item| item.as_str().map(str::to_string))
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(|| bad_entry(&entry.name, &entry.value))?,
                    },
                    other => return Err(bad_entry(&entry.name, other)),
                };
                Ok(FormSubmissionEntry { name: entry.name.clone(), value })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(ActionSubmitRequest {
            url: request.url.clone(),
            method: request.method.clone(),
            format: request.format.clone(),
            flags_raw: request.flags_raw as u32,
            char_set: request.char_set.clone(),
            entries,
            origin: request.origin,
            event: request.event.clone(),
        })
    }
}

fn bad_entry(name: &str, value: &Value) -> Error {
    Error::named("InvalidState", format!("[actions] submit entry \"{name}\" has value {value}"))
        .with("code", "invalid-state")
}

/// Sink 1 of the submit chain. Return to accept (the network call is yours,
/// and its outcome is yours to report); throw to fail the node.
#[uniffi::export(with_foreign)]
pub trait ActionSubmitHandler: Send + Sync {
    fn submit(&self, request: ActionSubmitRequest) -> Result<(), HostError>;
}

/// A save or print, run between the document's will- and did- trees.
#[uniffi::export(with_foreign)]
pub trait DocumentVerbOperation: Send + Sync {
    fn run(&self, completion: Arc<DocumentVerbCompletion>);
}

#[derive(uniffi::Object)]
pub struct DocumentVerbCompletion {
    settle: ForeignSettle<()>,
}

#[uniffi::export]
impl DocumentVerbCompletion {
    pub fn finish(&self) {
        self.settle.settle(|| Ok(()));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || {
            Err(Error::new(detail).with("code", epdf_kernel::error::code::OPERATION_FAILED))
        });
    }

    /// Fail with the thrown error's name and code, which the verb's
    /// rejection keeps.
    pub fn fail_with(&self, failure: HostFailure) {
        self.settle.fail(failure, "actions");
    }
}

/// One registration, as umbrella Task 0c's `RegistrationHandle`. Dropping
/// it keeps what it registered; `unregister` removes it, and only while it
/// is still the one installed. The closures that register run on the kernel
/// thread without `&self`, so this takes the post explicitly.
fn registration(remove: Disposer, post: KernelPost) -> Arc<RegistrationHandle> {
    RegistrationHandle::new(post, move || remove())
}

#[uniffi::export]
impl ActionsHandle {
    /// Last wins: a second executor for a type replaces the first (with a
    /// `duplicate-executor` diagnostic).
    pub async fn register_executor(
        &self,
        action_type: String,
        executor: Arc<dyn ActionExecutor>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let settle_post = post.clone();
                let run: plugin::ActionExecutor =
                    Rc::new(move |node: &Value, ctx: &plugin::ActionContext| {
                        let (promise, resolver) = Promise::pending();
                        let completion = Arc::new(ActionExecutorCompletion {
                            settle: ForeignSettle::new(
                                resolver,
                                settle_post.clone(),
                                "an ActionExecutor completion",
                            ),
                        });
                        executor.execute(
                            Arc::new(PdfActionNode(node.clone())),
                            ctx.into(),
                            completion,
                        );
                        Ok(promise)
                    });
                Ok(registration(open(c)?.register_executor(&action_type, run), post))
            })
            .await
    }

    /// `None` uninstalls. Installing one releases an `auto` open sequence.
    pub async fn set_ui_adapter(
        &self,
        handler: Option<Arc<dyn ActionUiHandler>>,
        support: ActionUiSupport,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let adapter = handler.map(|h| Rc::new(ui_adapter(h, support)));
                Ok(registration(open(c)?.set_ui_adapter(adapter), post))
            })
            .await
    }

    /// `None` uninstalls.
    pub async fn set_submit_handler(
        &self,
        handler: Option<Arc<dyn ActionSubmitHandler>>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let handler = handler.map(|h| -> plugin::ActionSubmitHandler {
                    Rc::new(
                        move |request: &plugin::ActionSubmitRequest,
                              _home: plugin::SubmitHandlerContext| {
                            h.submit(ActionSubmitRequest::try_from(request)?)
                                .map_err(Error::from)?;
                            Ok(None)
                        },
                    )
                });
                Ok(registration(open(c)?.set_submit_handler(handler), post))
            })
            .await
    }

    /// Runs the will- trees, then `operation`, then the did- trees, on the
    /// document's serial queue.
    pub async fn run_document_verb(
        &self,
        verb: DocumentVerb,
        operation: Arc<dyn DocumentVerbOperation>,
    ) -> Result<(), ViewerError> {
        let post = self.post.clone();
        self.cap
            .call_promise(
                move |c| {
                    Ok(open(c)?.run_document_verb(
                        verb,
                        move || {
                            let (promise, resolver) = Promise::pending();
                            operation.run(Arc::new(DocumentVerbCompletion {
                                settle: ForeignSettle::new(
                                    resolver,
                                    post,
                                    "a DocumentVerbOperation completion",
                                ),
                            }));
                            Ok(promise)
                        },
                        None,
                    ))
                },
                |()| (),
            )
            .await
    }
}
