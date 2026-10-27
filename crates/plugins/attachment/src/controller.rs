//! `controller.ts`: the attachments controller. The document's attachments
//! are a kernel mirror, read when something asks for them
//! (`ensure_loaded()`, what the state view does), and every read shows its
//! `view()`: the engine's attachments with this session's pending adds and
//! removes on top. A page's file is a file attachment annotation, read from
//! the annotation plugin, so the list shows both homes with no read of its
//! own. Each add or remove of an attachment is one change on the document's
//! queue: it shows at once, is one step of the history, and undoing it
//! shows the files as they were.
//!
//! Both optional plugins are looked up when they are needed
//! (`ctx.tryGet`): the annotation plugin (a page's files, removing and
//! saving them) and the stage (going to a page's file).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use epdf_geometry::Rect;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::changes::{PendingChange, StagedChange};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::mirror::{Fold, Load, Loaded, Mirror, MirrorChange, MirrorSpec, ResourceStatus};
use epdf_kernel::plugin::{ConnectFn, Controller};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe, undefined};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_stage::{
    PageTarget, RevealAnchor, RevealAnchorValue, RevealOptions, RevealWord, STAGE,
    ScrollBehaviorKind,
};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::model::*;
use crate::names::display_name_of;
use crate::sync::{fold_files, predict_files, undo_of};

pub(crate) const CAPABILITY: &str = "attachment";
const MODIFY: &str = "doc.attachments.modify";
const DOWNLOAD: &str = "doc.download";

/// `list`: the view and the annotation list it was read from.
type ListMemo = (Rc<Value>, Option<Rc<Vec<Value>>>, Rc<Vec<Value>>);
/// `getSelected`: the list and the selected ids it was read from.
type SelectedMemo = (Rc<Vec<Value>>, Rc<Vec<Value>>, Rc<Vec<Value>>);

/// The memos behind the reads (`memo`): each holds the inputs it was
/// computed from and answers the same `Rc` while they are the same `Rc`s
/// (`Rc::ptr_eq`, the TypeScript's `Object.is` on each input).
#[derive(Default)]
struct Memos {
    list: Option<ListMemo>,
    selected: Option<SelectedMemo>,
}

fn same_input<T>(held: &Option<Rc<T>>, now: &Option<Rc<T>>) -> bool {
    match (held, now) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `TypeError: Cannot read properties of undefined (reading '<member>')`.
fn type_error(member_name: &str) -> Error {
    Error::named(
        "TypeError",
        format!("Cannot read properties of undefined (reading '{member_name}')"),
    )
}

/// `file.home.kind === 'page'`.
fn is_page_file(file: &Value) -> bool {
    file.get("home").and_then(|h| h.get("kind")).and_then(Value::as_str) == Some("page")
}

/// `file.home.ref`.
fn home_ref(file: &Value) -> Value {
    file.get("home").map_or_else(undefined, |home| member(home, "ref"))
}

/// `factOf(event, previous)`: the fact a confirmed attachment event
/// reports, `None` for any other event.
fn fact_of(event: &Value, previous: &Value) -> Option<(&'static str, Vec<Value>)> {
    Some(match event.get("type").and_then(Value::as_str)? {
        "attachments.created" => ("added", vec![document_file_of(&member(event, "attachment"))]),
        "attachments.restored" => {
            ("restored", vec![document_file_of(&member(event, "attachment"))])
        }
        "attachments.deleted" => {
            // `previous?.find((file) => file.ref.key === event.deleted?.key)`
            let key = event
                .get("deleted")
                .filter(|d| truthy(Some(d)))
                .map_or_else(undefined, |d| member(d, "key"));
            let gone = previous
                .as_array()
                .and_then(|files| files.iter().find(|file| strict_equals(&key_of(file), &key)))
                .map(document_file_of);
            ("removed", gone.into_iter().collect())
        }
        _ => return None,
    })
}

pub(crate) struct Core {
    cx: Controller<AttachmentState, Value>,
    files: Mirror,
    changed: EventHookSource<AttachmentsChangedEvent>,
    resynced: EventHookSource<AttachmentsResyncedEvent>,
    memos: RefCell<Memos>,
}

/// `AttachmentCapability`: the api `createAttachmentController` returns.
#[derive(Clone)]
pub struct AttachmentCapability(pub(crate) Rc<Core>);

impl Core {
    fn annotations(&self) -> Option<AnnotationCapability> {
        self.cx.try_get(&ANNOTATION).ok().flatten()
    }

    fn list(&self) -> Rc<Vec<Value>> {
        let view = self.files.view();
        let annotations = self.annotations().map(|a| a.list(None));
        let mut memos = self.memos.borrow_mut();
        if let Some((held_view, held_annotations, list)) = &memos.list
            && Rc::ptr_eq(held_view, &view)
            && same_input(held_annotations, &annotations)
        {
            return list.clone();
        }
        let mut all: Vec<Value> =
            view.as_array().map(|a| a.iter().map(document_file_of).collect()).unwrap_or_default();
        if let Some(annotations) = &annotations {
            all.extend(annotations.iter().filter_map(page_file_of));
        }
        let list = Rc::new(all);
        memos.list = Some((view, annotations, list.clone()));
        list
    }

    /// `isPending(file)`.
    fn is_pending(&self, file: &Value) -> bool {
        if is_page_file(file) {
            return self.annotations().is_some_and(|a| a.is_pending(&home_ref(file)));
        }
        let key = member(&home_ref(file), "key");
        let confirmed = self.files.get();
        !confirmed
            .as_array()
            .is_some_and(|files| files.iter().any(|f| strict_equals(&key_of(f), &key)))
    }

    /// `refuse(permission, operation, options)`: without the permission, or
    /// once the caller cancelled, refuse before anything starts.
    fn refuse(
        &self,
        permission: &str,
        operation: &str,
        signal: Option<&AbortSignal>,
    ) -> Result<(), Error> {
        self.cx.assert_allowed(permission, operation)?;
        if signal.is_some_and(AbortSignal::aborted) {
            return Err(plugin_error(
                code::OPERATION_CANCELLED,
                CAPABILITY,
                format!("{operation} was cancelled"),
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `answer(change, options)`: the engine's answer to a staged change.
    async fn answer(
        self: Rc<Self>,
        change: PendingChange,
        signal: Option<AbortSignal>,
    ) -> Result<Value, Error> {
        let answered = cancellable(CAPABILITY, signal, change.result()).await;
        answered.map_err(|error| to_plugin_error(CAPABILITY, &error))
    }

    /// `annotationsFor(operation)`: the annotation plugin, for a page's
    /// file; the list shows none without it.
    fn annotations_for(&self, operation: &str) -> Result<AnnotationCapability, Error> {
        self.annotations().ok_or_else(|| {
            plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("{operation}: no annotation plugin"),
                None,
                None,
            )
        })
    }

    /// `stageChange`: one user action; it shows at once, and undoing it
    /// shows the files as they were.
    fn stage(
        &self,
        label: Value,
        ops: Vec<Value>,
        view: &Rc<Value>,
    ) -> Result<PendingChange, Error> {
        let undo = undo_of(view, &ops)?;
        let mut change = StagedChange::ops(label, ops);
        change.undo = undo;
        self.cx.changes()?.stage(change)
    }
}

/// `createOp(source, taken)`: the op that attaches `source` under a name no
/// file has (`taken`, which it joins).
fn create_op(source: &Value, taken: &mut HashSet<String>) -> Result<Value, Error> {
    let data = member(source, "data");
    // `source.name ?? (blob && 'name' in blob ? String(blob.name) : '')`:
    // JSON has no `Blob`, so a missing name is `''`.
    let given = source.get("name").filter(|n| !n.is_null() && !epdf_kernel::is_undefined(n));
    let Some(given) = given.filter(|n| truthy(Some(n))) else {
        return Err(plugin_error(
            code::INVALID_INPUT,
            CAPABILITY,
            "attachment.add: a file needs a name: pass { data, name } or a File",
            None,
            None,
        ));
    };
    // `freeName` reads a name as a string; one of another type is never
    // taken, so it is kept as given.
    let name = match given {
        Value::String(given) => {
            let name = free_name(given, taken);
            taken.insert(name.clone());
            Value::String(name)
        }
        other => other.clone(),
    };
    let mut file = Map::new();
    file.insert("name".into(), name);
    // `source.mimeType ?? (blob?.type || undefined)`
    if let Some(mime_type) =
        source.get("mimeType").filter(|m| !m.is_null() && !epdf_kernel::is_undefined(m))
    {
        file.insert("mimeType".into(), mime_type.clone());
    }
    if let Some(description) = source.get("description").filter(|d| !epdf_kernel::is_undefined(d)) {
        file.insert("description".into(), description.clone());
    }
    Ok(json!({ "type": "attachments.create", "file": file, "resources": { "file": data } }))
}

/// `content(bytes)`: the file's bytes, ready to save, its name cleaned.
fn content_of(file: &Value, bytes: Value) -> Result<AttachmentContent, Error> {
    let name = match file.get("name") {
        Some(Value::String(name)) => name,
        None => return Err(type_error("replace")),
        Some(v) if epdf_kernel::is_undefined(v) => return Err(type_error("replace")),
        Some(Value::Null) => {
            return Err(Error::named(
                "TypeError",
                "Cannot read properties of null (reading 'replace')",
            ));
        }
        Some(_) => {
            return Err(Error::named("TypeError", "file.name.replace is not a function"));
        }
    };
    Ok(AttachmentContent {
        bytes,
        name: display_name_of(name),
        mime_type: member(file, "mimeType"),
    })
}

impl AttachmentCapability {
    /// Every file the document carries: its attachments, in the order the
    /// document keeps them (by name), then the files of its file attachment
    /// annotations, in the annotation plugin's order. This session's pending
    /// adds and removes show. Reference-stable until it changes. The
    /// attachments are missing until the first read lands; the pages' files
    /// come with the annotation plugin.
    pub fn list(&self) -> Rc<Vec<AttachedFile>> {
        self.0.list()
    }

    /// One file by its `id`, or `None`.
    pub fn get(&self, id: &str) -> Option<AttachedFile> {
        self.0
            .list()
            .iter()
            .find(|file| file.get("id").and_then(Value::as_str) == Some(id))
            .cloned()
    }

    /// The selected files, in the order they were selected; files the list
    /// no longer has drop out. Reference-stable until the list or the
    /// selection changes.
    pub fn get_selected(&self) -> Rc<Vec<AttachedFile>> {
        let list = self.0.list();
        let ids = self.0.cx.state().selected.clone();
        let mut memos = self.0.memos.borrow_mut();
        if let Some((held_list, held_ids, chosen)) = &memos.selected
            && Rc::ptr_eq(held_list, &list)
            && Rc::ptr_eq(held_ids, &ids)
        {
            return chosen.clone();
        }
        // `new Map(files.map((file) => [file.id, file]))`: a later file with
        // the same id wins.
        let chosen: Vec<Value> = ids
            .iter()
            .filter_map(|id| list.iter().rev().find(|file| strict_equals(&member(file, "id"), id)))
            .cloned()
            .collect();
        let chosen = Rc::new(chosen);
        memos.selected = Some((list, ids, chosen.clone()));
        chosen
    }

    /// Make these files the selection; an empty list clears it.
    pub fn select(&self, files: &[AttachedFile]) {
        let ids: Vec<Value> = files.iter().map(|file| member(file, "id")).collect();
        self.0.cx.update_state(|state| selected(state, &ids));
    }

    /// Whether a change to it waits for the engine: an add of an
    /// attachment, or a write to its annotation.
    pub fn is_pending(&self, file: &AttachedFile) -> bool {
        self.0.is_pending(file)
    }

    /// Load state of the attachments: `idle` until `ensure_loaded()` asks,
    /// then `loading` → `ready`, or `forbidden` / `error`.
    pub fn get_status(&self) -> ResourceStatus {
        self.0.files.get_status()
    }

    /// Read the attachments unless they are read or being read: nothing
    /// reads them when a document opens. Resolves once they landed and
    /// `on_resynced` fired. Rejects `operation-cancelled` or
    /// `instance-closed`; after a failure it reads again.
    pub fn ensure_loaded(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.files.ensure_loaded())
    }

    /// Read them again. Rarely needed: the event stream keeps them current.
    pub fn refresh(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.files.refresh())
    }

    /// Whether this session may take files out (`doc.download`):
    /// [`download`](Self::download).
    pub fn can_download(&self) -> bool {
        self.0.cx.allows(DOWNLOAD)
    }

    /// Whether this session may add and remove attachments
    /// (`doc.attachments.modify`).
    pub fn can_add(&self) -> bool {
        self.0.cx.allows(MODIFY)
    }

    /// Whether this session may remove this file: [`can_add`](Self::can_add)
    /// for an attachment, the annotation plugin's `can_delete` for a page's
    /// file (`false` without the annotation plugin).
    pub fn can_remove(&self, file: &AttachedFile) -> bool {
        if is_page_file(file) {
            return self.0.annotations().is_some_and(|a| a.can_delete(&home_ref(file)));
        }
        self.0.cx.allows(MODIFY)
    }

    /// Attach files to the document, as one change: one step of the
    /// history. A source's bytes need a `name`. A name the document has
    /// already gets a number, `notes (2).txt`, so nothing is replaced. The
    /// files show at once, pending, and resolve with the engine's records,
    /// in order. Fires `on_changed`. Rejects `permission-denied`,
    /// `invalid-input` for bytes without a name, `operation-cancelled` or
    /// `instance-closed`.
    pub fn add(
        &self,
        sources: Vec<AttachmentFileSource>,
        options: OperationOptions,
    ) -> Promise<Vec<AttachedFile>> {
        let this = self.0.clone();
        Promise::spawn(async move {
            let signal = options.signal;
            this.refuse(MODIFY, "attachment.add", signal.as_ref())?;
            if sources.is_empty() {
                return Ok(Vec::new());
            }
            // The names in the document decide the free ones, so they are
            // read first.
            cancellable(CAPABILITY, signal.clone(), this.files.ensure_loaded()).await?;
            let view = this.files.view();
            let mut taken: HashSet<String> = view
                .as_array()
                .map(|files| {
                    files.iter().filter_map(|f| key_of(f).as_str().map(str::to_string)).collect()
                })
                .unwrap_or_default();
            let ops = sources
                .iter()
                .map(|source| create_op(source, &mut taken))
                .collect::<Result<Vec<_>, _>>()?;
            let label = json!({ "key": "attachment.add", "count": ops.len() });
            let change = this.stage(label, ops, &this.files.view())?;
            let result = rt::returned(this.clone().answer(change, signal)).await?;
            let items = result
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| type_error("flatMap"))?;
            Ok(items
                .iter()
                .filter(|item| {
                    item.get("type").and_then(Value::as_str) == Some("attachments.create")
                })
                .map(|item| document_file_of(&member(item, "attachment")))
                .collect())
        })
    }

    /// Remove a file, as one step of the history: an attachment, or a
    /// page's file attachment annotation (through the annotation plugin).
    /// It goes at once; an undo brings it back. Rejects `permission-denied`,
    /// `not-found` for a file the document doesn't have,
    /// `operation-cancelled` or `instance-closed`.
    pub fn remove(&self, file: AttachedFile, options: OperationOptions) -> Promise<()> {
        let this = self.0.clone();
        Promise::spawn(async move {
            if is_page_file(&file) {
                // `return annotation.delete(ref, options)`: the async
                // function's promise adopts the annotation plugin's, a turn
                // for the resolve-thenable job, then its reaction.
                let deleting = this
                    .annotations_for("attachment.remove")?
                    .delete(&home_ref(&file), options.signal);
                rt::tick().await;
                return deleting.reaction().await;
            }
            this.refuse(MODIFY, "attachment.remove", options.signal.as_ref())?;
            let attachment_ref = home_ref(&file);
            let key = member(&attachment_ref, "key");
            let view = this.files.view();
            if let Some(files) = view.as_array()
                && !files.iter().any(|f| strict_equals(&key_of(f), &key))
            {
                return Err(plugin_error(
                    code::NOT_FOUND,
                    CAPABILITY,
                    format!(
                        "attachment.remove: the document has no attachment '{}'",
                        js_string(&key)
                    ),
                    None,
                    None,
                ));
            }
            let ops = vec![json!({ "type": "attachments.delete", "ref": attachment_ref })];
            let change = this.stage(json!({ "key": "attachment.remove" }), ops, &view)?;
            rt::returned(this.clone().answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// The file's bytes, ready to save: its name cleaned
    /// ([`display_name_of`]) and its type. A pending add is waited for.
    /// Rejects `permission-denied` without `doc.download`, `not-found`,
    /// `invalid-input` for a file that decodes past the engine's limit,
    /// `operation-cancelled` or `instance-closed`.
    pub fn download(
        &self,
        file: AttachedFile,
        options: OperationOptions,
    ) -> Promise<AttachmentContent> {
        let this = self.0.clone();
        Promise::spawn(async move {
            let signal = options.signal;
            if is_page_file(&file) {
                let annotation = this.annotations_for("attachment.download")?;
                let bytes = annotation.download_resource(&home_ref(&file), "file", signal).await?;
                return content_of(&file, bytes);
            }
            this.refuse(DOWNLOAD, "attachment.download", signal.as_ref())?;
            // A file this session is still adding is read once the engine
            // has it.
            if this.is_pending(&file) {
                let settled = this.cx.changes()?.when_settled();
                cancellable(CAPABILITY, signal.clone(), settled).await?;
            }
            let read = async {
                let doc = this.cx.doc()?;
                let call = doc.call("attachments.download", None, vec![home_ref(&file)]);
                let bytes = cancellable(CAPABILITY, signal, call).await?;
                content_of(&file, bytes)
            };
            read.await.map_err(|error| to_plugin_error(CAPABILITY, &error))
        })
    }

    /// Go to a page's file: select its annotation and bring it into view,
    /// a third down. Does nothing for an attachment, or without the
    /// annotation plugin; without the stage it only selects.
    pub fn reveal(&self, file: &AttachedFile) {
        if !is_page_file(file) {
            return;
        }
        let Some(annotation) = self.0.annotations() else { return };
        let Some(record) = annotation.get(&home_ref(file)) else { return };
        let record_ref = member(&record, "ref");
        annotation.selection_set(std::slice::from_ref(&record_ref));
        let Some(stage) = self.0.cx.try_get(&STAGE).ok().flatten() else { return };
        // A record always names its page. A page ref that does not decode
        // (another kind) reveals nothing, where the TypeScript hands it to
        // the stage as it is; registered in the README.
        let Some(page) = record_ref.get("page").and_then(PageRef::from_json) else { return };
        let rect = record.get("rect").and_then(|r| serde_json::from_value::<Rect>(r.clone()).ok());
        stage.reveal(
            PageTarget::Ref(page),
            Some(RevealOptions {
                rect,
                anchor: Some(RevealAnchor {
                    x: Some(RevealAnchorValue::Word(RevealWord::Center)),
                    y: Some(RevealAnchorValue::Fraction(0.35)),
                }),
                behavior: Some(ScrollBehaviorKind::Smooth),
                zoom: None,
            }),
        );
    }

    /// A confirmed change of the attachments, whoever made it.
    pub fn on_changed(
        &self,
        listener: impl Fn(&AttachmentsChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.changed.on(listener)
    }

    /// The attachments were read, first or again. Fires no per-file
    /// `on_changed`.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&AttachmentsResyncedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.resynced.on(listener)
    }
}

/// `createAttachmentController(ctx)`: the api; there is no `connect`, since
/// nothing is read until someone asks.
pub fn create_attachment_controller(
    cx: Controller<AttachmentState, Value>,
) -> Result<(AttachmentCapability, Option<ConnectFn>), Error> {
    let changed = cx.event_source::<AttachmentsChangedEvent>();
    let resynced = cx.event_source::<AttachmentsResyncedEvent>();

    // `load: async (doc) => ({ value: (await doc.attachments.list()).attachments })`
    let load: Load = Rc::new(|doc, _signal| {
        let call = doc.map(|doc| doc.call("attachments.list", None, Vec::new()));
        Promise::spawn(async move {
            let Some(call) = call else {
                return Err(Error::new("[attachment] no document"));
            };
            let list = call.await?;
            Ok(Loaded { value: member(&list, "attachments"), cursor: None })
        })
    });
    let mut spec = MirrorSpec::new(
        "attachments",
        Value::Null,
        load,
        Rc::new(fold_files) as Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>>,
    );
    spec.on_demand = true;
    spec.predict = Some(Rc::new(predict_files));

    // The `changed` hook emits through the core's event sources, so it
    // reaches the core through a weak slot filled once the core exists.
    let slot: Rc<RefCell<Weak<Core>>> = Rc::new(RefCell::new(Weak::new()));
    let hook_slot = slot.clone();
    spec.changed = Some(Rc::new(move |change: MirrorChange| {
        let Some(core) = hook_slot.borrow().upgrade() else { return Ok(()) };
        if !truthy(Some(&change.next)) {
            return Ok(());
        }
        if change.cause == "load" {
            let files = change
                .next
                .as_array()
                .map(|a| a.iter().map(document_file_of).collect())
                .unwrap_or_default();
            core.resynced.emit(&AttachmentsResyncedEvent { files });
            return Ok(());
        }
        let Some(event) = change.event.as_ref() else { return Ok(()) };
        let (Some(origin), Some((kind, files))) =
            (event.get("origin"), fact_of(event, &change.previous))
        else {
            return Ok(());
        };
        core.changed.emit(&AttachmentsChangedEvent { kind, files, origin: origin.clone() });
        Ok(())
    }));
    let files = cx.mirror(spec)?;

    let core =
        Rc::new(Core { cx, files, changed, resynced, memos: RefCell::new(Memos::default()) });
    *slot.borrow_mut() = Rc::downgrade(&core);
    Ok((AttachmentCapability(core), None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_not_a_string_throws_as_replace_does() {
        let message = |file: Value| content_of(&file, Value::Null).unwrap_err().message;
        assert_eq!(message(json!({})), "Cannot read properties of undefined (reading 'replace')");
        assert_eq!(
            message(json!({ "name": null })),
            "Cannot read properties of null (reading 'replace')"
        );
        assert_eq!(message(json!({ "name": 5 })), "file.name.replace is not a function");
    }
}
