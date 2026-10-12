//! `write/interchange.ts`: FDF and XFDF export and import, and the repair
//! pass.

use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, undefined};
use serde_json::{Value, json};

use crate::controller::FormCapability;
use crate::core::model::Msg;
use crate::promise::{adopt, await_engine};
use crate::value::get;

impl FormCapability {
    /// `exportData(format = 'xfdf')`.
    pub fn export_data(&self, format: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
            let format = format.unwrap_or_else(|| json!("xfdf"));
            adopt(doc.call("forms.exportData", None, vec![format])).await
        })
    }

    /// `importData(data, format)`: sets the snapshot the engine answers,
    /// without a re-read.
    pub fn import_data(&self, data: Value, format: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
                let args = vec![data, format.unwrap_or_else(undefined)];
                let result = await_engine(doc.call("forms.importData", None, args)).await?;
                let snapshot = get(&result, "snapshot").cloned().unwrap_or(Value::Null);
                this.apply(Msg::Snapshot { snapshot: std::rc::Rc::new(snapshot) });
                Ok(result)
            }))
        })
    }

    /// `repair(options)`.
    pub fn repair(&self, options: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
                let args = vec![options.unwrap_or_else(undefined)];
                let result = await_engine(doc.call("forms.repair", None, args)).await?;
                this.refresh().await?;
                Ok(result)
            }))
        })
    }
}
