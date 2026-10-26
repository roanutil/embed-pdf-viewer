//! `write/interchange.ts`: fields in and out as bundles: whole fields with
//! their widgets, values and scripts, taken out of one document and copied
//! into another, or the same.

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::value::get;

impl crate::controller::Core {
    /// `export(selection, options)`: fields out as a bundle. Waits for
    /// pending writes first.
    pub fn export(&self, selection: Option<Value>, signal: Option<AbortSignal>) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.assert_allowed("doc.forms.read", "form.export")?;
            me.assert_allowed("doc.download", "form.export")?;
            let selection = selection.unwrap_or_else(|| json!({}));
            let pages = match get(&selection, "pages").and_then(Value::as_array) {
                Some(pages) => Some(
                    pages
                        .iter()
                        .map(|page| {
                            me.page_of(page)
                                .map(|info| get(&info, "ref").cloned().unwrap_or(Value::Null))
                        })
                        .collect::<Result<Vec<_>, Error>>()?,
                ),
                None => None,
            };
            let mut args = Map::new();
            if let Some(fields) =
                get(&selection, "fields").filter(|f| crate::value::truthy(Some(f)))
            {
                args.insert("fields".into(), fields.clone());
            }
            if let Some(pages) = pages {
                args.insert("pages".into(), Value::Array(pages));
            }
            // What the user sees is what the bundle carries: pending writes land first.
            let run = {
                let (me, signal) = (me.clone(), signal.clone());
                move || -> Result<Promise<Value>, Error> {
                    let doc = me.doc_or_throw()?;
                    Ok(me.cancellable(
                        signal,
                        doc.call("forms.export", None, vec![Value::Object(args)]),
                    ))
                }
            };
            crate::promise::adopt(me.enqueue.run(run, signal)).await
        })
    }

    /// `import(bundle, options)`: a bundle's fields copied into this
    /// document, as one change you can undo.
    pub fn import(
        &self,
        bundle: Value,
        options: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            me.assert_allowed("doc.forms.modify", "form.import")?;
            let attribution =
                get(&options, "attribution").and_then(Value::as_str).unwrap_or("restore");
            if attribution == "restore" {
                me.assert_allowed("doc.forms.import", "form.import")?;
            }
            let run = {
                let (me, signal) = (me.clone(), signal.clone());
                move || -> Result<Promise<Value>, Error> {
                    let doc = me.doc_or_throw()?;
                    let call = doc.call("forms.import", None, vec![bundle, options]);
                    let answer = me.cancellable(signal, call);
                    Ok(Promise::spawn(async move {
                        let result = answer.await?;
                        // Wait for the annotation plugin to read the new
                        // widgets, as `create()` does.
                        if let Some(annotation) = me.annotation() {
                            annotation.when_synced().await?;
                        }
                        Ok(json!({
                            "fields": get(&result, "fields").cloned().unwrap_or(Value::Null),
                            "refMap": get(&result, "refMap").cloned().unwrap_or(Value::Null),
                            "dropped": get(&result, "dropped").cloned().unwrap_or(Value::Null),
                        }))
                    }))
                }
            };
            crate::promise::adopt(me.enqueue.run(run, signal)).await
        })
    }
}
