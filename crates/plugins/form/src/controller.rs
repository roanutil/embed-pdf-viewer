//! `controller.ts`: the composition root. In Rust the services and every
//! area's own data are fields of one [`Core`], and each area is an
//! `impl Core` block in the module named after its TypeScript file
//! (`read/`, `write/`, `sync/`, `services/`, `tools/`), so every verb still
//! has one home. [`FormCapability`] is the public lens and the host lens in
//! one type, as the TypeScript's two lenses are one object.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::mirror::Mirror;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error};
use indexmap::IndexMap;
use serde_json::Value;

use crate::model::FieldIndex;
use crate::services::events::Events;
use crate::services::formatted::FormattedTexts;
use crate::services::index_cache::IndexCache;
use crate::services::object_numbers::ObjectNumbers;
use crate::services::scripting::ScriptingSeam;
use epdf_kernel::serial_queue::SerialQueue;

/// The capability name errors carry.
pub const CAPABILITY: &str = "form";

/// The plugin's services and every area's data.
pub struct Core {
    pub(crate) me: Weak<Core>,
    pub(crate) cx: Controller<Value, Value>,
    pub(crate) settings: InstanceSettings,
    pub(crate) events: Events,
    /// The form: what the engine confirmed, with this session's pending
    /// changes on top (`view`).
    pub(crate) fields: Mirror,
    /// The mirror's values as indexes, decoded once each.
    pub(crate) indexes: Rc<IndexCache>,
    pub(crate) formatted: Rc<FormattedTexts>,
    pub(crate) numbers: ObjectNumbers,
    /// Fills take turns on one serial queue until they are sent, and a
    /// repair until the engine answers.
    pub(crate) enqueue: SerialQueue,
    pub(crate) scripting: ScriptingSeam,
    /// A recalculation after a refused fill is on its way.
    pub(crate) recalculating: Cell<bool>,
    /// The text typed in each field and not written yet, by field key.
    pub(crate) drafts: RefCell<IndexMap<String, (Value, String)>>,
    /// `rights.key()`'s group list and the view it was read from.
    pub(crate) rights_groups: RefCell<Option<(Rc<FieldIndex>, Rc<Vec<Value>>)>>,
    /// The field reads' identity memos.
    pub(crate) reads: crate::read::fields::ReadMemos,
    /// `listWidgets`' memo, by page: the items it made, and from what.
    pub(crate) widget_items: RefCell<HashMap<String, crate::read::widgets::WidgetItems>>,
    /// The widget hover pump, made on first use.
    pub(crate) widget_pump: RefCell<Option<epdf_plugin_actions::HoverPump>>,
}

/// `FormHostCapability`, which includes the public `FormCapability`.
/// Cheap to clone.
#[derive(Clone)]
pub struct FormCapability(pub(crate) Rc<Core>);

impl std::ops::Deref for FormCapability {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.0
    }
}

impl Core {
    pub(crate) fn rc(&self) -> Rc<Core> {
        self.me.upgrade().expect("the form core is alive while it runs")
    }

    pub(crate) fn weak(&self) -> Weak<Core> {
        self.me.clone()
    }

    pub fn capability(&self) -> FormCapability {
        FormCapability(self.rc())
    }

    /// `fields.view()`: the form as this session shows it.
    pub fn view(&self) -> Rc<FieldIndex> {
        self.indexes.index(&self.fields.view())
    }

    /// `fields.get()`: the form as the engine confirmed it.
    pub fn truth(&self) -> Rc<FieldIndex> {
        self.indexes.index(&self.fields.get())
    }

    /// The instance lifetime: aborted when the document closes.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &AbortSignal {
        self.cx.lifetime()
    }

    /// `ctx.doc`, or `None` while there is none.
    pub(crate) fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.cx.doc().ok()
    }

    /// `ctx.doc`, or the kernel's error reading it.
    pub(crate) fn doc_or_throw(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.cx.doc()
    }

    /// `ctx.allows(permission)`.
    pub(crate) fn allows(&self, permission: &str) -> bool {
        self.cx.allows(permission)
    }

    /// `ctx.assertAllowed(permission, operation)`.
    pub(crate) fn assert_allowed(&self, permission: &str, operation: &str) -> Result<(), Error> {
        self.cx.assert_allowed(permission, operation)
    }

    /// `ctx.cancellable(signal, task)`.
    pub(crate) fn cancellable<T: Clone + 'static>(
        &self,
        signal: Option<AbortSignal>,
        task: Promise<T>,
    ) -> Promise<T> {
        cancellable(CAPABILITY, signal, task)
    }

    /// The settings now.
    pub(crate) fn settings_now(&self) -> Rc<Value> {
        self.settings.get()
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
        let wanted = page.get("objectNumber").and_then(Value::as_f64)?;
        list.iter()
            .find(|info| {
                info.get("ref").and_then(|r| r.get("objectNumber")).and_then(Value::as_f64)
                    == Some(wanted)
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
                    epdf_geometry::js::number_to_string(index)
                )
            }
            None => format!(
                "page {} is not in this document",
                crate::value::string_of(page.get("objectNumber"))
            ),
        };
        Err(plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None))
    }

    /// A `PluginError` of this capability.
    pub(crate) fn error(&self, code: &str, message: impl Into<String>) -> Error {
        plugin_error(code, CAPABILITY, message, None, None)
    }
}

/// `createFormController(ctx)`.
pub fn create_form_controller(
    cx: Controller<Value, Value>,
) -> Result<(FormCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    let events = Events::new(&cx);
    let indexes = Rc::new(IndexCache::default());
    let fields = crate::sync::fields::create_fields_mirror(&cx, &events, &indexes)?;
    let formatted = Rc::new(FormattedTexts::default());
    let queue = cx.serial_queue("writes");
    let core = Rc::new_cyclic(|me| Core {
        me: me.clone(),
        cx: cx.clone(),
        settings,
        events,
        fields,
        indexes,
        formatted,
        numbers: ObjectNumbers::default(),
        enqueue: queue,
        scripting: ScriptingSeam::default(),
        recalculating: Cell::new(false),
        drafts: RefCell::new(IndexMap::new()),
        rights_groups: RefCell::new(None),
        reads: Default::default(),
        widget_items: RefCell::new(HashMap::new()),
        widget_pump: RefCell::new(None),
    });
    core.watch_formatted_texts()?;
    core.wire_fills()?;
    core.wire_typing()?;
    let capability = FormCapability(core.clone());
    let weak = Rc::downgrade(&core);
    let connect: ConnectFn = Rc::new(move || match weak.upgrade() {
        Some(core) => core.connect(),
        None => Ok(()),
    });
    Ok((capability, Some(connect)))
}

// ── the capability's own members ────────────────────────────────────────────

impl Core {
    /// `canRead()`: `doc.forms.read`.
    pub fn can_read(&self) -> bool {
        self.allows("doc.forms.read")
    }

    /// `canFill(ref)`: `false` for a field the form doesn't have.
    pub fn can_fill(&self, reference: &Value) -> bool {
        let index = self.view();
        crate::model::field_by_ref(&index, reference).is_some_and(|field| self.may_fill(field))
    }

    /// `canDesign()`: `doc.forms.modify`.
    pub fn can_design(&self) -> bool {
        self.allows("doc.forms.modify")
    }

    /// `canRestoreAttribution()`: `doc.forms.import`.
    pub fn can_restore_attribution(&self) -> bool {
        self.allows("doc.forms.import")
    }

    /// `canWriteScripts()`: `doc.forms.script`.
    pub fn can_write_scripts(&self) -> bool {
        self.allows("doc.forms.script")
    }

    /// `getSettings()`.
    pub fn get_settings(&self) -> Rc<Value> {
        self.settings.get()
    }

    /// `updateSettings(changes)`: for every document.
    pub fn update_settings(&self, changes: &Value) {
        self.settings.store.update_settings(changes);
    }

    /// `resetSettings()`.
    pub fn reset_settings(&self) {
        self.settings.store.reset_settings();
    }

    /// `onSettingsChanged(listener)`: `{ settings, changed }`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&Value) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.settings.events.on(move |change| {
            listener(
                &serde_json::json!({ "settings": *change.settings, "changed": change.changed }),
            )
        })
    }
}
