//! `write/interchange.ts`: fields in and out as bundles: whole fields with
//! their widgets, values and scripts, taken out of one document and copied
//! into another, or the same.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::changes::{SendChange, StagedChange};
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
            crate::promise::adopt(me.enqueue.run_with(run, signal)).await
        })
    }

    /// `import(bundle, options)`: a bundle's fields copied into this
    /// document, as one change and one step to undo, staged on the
    /// document's queue as the engine verb's call after the fills on their
    /// way. `history: false` keeps it out of undo and redo.
    pub fn import(
        &self,
        bundle: Value,
        options: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            // `const { signal, history, ...importOptions } = options`.
            let history = get(&options, "history").cloned();
            let mut import_options = options.as_object().cloned().unwrap_or_default();
            import_options.shift_remove("signal");
            import_options.shift_remove("history");
            me.assert_allowed("doc.forms.modify", "form.import")?;
            let attribution =
                import_options.get("attribution").and_then(Value::as_str).unwrap_or("restore");
            if attribution == "restore" {
                me.assert_allowed("doc.forms.import", "form.import")?;
            }
            let run = {
                let (me, signal) = (me.clone(), signal.clone());
                move || -> Result<Promise<Value>, Error> {
                    let op_id =
                        import_options.get("opId").and_then(Value::as_str).map(str::to_string);
                    let send: SendChange = {
                        let (me, signal) = (me.clone(), signal.clone());
                        Rc::new(move |write: Value| {
                            let doc = match me.doc_or_throw() {
                                Ok(doc) => doc,
                                Err(e) => return Promise::rejected(e),
                            };
                            let mut options = import_options.clone();
                            options.insert(
                                "opId".into(),
                                get(&write, "opId").cloned().unwrap_or(Value::Null),
                            );
                            let call = me.cancellable(
                                signal.clone(),
                                doc.call(
                                    "forms.import",
                                    None,
                                    vec![bundle.clone(), Value::Object(options)],
                                ),
                            );
                            Promise::spawn(
                                async move { Ok(verb_answer("forms.import", call.await?)) },
                            )
                        })
                    };
                    let change = me.cx.changes()?.stage(StagedChange::verb(
                        json!({ "key": "form.import" }),
                        send,
                        op_id,
                        history != Some(Value::Bool(false)),
                    ))?;
                    let result = change.result();
                    Ok(Promise::spawn(async move {
                        let answer = result.await?;
                        let imported = get(&answer, "items")
                            .and_then(|i| i.get(0))
                            .cloned()
                            .unwrap_or(Value::Null);
                        // Wait for the annotation plugin to read the new
                        // widgets, as `create()` does.
                        if let Some(annotation) = me.annotation() {
                            annotation.when_synced().await?;
                        }
                        Ok(json!({
                            "fields": get(&imported, "fields").cloned().unwrap_or(Value::Null),
                            "refMap": get(&imported, "refMap").cloned().unwrap_or(Value::Null),
                            "dropped": get(&imported, "dropped").cloned().unwrap_or(Value::Null),
                        }))
                    }))
                }
            };
            crate::promise::adopt(me.enqueue.run_with(run, signal)).await
        })
    }
}

/// `{ items: [{ type, ...result }], meta: result.meta }`: a verb's result as
/// a change's answer.
fn verb_answer(kind: &str, result: Value) -> Value {
    let mut item = Map::new();
    item.insert("type".into(), json!(kind));
    for (k, v) in result.as_object().into_iter().flatten() {
        item.insert(k.clone(), v.clone());
    }
    let mut answer = Map::new();
    answer.insert("items".into(), json!([item]));
    answer
        .insert("meta".into(), result.get("meta").cloned().unwrap_or_else(epdf_kernel::undefined));
    Value::Object(answer)
}
