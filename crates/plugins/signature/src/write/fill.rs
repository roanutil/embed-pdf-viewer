//! `write/fill.ts`: visual fills. The mark becomes the widget's appearance;
//! nothing is sealed.

use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::blank_page::blank_page_pdf;
use crate::contract::{Mark, OperationOptions, SignatureClearedEvent, SignatureFilledEvent};
use crate::controller::SignatureCapability;
use crate::js::truthy;
use crate::promise::adopt;
use crate::services::errors::verb;

impl SignatureCapability {
    /// `setAppearance(field, pdf, options)`: `unsupported` without the
    /// engine door, `conflict` on a signed field.
    fn set_appearance(&self, field: Value, pdf: Vec<u8>, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.cx().doc()?;
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
                return Err(plugin_error(
                    code::CONFLICT,
                    "signature",
                    format!("'{}' is signed", crate::js::template(signature.get("fieldName"))),
                    None,
                    None,
                ));
            }
            let call = doc.call(
                "forms.setSignatureAppearance",
                None,
                vec![field, json!({ "pdf": Bytes(pdf).to_json() })],
            );
            this.cancellable(options.signal, call).await?;
            Ok(())
        })
    }

    /// `fillField(field, mark, options)`, unwrapped: the placement rule
    /// calls it as it is.
    pub(crate) fn fill_field_raw(
        &self,
        field: Value,
        mark: Mark,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_allowed_field("fill", &field, "signature.fillField")?;
            let busy = this.clone();
            adopt(this.with_busy(move || {
                Promise::spawn(async move {
                    let bytes = busy.mark_bytes(&mark).await?;
                    busy.set_appearance(field.clone(), bytes, options).await?;
                    busy.clear_if_target(&field);
                    busy.0.events.filled.emit(&SignatureFilledEvent { field });
                    Ok(())
                })
            }))
            .await
        })
    }

    /// `fillField(field, mark, options)`: draw a mark into a field without
    /// signing. Fires `onFilled`. Rejects `permission-denied` for a field
    /// you may not fill in, `conflict` on a signed field.
    pub fn fill_field(&self, field: Value, mark: Mark, options: OperationOptions) -> Promise<()> {
        verb(self.fill_field_raw(field, mark, options))
    }

    /// `clearField(field, options)`: take a drawn mark out of a field.
    /// Fires `onCleared`.
    pub fn clear_field(&self, field: Value, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            this.assert_allowed_field("fill", &field, "signature.clearField")?;
            let busy = this.clone();
            adopt(this.with_busy(move || {
                Promise::spawn(async move {
                    busy.set_appearance(field.clone(), blank_page_pdf(), options).await?;
                    busy.0.events.cleared.emit(&SignatureClearedEvent { field });
                    Ok(())
                })
            }))
            .await
        }))
    }
}
