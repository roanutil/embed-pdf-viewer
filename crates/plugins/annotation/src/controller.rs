//! `controller.ts`: the composition root. It builds the plugin's services
//! once and wires each area with them. In Rust the services and every
//! area's own data are fields of one [`Core`], and each area is an
//! `impl Core` block in the module named after its TypeScript file
//! (`read/`, `write/`, `sync/`, `comments/`, `tools/`, `services/`), so every
//! verb still has one home. [`AnnotationCapability`] is the public lens and
//! the host lens in one type, as the TypeScript's two lenses are one object.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use epdf_annotation_core::types::{Model, Session};
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error};
use serde_json::Value;

use crate::comments::threads::ThreadsState;
use crate::contract::ParsedSettings;
use crate::host_contract::{Behavior, FilePickerProvider};
use crate::model::AnnotationState;
use crate::read::annotations::ReadsState;
use crate::read::chrome::ChromeState;
use crate::read::properties::PropertiesState;
use crate::read::render::RenderState;
use crate::read::view::{View, ViewInputs};
use crate::services::events::Events;
use crate::services::lending::Lending;
use crate::services::store::StoreState;
use crate::tools::ghost::GhostState;
use crate::tools::hover_feed::HoverFeed;
use crate::tools::registry::ToolsState;
use crate::write::drafts::DraftsState;
use crate::write::measurement::MeasurementState;
use crate::write::stamps::StampsState;
use crate::write::text_editing::TextState;

/// The capability name errors carry.
pub const CAPABILITY: &str = "annotation";

/// The plugin's services and every area's data.
pub struct Core {
    pub(crate) me: Weak<Core>,
    pub(crate) cx: Controller<AnnotationState, Value>,
    pub(crate) settings: InstanceSettings,
    parsed: RefCell<Option<ParsedSettings>>,
    pub(crate) events: Events,
    /// The records: what the engine confirmed, with this session's pending changes on top.
    pub(crate) records: epdf_kernel::mirror::Mirror,
    pub(crate) view: View,
    pub(crate) lending: Rc<Lending>,
    pub(crate) store: StoreState,
    pub(crate) tools: ToolsState,
    pub(crate) behaviors: RefCell<Vec<Rc<dyn Behavior>>>,
    pub(crate) file_picker: RefCell<Option<FilePickerProvider>>,
    pub(crate) reads: ReadsState,
    pub(crate) chrome: ChromeState,
    pub(crate) properties: PropertiesState,
    pub(crate) render: RenderState,
    pub(crate) text: TextState,
    pub(crate) stamps: StampsState,
    pub(crate) ghost: GhostState,
    pub(crate) measurement: MeasurementState,
    pub(crate) drafts: DraftsState,
    pub(crate) threads: ThreadsState,
    pub(crate) hover_feed: RefCell<Option<HoverFeed>>,
}

/// `AnnotationHostCapability`, which includes the public
/// `AnnotationCapability`. Cheap to clone.
#[derive(Clone)]
pub struct AnnotationCapability(pub(crate) Rc<Core>);

impl std::ops::Deref for AnnotationCapability {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.0
    }
}

impl Core {
    pub(crate) fn rc(&self) -> Rc<Core> {
        self.me.upgrade().expect("the annotation core is alive while it runs")
    }

    pub(crate) fn weak(&self) -> Weak<Core> {
        self.me.clone()
    }

    pub fn capability(&self) -> AnnotationCapability {
        AnnotationCapability(self.rc())
    }

    /// The plugin's state.
    pub(crate) fn state(&self) -> Rc<AnnotationState> {
        self.cx.state()
    }

    /// `ctx.state.update(transition)`.
    pub(crate) fn update_state(
        &self,
        transition: impl FnOnce(&Rc<AnnotationState>) -> Rc<AnnotationState>,
    ) {
        self.cx.update_state(|state| transition(&state));
    }

    /// The settings, parsed once per settings value.
    pub(crate) fn parsed(&self) -> ParsedSettings {
        let value = self.settings.get();
        if let Some(parsed) = &*self.parsed.borrow()
            && Rc::ptr_eq(&parsed.value, &value)
        {
            return parsed.clone();
        }
        let parsed = ParsedSettings::parse(value);
        *self.parsed.borrow_mut() = Some(parsed.clone());
        parsed
    }

    /// `ctx.doc`, or `None` while there is none.
    pub(crate) fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.cx.doc().ok()
    }

    /// `ctx.doc`, or the kernel's error reading it.
    pub(crate) fn doc_or_throw(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.cx.doc()
    }

    /// `ctx.doc?.security`.
    pub(crate) fn security(&self) -> Option<Rc<dyn DocumentSecurity>> {
        self.doc().map(|doc| doc.security())
    }

    /// `ctx.allows(permission)`: `annotations:create` asks about the user's
    /// own identity, as the kernel's `sessionAllows` does.
    pub(crate) fn allows(&self, permission: &str) -> bool {
        match self.security() {
            None => false,
            Some(security) if permission == "annotations:create" => {
                security.allows_annotation("create", &serde_json::json!({}))
            }
            Some(security) => security.allows(permission),
        }
    }

    /// `ctx.assertAllowed(permission, operation)`.
    pub(crate) fn assert_allowed(&self, permission: &str, operation: &str) -> Result<(), Error> {
        if self.doc().is_none() {
            return Err(self.cx.doc().err().unwrap_or_else(|| Error::new("no document")));
        }
        if self.allows(permission) {
            return Ok(());
        }
        Err(plugin_error(
            code::PERMISSION_DENIED,
            CAPABILITY,
            format!("{operation} requires {permission}"),
            None,
            None,
        )
        .with("permission", permission))
    }

    /// The instance lifetime: aborted when the document closes.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.cx.lifetime()
    }

    /// A commit a `void` verb makes, whose failure the TypeScript would throw
    /// to the caller: logged to the console with the plugin's prefix rather
    /// than dropped. Every message these verbs build is well formed, so this
    /// is reached only by a failure in the store's later stages.
    pub(crate) fn commit_or_log(&self, message: Value) {
        if let Err(error) = self.commit_json(message) {
            crate::console::error("[annotation] a change failed:", error.to_canonical());
        }
    }

    /// `ctx.cancellable(signal, task)`.
    pub(crate) fn cancellable<T: Clone + 'static>(
        &self,
        signal: Option<AbortSignal>,
        task: Promise<T>,
    ) -> Promise<T> {
        cancellable(CAPABILITY, signal, task)
    }

    /// The document's page list (`ctx.document()?.pages`).
    pub(crate) fn pages(&self) -> Rc<Value> {
        self.cx
            .document()
            .map(|meta| meta.pages)
            .unwrap_or_else(|| Rc::new(Value::Array(Vec::new())))
    }

    /// `ctx.getPage(page)`: the page a ref or an index names, or `None`.
    pub(crate) fn get_page(&self, page: &Value) -> Option<Value> {
        let pages = self.pages();
        let list = pages.as_array()?;
        if let Some(index) = page.as_f64() {
            if index >= 0.0 && index.fract() == 0.0 {
                return list.get(index as usize).cloned();
            }
            return None;
        }
        let wanted = crate::js::page_object_number(page);
        list.iter()
            .find(|info| {
                info.get("ref").map(crate::js::page_object_number) == Some(wanted)
                    && wanted.is_finite()
            })
            .cloned()
    }

    /// `ctx.pageOf(page)`: the page, or `not-found`.
    pub(crate) fn page_of(&self, page: &Value) -> Result<Value, Error> {
        if let Some(found) = self.get_page(page) {
            return Ok(found);
        }
        let message = match page.as_f64() {
            Some(index) => {
                format!(
                    "there is no page {} in this document",
                    epdf_annotation_core::js::number_to_string(index)
                )
            }
            None => format!(
                "page {} is not in this document",
                crate::js::stringify(page.get("objectNumber")).trim_matches('"')
            ),
        };
        Err(plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None))
    }

    /// The page's registry ref (`pageOf(page).ref`).
    pub(crate) fn page_ref_of(&self, page: &Value) -> Result<Value, Error> {
        Ok(self.page_of(page)?.get("ref").cloned().unwrap_or(Value::Null))
    }

    /// The fonts the engine knows: the local engine's list, none on the
    /// cloud engine. The plugin context has no local engine's font list, so
    /// it is none here.
    pub(crate) fn fonts(&self) -> Option<&[crate::rich_text::FontHandle]> {
        None
    }

    /// The view's inputs now.
    fn view_inputs(&self) -> ViewInputs {
        let document = self.cx.document().map(|meta| (meta.pages, meta.revision));
        ViewInputs {
            truths: self.records.get(),
            shown: self.records.view(),
            vector: self.state().vector.clone(),
            document,
            lent: self.lending.records(),
        }
    }

    /// `view.view()`: the records as shown.
    pub(crate) fn annotation_view(&self) -> crate::read::view::AnnotationView {
        let lender = self.lending.lender();
        let security = self.security();
        let drawing = lender.as_deref().map(crate::host_contract::LenderDrawing);
        self.view.view(
            self.view_inputs(),
            security.as_ref(),
            drawing.as_ref().map(|d| d as &dyn crate::read::view::LentDrawing),
        )
    }

    /// `store.model()`: the composed model every read and gesture works on.
    pub fn model(&self) -> Rc<Model> {
        let view = self.annotation_view();
        let session: Session = self.state().session.clone();
        self.view.model(&session, &view)
    }

    /// `view.pageModel(pageObjectNumber)`.
    pub(crate) fn page_model(&self, page: f64) -> Rc<Model> {
        let whole = self.model();
        self.view.page_model(&whole, page)
    }
}

/// `createAnnotationController(ctx)`.
pub fn create_annotation_controller(
    cx: Controller<AnnotationState, Value>,
) -> Result<(AnnotationCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    let events = Events::new(&cx);
    let records = crate::sync::records::create_records_mirror(&cx, &events)?;
    let core = Rc::new_cyclic(|me| Core {
        me: me.clone(),
        cx: cx.clone(),
        settings,
        parsed: RefCell::new(None),
        events,
        records,
        view: View::default(),
        lending: Rc::new(Lending::default()),
        store: StoreState::new(&cx),
        tools: ToolsState::default(),
        behaviors: RefCell::new(Vec::new()),
        file_picker: RefCell::new(None),
        reads: ReadsState::default(),
        chrome: ChromeState::default(),
        properties: PropertiesState::default(),
        render: RenderState::default(),
        text: TextState::default(),
        stamps: StampsState::default(),
        ghost: GhostState::default(),
        measurement: MeasurementState::default(),
        drafts: DraftsState::default(),
        threads: ThreadsState::default(),
        hover_feed: RefCell::new(None),
    });
    // `createServices`: the store's listeners, then the tools a document
    // has (read once, when it opens: each seeds its defaults).
    core.wire_store()?;
    core.create_tool_registry()?;
    // The session snaps, and lets people change by hand, as the settings say.
    let parsed = core.parsed();
    core.update_state(|s| crate::model::with_snap(s, &parsed.snap));
    core.update_state(|s| crate::model::with_editable(s, &parsed.editable));
    core.wire_settings();
    core.follow_confirmed_changes();
    core.wire_editable_freeze();
    core.wire_measurement();
    core.create_hover_feed();
    let capability = AnnotationCapability(core.clone());
    let weak = Rc::downgrade(&core);
    let connect: ConnectFn = Rc::new(move || match weak.upgrade() {
        Some(core) => core.connect(),
        None => Ok(()),
    });
    Ok((capability, Some(connect)))
}

impl Core {
    /// The settings' own listener: snapping and the editable rule follow.
    fn wire_settings(&self) {
        let me = self.weak();
        let off = self.settings.events.on(move |change| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            let parsed = core.parsed();
            if change.changed.iter().any(|k| k == "snap") {
                core.update_state(|s| crate::model::with_snap(s, &parsed.snap));
            }
            if change.changed.iter().any(|k| k == "editable") {
                core.update_state(|s| crate::model::with_editable(s, &parsed.editable));
            }
            Ok(())
        });
        self.cx.listen(off);
    }

    /// A text box, or a gesture, on an annotation the `editable` rule now freezes ends.
    fn wire_editable_freeze(&self) {
        let me = self.weak();
        let off = self.settings.events.on(move |change| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            if !change.changed.iter().any(|k| k == "editable") {
                return Ok(());
            }
            let model = core.model();
            let editing = model.editing.as_ref().and_then(|id| model.by_id.get(id));
            if let Some(record) = editing
                && !epdf_annotation_core::editable::is_contents_editable_by_hand(
                    &model.editable,
                    record,
                )
            {
                let _ = core.text_end(None);
            }
            let ids: Vec<String> =
                model.draft().map(|d| d.ids().into_iter().cloned().collect()).unwrap_or_default();
            if ids.iter().any(|id| {
                model.by_id.get(id).is_some_and(|r| {
                    !epdf_annotation_core::editable::is_transformable_by_hand(&model.editable, r)
                })
            }) {
                core.commit_json(serde_json::json!({ "type": "cancel" }))?;
            }
            Ok(())
        });
        self.cx.listen(off);
    }
}
