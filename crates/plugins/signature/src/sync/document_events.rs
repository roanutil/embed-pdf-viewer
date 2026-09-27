//! `sync/document-events.ts`: live facts. Parked signings, new versions,
//! remote signings, and the edits that make a re-judgement due.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt;
use serde_json::Value;

use crate::contract::SignaturePending;
use crate::controller::SignatureCapability;
use crate::model::SignatureAction;
use crate::promise::adopt;
use crate::sync::hydration::ValidateRequest;

impl SignatureCapability {
    /// `subscribeDocumentEvents(ctx, hydration)`.
    pub(crate) fn subscribe_document_events(&self) -> Result<(), Error> {
        let doc = self.cx().doc()?;
        let this = self.clone();
        let unsubscribe =
            doc.subscribe(Rc::new(move |event: &Value| this.on_document_event(event)));
        self.cx().listen(unsubscribe);
        Ok(())
    }

    fn on_document_event(&self, event: &Value) {
        match event.get("type").and_then(Value::as_str).unwrap_or_default() {
            "signature.prepared" => self.dispatch(SignatureAction::Pending {
                pending: Some(SignaturePending {
                    signing_id: event
                        .get("signingId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    field: event.get("field").cloned().unwrap_or(Value::Null),
                }),
            }),
            "signature.aborted" | "signature.completed" => {
                self.dispatch(SignatureAction::Pending { pending: None })
            }
            // Byte-level facts moved: re-read them and re-judge.
            "document.versioned" | "stream.desynced" => self.refresh_then(false),
            // The field set changed: the snapshot lists fields, so re-read
            // it; and the change modifies the working copy, so re-judge.
            "form.fieldCreated"
            | "form.fieldDeleted"
            | "form.widgetAttached"
            | "form.widgetDetached"
            | "form.imported"
            | "form.repaired" => self.refresh_then(true),
            // An edit of the working copy.
            "annotation.created"
            | "annotation.updated"
            | "annotation.deleted"
            | "annotation.moved"
            | "form.valueChanged"
            | "form.fieldUpdated"
            | "form.effectsApplied" => self.revalidate_soon(),
            _ => {}
        }
    }

    /// `void refresh().then(() => validate() | revalidateSoon()).catch(report)`.
    fn refresh_then(&self, soon: bool) {
        let this = self.clone();
        let refresh = self.refresh();
        rt::spawn(async move {
            let outcome: Result<(), Error> = async {
                refresh.await?;
                if soon {
                    this.revalidate_soon();
                } else {
                    adopt(this.validate(ValidateRequest::default())).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = outcome {
                rt::tick().await;
                this.console_error("[signature] refresh failed:", &error);
            }
        });
    }
}
