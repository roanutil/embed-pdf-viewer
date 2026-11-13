//! `write/crud.ts`: create, update, delete and reorder, the public verbs in
//! the engine's own terms, and the transfers (export, import, a resource's
//! bytes). Each change states itself through `store.apply`, so it shows at
//! once and is sent, answered and refused exactly like a gesture's, and each
//! resolves with what the engine wrote for it.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_annotation_core::engine::facts::field_names;
use epdf_annotation_core::links::link_children_of;
use epdf_annotation_core::types::Id;
use epdf_annotation_core::update::defaults_for;

use epdf_kernel::Error;
use epdf_kernel::changes::{SendChange, StagedChange};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::controller::{CAPABILITY, Core};
use crate::engine::anchor_of;
use crate::js::{Object, key_or_none, page_object_number};
use crate::services::staging::StoreChange;
use crate::services::store::{Applied, ApplyOptions};
use crate::write::outcomes::{applied_annotation_of, applied_or_throw};

fn not_found(reference: &Value) -> Error {
    plugin_error(
        code::NOT_FOUND,
        CAPABILITY,
        format!("no annotation {}", key_or_none(reference)),
        None,
        None,
    )
}

/// `create`'s options.
#[derive(Clone, Default)]
pub struct CreateOptions {
    pub signal: Option<AbortSignal>,
    pub tool: Option<String>,
    pub select: bool,
}

impl Core {
    /// The annotation `ref` names, as the view holds it.
    fn current(&self, reference: &Value) -> Result<Value, Error> {
        match self.get(reference) {
            Some(annotation) => Ok(json!({ "annotation": annotation })),
            None => Err(not_found(reference)),
        }
    }

    /// `withTool(draft, toolId)`: `draft` over a tool's defaults and flags.
    fn with_tool(&self, draft: &Object, tool_id: &str) -> Result<Object, Error> {
        let Some(tool) = self.tool(tool_id) else {
            return Err(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("no tool '{tool_id}'"),
                None,
                None,
            ));
        };
        let subtype = draft.get("subtype").and_then(Value::as_str).unwrap_or("");
        let declared = field_names(subtype);
        let mut out = Map::new();
        for (k, v) in tool.flags.iter().flatten() {
            out.insert(k.clone(), v.clone());
        }
        for (k, v) in defaults_for(&self.model(), &tool.preset) {
            if k != "richText" && declared.contains(&k.as_str()) {
                out.insert(k, v);
            }
        }
        for (k, v) in draft {
            out.insert(k.clone(), v.clone());
        }
        Ok(out)
    }

    /// `create(page, draft, resources, options)`.
    pub fn create(
        &self,
        page: &Value,
        fields: &Object,
        resources: Option<Value>,
        options: CreateOptions,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.create") {
            return Promise::rejected(e);
        }
        let reference = match self.page_ref_of(page) {
            Ok(r) => r,
            Err(e) => return Promise::rejected(e),
        };
        if fields.get("subtype").and_then(Value::as_str) == Some("unsupported") {
            return Promise::rejected(plugin_error(
                code::INVALID_INPUT,
                CAPABILITY,
                "a kind the engine doesn't know can't be created",
                None,
                None,
            ));
        }
        let stated = match &options.tool {
            Some(tool) => match self.with_tool(fields, tool) {
                Ok(d) => d,
                Err(e) => return Promise::rejected(e),
            },
            None => fields.clone(),
        };
        let applied = self.apply_when_numbered(
            vec![StoreChange::Create { page: reference, draft: stated, resources }],
            options.select,
        );
        let me = self.weak();
        let signal = options.signal.clone();
        Promise::spawn(async move {
            let applied = applied.await?;
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            let annotation = core.cancellable(signal, applied_annotation_of(&applied, 0));
            drop(core);
            match annotation.await? {
                Some(annotation) => Ok(json!({ "annotation": annotation })),
                None => Err(plugin_error(
                    code::OPERATION_FAILED,
                    CAPABILITY,
                    "the annotation could not be created",
                    None,
                    None,
                )),
            }
        })
    }

    /// `update(ref, patch, resources, options)`.
    pub fn update(
        &self,
        reference: &Value,
        patch: &Object,
        resources: Option<Value>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let applied = match self.apply(
            &[StoreChange::Update {
                reference: reference.clone(),
                patch: patch.clone(),
                resources,
            }],
            false,
        ) {
            Ok(a) => a,
            Err(e) => return Promise::rejected(e),
        };
        let annotation = self.cancellable(signal, applied_annotation_of(&applied, 0));
        let me = self.weak();
        let reference = reference.clone();
        Promise::spawn(async move {
            match annotation.await? {
                Some(annotation) => Ok(json!({ "annotation": annotation })),
                None => match me.upgrade() {
                    Some(core) => core.current(&reference),
                    None => Err(not_found(&reference)),
                },
            }
        })
    }

    /// `delete(ref, options)`.
    pub fn delete(&self, reference: &Value, signal: Option<AbortSignal>) -> Promise<()> {
        let applied =
            match self.apply(&[StoreChange::Delete { reference: reference.clone() }], false) {
                Ok(a) => a,
                Err(e) => return Promise::rejected(e),
            };
        let done = self.cancellable(signal, applied_or_throw(&applied));
        Promise::spawn(async move {
            done.await?;
            Ok(())
        })
    }

    /// `reorder(refs, position, options)`: one page's drawing order.
    pub fn reorder(
        &self,
        refs: &[Value],
        position: &Value,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        if refs.is_empty() {
            return Promise::resolved(());
        }
        let model = self.model();
        let record_of =
            |r: &Value| model.by_id.get(&key_or_none(r)).cloned().ok_or_else(|| not_found(r));
        let moving = match refs.iter().map(record_of).collect::<Result<Vec<_>, _>>() {
            Ok(m) => m,
            Err(e) => return Promise::rejected(e),
        };
        let neighbour = match anchor_of(position) {
            Some(anchor) => match record_of(anchor) {
                Ok(r) => Some(r),
                Err(e) => return Promise::rejected(e),
            },
            None => None,
        };
        let page = moving[0].annotation.get("page").cloned().unwrap_or(Value::Null);
        let number = page_object_number(&page);
        let other = |r: &std::rc::Rc<epdf_annotation_core::types::ModelAnnotation>| {
            r.page_number() != Some(number)
        };
        if moving.iter().any(other) || neighbour.as_ref().is_some_and(other) {
            return Promise::rejected(plugin_error(
                code::INVALID_INPUT,
                CAPABILITY,
                "a reorder stays on one page: the annotations and their neighbour",
                None,
                None,
            ));
        }
        let refs: Vec<Value> = moving
            .iter()
            .map(|r| r.annotation.get("ref").cloned().unwrap_or(Value::Null))
            .collect();
        let applied = match self
            .apply(&[StoreChange::Reorder { page, refs, position: position.clone() }], false)
        {
            Ok(a) => a,
            Err(e) => return Promise::rejected(e),
        };
        let done = self.cancellable(signal, applied_or_throw(&applied));
        Promise::spawn(async move {
            done.await?;
            Ok(())
        })
    }

    /// The pages a selection names, as refs.
    fn page_refs_of(&self, pages: Option<&Value>) -> Result<Option<Vec<Value>>, Error> {
        let Some(pages) = pages.and_then(Value::as_array) else { return Ok(None) };
        pages.iter().map(|p| self.page_ref_of(p)).collect::<Result<Vec<_>, _>>().map(Some)
    }

    /// `settle()`: send the words typed in the last moment, and wait for every change on its way.
    pub(crate) fn settle_writes(&self) -> Promise<()> {
        let typed = self.send_typing();
        let me = self.weak();
        Promise::spawn(async move {
            typed.await?;
            if let Some(core) = me.upgrade() {
                let written = core.when_written();
                drop(core);
                written.await?;
            }
            Ok(())
        })
    }

    /// `export(selection, options)`: annotations taken out as a bundle.
    pub fn export(&self, selection: &Value, signal: Option<AbortSignal>) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("doc.download", "annotation.export") {
            return Promise::rejected(e);
        }
        let pages = match self
            .page_refs_of(selection.get("pages").filter(|p| !epdf_kernel::is_undefined(p)))
        {
            Ok(p) => p,
            Err(e) => return Promise::rejected(e),
        };
        let settled = self.cancellable(signal.clone(), self.settle_writes());
        let me = self.weak();
        let refs = selection.get("refs").filter(|r| !epdf_kernel::is_undefined(r)).cloned();
        Promise::spawn(async move {
            settled.await?;
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            let doc = core.doc_or_throw()?;
            let mut what = Map::new();
            if let Some(refs) = refs {
                what.insert("refs".into(), refs);
            }
            if let Some(pages) = pages {
                what.insert("pages".into(), Value::Array(pages));
            }
            let call = core.cancellable(
                signal,
                doc.call("annotations.export", None, vec![Value::Object(what)]),
            );
            drop(core);
            call.await
        })
    }

    /// `import(bundle, options)`: one change on the queue, staged as the
    /// engine verb's call: what was typed is sent first, and the history keeps
    /// it as one step unless `history: false` says it isn't one.
    pub fn import(
        &self,
        bundle: Value,
        options: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.import") {
            return Promise::rejected(e);
        }
        // `const { signal, history, ...importOptions } = options`.
        let history = options.get("history").cloned();
        let mut import_options = options.as_object().cloned().unwrap_or_default();
        import_options.shift_remove("signal");
        import_options.shift_remove("history");
        let op_id = import_options.get("opId").and_then(Value::as_str).map(str::to_string);
        let send: SendChange = {
            let me = self.weak();
            Rc::new(move |write: Value| {
                let Some(core) = me.upgrade() else {
                    return Promise::rejected(Error::new("closed"));
                };
                let doc = match core.doc_or_throw() {
                    Ok(d) => d,
                    Err(e) => return Promise::rejected(e),
                };
                let mut options = import_options.clone();
                options.insert("opId".into(), write.get("opId").cloned().unwrap_or(Value::Null));
                let call = core.cancellable(
                    signal.clone(),
                    doc.call(
                        "annotations.import",
                        None,
                        vec![bundle.clone(), Value::Object(options)],
                    ),
                );
                Promise::spawn(async move { Ok(verb_answer("annotations.import", call.await?)) })
            })
        };
        let staged = self.cx.changes().and_then(|changes| {
            changes.stage(StagedChange::verb(
                json!({ "key": "annotation.import" }),
                send,
                op_id,
                history != Some(Value::Bool(false)),
            ))
        });
        let change = match staged {
            Ok(change) => change,
            Err(e) => return Promise::rejected(e),
        };
        let result = change.result();
        let me = self.weak();
        Promise::spawn(async move {
            let answer = result.await?;
            let imported =
                answer.get("items").and_then(|i| i.get(0)).cloned().unwrap_or(Value::Null);
            // The engine published one `annotations.created` per annotation:
            // once the records hold them, `list()` shows them.
            if let Some(core) = me.upgrade() {
                let settled = core.records.settled();
                drop(core);
                settled.await?;
            }
            // A member the engine left out is `undefined`, as the TypeScript's literal has it.
            let member = |k: &str| imported.get(k).cloned().unwrap_or_else(epdf_kernel::undefined);
            Ok(json!({
                "annotations": member("annotations"),
                "refMap": member("refMap"),
                "dropped": member("dropped"),
            }))
        })
    }

    /// `duplicate(refs, options, lent)`: copy annotations in this document,
    /// as one change: the copies show at once, and it is one step to undo. A
    /// copy carries its original's fields, its drawing, its file and its
    /// link; it has its own ref and name. Resolves `{ annotations, dropped }`.
    ///
    /// `lent` are the selection's lent records (a form's widgets, in design
    /// mode), which only the selection's own duplicate passes: their lender
    /// says which ops copy them, and those join the same change.
    pub fn duplicate(
        &self,
        refs: Vec<Value>,
        options: DuplicateOptions,
        lent: Vec<Value>,
    ) -> Promise<Value> {
        if (!refs.is_empty() || lent.is_empty())
            && let Err(e) = self.assert_allowed("annotations:create", "annotation.duplicate")
        {
            return Promise::rejected(e);
        }
        if refs.is_empty() && lent.is_empty() {
            return Promise::resolved(json!({ "annotations": [], "dropped": [] }));
        }
        let page = match &options.page {
            None => None,
            Some(page) => match self.page_ref_of(page) {
                Ok(reference) => Some(reference),
                Err(e) => return Promise::rejected(e),
            },
        };
        let offset = options.offset.clone();
        let lender = if lent.is_empty() { None } else { self.lending.lender() };
        // The copies, staged on the store once the lender's ops are there.
        let start = {
            let (me, refs, page, offset) =
                (self.weak(), refs.clone(), page.clone(), offset.clone());
            let select = options.select;
            move |lent_copy: Option<(Vec<Value>, Vec<Value>)>| -> Result<(Promise<Applied>, Vec<Id>), Error> {
                let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
                let model = core.model();
                let stated: HashSet<String> = refs.iter().map(key_or_none).collect();
                let along: Vec<Value> = refs
                    .iter()
                    .flat_map(|reference| {
                        link_children_of(&model, &key_or_none(reference))
                            .into_iter()
                            .filter_map(|child| child.annotation.get("ref").cloned())
                            .collect::<Vec<_>>()
                    })
                    .filter(|child| !stated.contains(&key_or_none(child)))
                    .collect();
                let lent_ids: Vec<Id> =
                    lent_copy.as_ref().map(|(_, copies)| copies.iter().map(key_or_none).collect()).unwrap_or_default();
                let changes = if refs.is_empty() {
                    Vec::new()
                } else {
                    vec![StoreChange::Copy { refs: refs.clone(), along, page: page.clone(), offset: offset.clone() }]
                };
                let owner = core.lending.lender().map(|l| l.owner()).unwrap_or_default();
                let options = ApplyOptions {
                    select,
                    label: lent_copy.as_ref().map(|_| {
                        let key = if refs.is_empty() { format!("{owner}.duplicate") } else { "annotation.duplicate".to_string() };
                        json!({ "key": key, "count": refs.len() + lent_ids.len() })
                    }),
                    lent: lent_copy.map(|(ops, _)| (ops, lent_ids.clone())),
                };
                Ok((core.apply_when_numbered_with(changes, options), lent_ids))
            }
        };
        let start = Rc::new(start);
        let started = match &lender {
            None => match start(None) {
                Ok(started) => Some(started),
                Err(e) => return Promise::rejected(e),
            },
            Some(_) => None,
        };
        let copy_ops = lender.map(|l| l.copy_ops(&lent, page, offset));
        let me = self.weak();
        let signal = options.signal.clone();
        let named_refs = !refs.is_empty();
        Promise::spawn(async move {
            let (applied, lent_ids) = match (started, copy_ops) {
                (Some(started), _) => started,
                (None, Some(copy_ops)) => start(Some(copy_ops.await?))?,
                (None, None) => start(None)?,
            };
            let applied = applied.await?;
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            let outcome = core.cancellable(signal, applied_or_throw(&applied));
            drop(core);
            let outcome = outcome.await?;
            let items: Vec<Value> = outcome
                .written
                .result
                .as_ref()
                .and_then(|r| r.get("items"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let copied = items
                .iter()
                .find(|item| item.get("type").and_then(Value::as_str) == Some("annotations.copy"));
            if named_refs && copied.is_none() {
                return Err(plugin_error(
                    code::OPERATION_FAILED,
                    CAPABILITY,
                    "the annotations could not be copied",
                    None,
                    None,
                ));
            }
            // The copies of the annotations named: a link that came along isn't one.
            let named: HashSet<&str> = applied.ids.iter().map(String::as_str).collect();
            let mut annotations: Vec<Value> = copied
                .and_then(|c| c.get("annotations"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|copy| {
                    named.contains(key_or_none(copy.get("ref").unwrap_or(&Value::Null)).as_str())
                })
                .cloned()
                .collect();
            // A lent record's copy, as the last op that names it left it.
            let mut rows: HashMap<String, Value> = HashMap::new();
            for item in &items {
                let made: Vec<Value> = match item.get("type").and_then(Value::as_str) {
                    Some("forms.copy" | "forms.addWidget") => {
                        item.get("widgets").and_then(Value::as_array).cloned().unwrap_or_default()
                    }
                    Some("forms.updateWidget") => item.get("widget").cloned().into_iter().collect(),
                    _ => Vec::new(),
                };
                for row in made {
                    rows.insert(key_or_none(row.get("ref").unwrap_or(&Value::Null)), row);
                }
            }
            annotations.extend(lent_ids.iter().filter_map(|id| rows.get(id).cloned()));
            let dropped =
                copied.and_then(|c| c.get("dropped")).cloned().unwrap_or_else(|| json!([]));
            Ok(json!({ "annotations": annotations, "dropped": dropped }))
        })
    }

    /// `downloadResource(ref, role, options)`.
    pub fn download_resource(
        &self,
        reference: &Value,
        role: &str,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("doc.download", "annotation.downloadResource") {
            return Promise::rejected(e);
        }
        if let Err(e) = self.current(reference) {
            return Promise::rejected(e);
        }
        let doc = match self.doc_or_throw() {
            Ok(d) => d,
            Err(e) => return Promise::rejected(e),
        };
        let page = self.annotation_page_of(reference);
        let pon = page.get("objectNumber").and_then(Value::as_u64).map(|n| n as u32);
        let call = self.cancellable(
            signal,
            doc.call(
                "page.annotations.downloadResource",
                pon,
                vec![reference.clone(), json!(role)],
            ),
        );
        // `return` from the `async` function: its promise adopts the call's,
        // a turn for the resolve-thenable job, then the reaction.
        Promise::spawn(async move {
            epdf_kernel::rt::tick().await;
            call.reaction().await
        })
    }
}

/// `AnnotationDuplicateOptions & OperationOptions`.
#[derive(Clone, Default)]
pub struct DuplicateOptions {
    /// The page the copies go on, as a ref or an index; `None`: each on its own page.
    pub page: Option<Value>,
    /// How far each copy moves, `{ x, y }` in page points; `None`: not at all.
    pub offset: Option<Value>,
    /// Select the copies as they show.
    pub select: bool,
    pub signal: Option<AbortSignal>,
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
