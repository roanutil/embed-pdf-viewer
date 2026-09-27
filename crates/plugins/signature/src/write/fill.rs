//! `write/fill.ts`: visual fills. The mark becomes the widget's appearance;
//! nothing is sealed.

use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::blank_page::blank_page_pdf;
use crate::contract::{Mark, SignatureFieldEvent};
use crate::controller::SignatureCapability;
use crate::promise::await_engine;
use crate::read::signatures::truthy;
use crate::services::events::origin_api;

impl SignatureCapability {
    /// `setAppearance(field, pdf)`: `unsupported` without the engine door,
    /// `conflict` on a signed field; then the form is re-read.
    fn set_appearance(&self, field: Value, pdf: Vec<u8>) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_doc()?;
            if !doc.has("forms.setSignatureAppearance") {
                return Err(plugin_error(
                    code::UNSUPPORTED,
                    "signature",
                    "this engine cannot draw into a signature field",
                    None,
                    None,
                ));
            }
            if let Some(signature) = this.get_signature(&field)
                && signature.get("signed").is_some_and(truthy)
            {
                let name = match signature.get("fieldName") {
                    Some(Value::String(name)) => name.clone(),
                    Some(Value::Null) => "null".into(),
                    Some(other) => other.to_string(),
                    None => "undefined".into(),
                };
                return Err(plugin_error(
                    code::CONFLICT,
                    "signature",
                    format!("'{name}' is signed"),
                    None,
                    None,
                ));
            }
            await_engine(doc.call(
                "forms.setSignatureAppearance",
                None,
                vec![field, json!({ "pdf": Bytes(pdf).to_json(), "pageIndex": 0 })],
            ))
            .await?;
            this.form().refresh().await
        })
    }

    /// `fillField(field, mark)`: draw the mark into the field; a signed
    /// field refuses `conflict`.
    pub fn fill_field(&self, field: Value, mark: Mark) -> Promise<()> {
        let this = self.clone();
        self.with_busy(move || {
            Promise::spawn(async move {
                let bytes = this.mark_bytes(&mark).await?;
                this.set_appearance(field.clone(), bytes).await?;
                this.clear_if_target(&field);
                this.0.events.filled.emit(&SignatureFieldEvent { field, origin: origin_api() });
                Ok(())
            })
        })
    }

    /// `clearField(field)`: undo a visual fill with a blank appearance.
    pub fn clear_field(&self, field: Value) -> Promise<()> {
        let this = self.clone();
        self.with_busy(move || {
            Promise::spawn(async move {
                this.set_appearance(field.clone(), blank_page_pdf()).await?;
                this.0.events.cleared.emit(&SignatureFieldEvent { field, origin: origin_api() });
                Ok(())
            })
        })
    }
}
