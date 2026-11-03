//! The cursor and the controls currently able to receive it.
use crate::{
    controller::Core,
    model::{field_by_ref, field_key_of, shared_field_by_key, widget_row_of},
    read::fields::widget_object_of,
    value::number_key,
};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_stage::{PageTarget, RevealOptions, STAGE};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub type FocusTarget = Rc<dyn Fn(bool)>;
struct Pending {
    widget: String,
    select: bool,
    expire: Unsubscribe,
}
#[derive(Default)]
pub(crate) struct Cursor {
    targets: RefCell<HashMap<String, FocusTarget>>,
    pending: RefCell<Option<Pending>>,
    leaving: RefCell<Option<Unsubscribe>>,
    last: Cell<Option<f64>>,
}
impl Core {
    pub(crate) fn wire_cursor(&self) -> Result<(), Error> {
        let weak = self.weak();
        self.cx.on_state_change(move |next, previous| {
            if next.get("focused") == previous.get("focused") {
                return Ok(());
            }
            if let Some(core) = weak.upgrade() {
                let index = core.view();
                let focused = next.get("focused");
                let field = focused
                    .and_then(|f| f.get("field"))
                    .and_then(Value::as_str)
                    .and_then(|key| shared_field_by_key(&index, key))
                    .map(|f| (**f).clone());
                let row = focused
                    .and_then(|f| f.get("widget"))
                    .and_then(Value::as_f64)
                    .and_then(|n| widget_row_of(&index, n));
                core.events
                    .focus_changed
                    .emit(&json!({"field":field,"widget":row.and_then(|r|r.get("ref"))}));
            }
            Ok(())
        });
        let weak = self.weak();
        self.cx.cleanup_fn(move || {
            if let Some(core) = weak.upgrade() {
                core.clear_pending_focus();
                core.cursor.targets.borrow_mut().clear();
                let cancel = core.cursor.leaving.borrow_mut().take();
                if let Some(cancel) = cancel {
                    cancel.call();
                }
            }
        });
        Ok(())
    }
    pub(crate) fn clear_pending_focus(&self) {
        let pending = self.cursor.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending.expire.call();
        }
    }
    pub(crate) fn focused_widget(&self) -> Option<Value> {
        self.cx.state().get("focused").filter(|f| !f.is_null()).cloned()
    }
    pub(crate) fn last_focused_widget(&self) -> Option<f64> {
        self.cursor.last.get()
    }
    fn set_focused_widget(&self, next: Option<Value>) {
        self.cx.update_state(|state| crate::model::set_focused_widget(state, next));
    }
    pub(crate) fn note_widget_focus(&self, event: &str, field_ref: &Value, widget_ref: &Value) {
        let widget = widget_object_of(widget_ref);
        if event == "focus" {
            let leaving = self.cursor.leaving.borrow_mut().take();
            if let Some(leaving) = leaving {
                leaving.call();
            }
            let pending_elsewhere = self
                .cursor
                .pending
                .borrow()
                .as_ref()
                .is_some_and(|p| p.widget != number_key(widget));
            if pending_elsewhere {
                self.clear_pending_focus();
            }
            let index = self.view();
            let field = field_by_ref(&index, field_ref);
            self.cursor.last.set(Some(widget));
            self.set_focused_widget(
                field.map(|f| json!({"field":field_key_of(f),"widget":widget})),
            );
        } else if self.focused_widget().and_then(|f| f["widget"].as_f64()) == Some(widget) {
            let leaving = self.cursor.leaving.borrow_mut().take();
            if let Some(leaving) = leaving {
                leaving.call();
            }
            let weak = self.weak();
            let cancel = self.cx.clock().after(0.0, move || {
                if let Some(core) = weak.upgrade() {
                    core.cursor.leaving.borrow_mut().take();
                    core.set_focused_widget(None);
                }
            });
            *self.cursor.leaving.borrow_mut() = Some(cancel);
        }
    }
    pub(crate) fn focus_row(&self, row: &Value, select: bool) -> bool {
        let stage = self.cx.try_get(&STAGE).ok().flatten();
        if let Some(stage) = &stage
            && let (Some(page), Some(rect)) =
                (row["page"]["objectNumber"].as_u64(), row.get("rect"))
        {
            let rect = epdf_kernel::engine::decode(rect.clone()).ok();
            stage.reveal(
                PageTarget::Ref(epdf_kernel::geometry::PageRef::new(page as u32)),
                Some(RevealOptions { rect, ..Default::default() }),
            );
        }
        self.clear_pending_focus();
        let widget = number_key(widget_object_of(&row["ref"]));
        let target = self.cursor.targets.borrow().get(&widget).cloned();
        if let Some(target) = target {
            target(select);
            return true;
        }
        if stage.is_none() {
            return false;
        }
        let weak = self.weak();
        let pending_widget = widget.clone();
        let expire = self.cx.clock().after(2000.0, move || {
            if let Some(core) = weak.upgrade() {
                let matches = core
                    .cursor
                    .pending
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.widget == pending_widget);
                if matches {
                    core.cursor.pending.borrow_mut().take();
                }
            }
        });
        *self.cursor.pending.borrow_mut() = Some(Pending { widget, select, expire });
        true
    }
    /// Lend a control while its widget is mounted. Replacement releases are identity-safe.
    pub fn attach_focus_target(&self, widget: &Value, target: FocusTarget) -> Unsubscribe {
        let widget = number_key(widget_object_of(widget));
        self.cursor.targets.borrow_mut().insert(widget.clone(), target.clone());
        let select =
            self.cursor.pending.borrow().as_ref().filter(|p| p.widget == widget).map(|p| p.select);
        if let Some(select) = select {
            self.clear_pending_focus();
            target(select);
        }
        let weak = self.weak();
        Unsubscribe::new(move || {
            if let Some(core) = weak.upgrade() {
                let current = core
                    .cursor
                    .targets
                    .borrow()
                    .get(&widget)
                    .is_some_and(|t| Rc::ptr_eq(t, &target));
                if current {
                    core.cursor.targets.borrow_mut().remove(&widget);
                }
            }
        })
    }
}
