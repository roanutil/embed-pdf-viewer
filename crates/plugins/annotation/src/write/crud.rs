//! `write/crud.ts`: create, update, delete and reorder, the public verbs in
//! the engine's own terms, and the transfers (export, import, a resource's
//! bytes). Each change states itself through `store.apply`, so it shows at
//! once and is sent, answered and refused exactly like a gesture's, and each
//! resolves with what the engine wrote for it.

use epdf_annotation_core::engine::facts::field_names;
use epdf_annotation_core::update::defaults_for;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::controller::{CAPABILITY, Core};
use crate::engine::anchor_of;
use crate::js::{Object, key_or_none, page_object_number};
use crate::services::staging::StoreChange;
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

    /// `import(bundle, options)`.
    pub fn import(
        &self,
        bundle: Value,
        options: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.import") {
            return Promise::rejected(e);
        }
        let doc = match self.doc_or_throw() {
            Ok(d) => d,
            Err(e) => return Promise::rejected(e),
        };
        let call =
            self.cancellable(signal, doc.call("annotations.import", None, vec![bundle, options]));
        let me = self.weak();
        Promise::spawn(async move {
            let result = call.await?;
            if let Some(core) = me.upgrade() {
                let settled = core.records.settled();
                drop(core);
                settled.await?;
            }
            Ok(json!({
                "annotations": result.get("annotations").cloned().unwrap_or(json!([])),
                "refMap": result.get("refMap").cloned().unwrap_or(json!([])),
                "dropped": result.get("dropped").cloned().unwrap_or(json!([])),
            }))
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
        self.cancellable(
            signal,
            doc.call(
                "page.annotations.downloadResource",
                pon,
                vec![reference.clone(), json!(role)],
            ),
        )
    }
}
