//! `sync/viewports.ts`: the pages' viewports. Each page's layout carries
//! them (`PageLayout.viewports`), and the page list keeps them current: a
//! scale change's own prediction, its event from this session or another, a
//! page insert or delete. Where the engine can't keep a scale, the session
//! keeps its own (`localViewports`). `connect` hands the annotation plugin
//! each page's viewports, again whenever a page's viewports change, and for
//! every page on a new `defaultScale` setting.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::contract::{OperationOptions, PageInput, PageScale, no_page_scale};
use crate::controller::{MeasurementCapability, MeasurementContext};
use crate::model::select_page_scale;
use crate::scale::default_measure;
use crate::services::store::{CAPABILITY, PageRow};

/// The inputs a page's public scale was projected from, and the scale:
/// `memoByKey`, so a page's scale is the same `Rc` until its layout, its
/// viewports or the default scale change.
pub(crate) struct ScaleMemo {
    /// The registry the page's layout row came from. A registry is replaced
    /// whole, every row new, so this is the TypeScript's key on the layout
    /// object (whose viewports it carries).
    pages: Option<Rc<Value>>,
    /// The session's own viewports for the page, where the engine keeps none.
    local: Option<Rc<Value>>,
    persistent: bool,
    settings: Rc<Value>,
    scale: Rc<PageScale>,
}

fn same(a: &Option<Rc<Value>>, b: &Option<Rc<Value>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// `fallbackOf(layout)`: the `defaultScale` setting, in the page's user
/// unit.
pub(crate) fn fallback_of(settings: &Value, layout: &PageRow) -> Result<Value, Error> {
    default_measure(settings.get("defaultScale").unwrap_or(&Value::Null), layout.user_unit_or_one())
}

/// `isPersistent(page)`: the engine can persist a scale into the document
/// (`doc.page(page).measure !== undefined`). `Err` where reading `ctx.doc`
/// throws: the registry still names the page after the handle is gone.
pub(crate) fn is_persistent(cx: &MeasurementContext) -> Result<bool, Error> {
    Ok(cx.doc()?.has("page.measure"))
}

/// `viewportsOf(layout)`: a page's viewports, its layout's, or the
/// session's own where the engine keeps none; an empty list when there are
/// none. The second half is the session's list, which the memo keys on.
fn viewports_of(
    cx: &MeasurementContext,
    layout: &PageRow,
    persistent: bool,
) -> (Vec<Value>, Option<Rc<Value>>) {
    if persistent {
        let list = layout.row().get("viewports").and_then(Value::as_array).cloned();
        return (list.unwrap_or_default(), None);
    }
    let local = cx.state().local_viewports.get(&layout.page().page_object_number).cloned();
    (local.as_deref().and_then(Value::as_array).cloned().unwrap_or_default(), local)
}

impl MeasurementCapability {
    /// `scaleOfPage(pageObjectNumber)`: the page's public scale, the same
    /// `Rc` until its layout, viewports or the default change.
    fn scale_of_page(&self, page: PageRef) -> Result<Rc<PageScale>, Error> {
        let core = self.core();
        let pon = page.page_object_number;
        let Some(layout) = core.registry.find(PageInput::Ref(page)) else {
            return Ok(Rc::new(no_page_scale()));
        };
        let pages = core.registry.pages();
        let persistent = is_persistent(&core.cx)?;
        let (viewports, local) = viewports_of(&core.cx, &layout, persistent);
        let settings = core.settings.get();
        if let Some(memo) = core.scales.borrow().get(&pon)
            && same(&memo.pages, &pages)
            && same(&memo.local, &local)
            && memo.persistent == persistent
            && Rc::ptr_eq(&memo.settings, &settings)
        {
            return Ok(memo.scale.clone());
        }
        let fallback = fallback_of(&settings, &layout)?;
        let scale = Rc::new(select_page_scale(&viewports, layout.size(), &fallback, persistent));
        core.scales
            .borrow_mut()
            .insert(pon, ScaleMemo { pages, local, persistent, settings, scale: scale.clone() });
        Ok(scale)
    }

    /// `scaleOf(page)`: a page's scale, by ref or index; a page that isn't
    /// there has none, and isn't ready. `Err` where the TypeScript throws:
    /// a `defaultScale` that cannot be built in the page's user unit.
    pub(crate) fn scale_of(&self, page: PageInput) -> Result<Rc<PageScale>, Error> {
        match page {
            PageInput::Ref(page) => self.scale_of_page(page),
            PageInput::Index(_) => match self.core().registry.find(page) {
                Some(layout) => self.scale_of_page(layout.page()),
                None => Ok(Rc::new(no_page_scale())),
            },
        }
    }

    /// `ensureLoaded(page)`: a page's scale is known as soon as the page
    /// is, since its layout carries it. Rejects `not-found` for a page that
    /// isn't there. Kept from 3.0, when a page's viewports were read on
    /// their own; the options are not read.
    pub fn ensure_loaded(
        &self,
        page: impl Into<PageInput>,
        _options: OperationOptions,
    ) -> Promise<()> {
        match self.core().registry.find(page.into()) {
            Some(_) => Promise::resolved(()),
            None => Promise::rejected(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                "no such page",
                None,
                None,
            )),
        }
    }

    /// `connect()` of the viewport sync: tell the annotation plugin each
    /// page's viewports now, again whenever a page's viewports change, and
    /// for every page on a new default scale.
    pub(crate) fn connect_viewports(&self) {
        let core = self.core();
        // What the annotation plugin was last told, by page: the same list
        // isn't told again.
        let told: Rc<RefCell<HashMap<u32, Vec<Value>>>> = Rc::default();
        let tell = {
            let this = Rc::downgrade(&self.0);
            let told = told.clone();
            move |layout: &PageRow, always: bool| -> Result<(), Error> {
                let Some(core) = this.upgrade() else { return Ok(()) };
                let persistent = is_persistent(&core.cx)?;
                let (viewports, _) = viewports_of(&core.cx, layout, persistent);
                let pon = layout.page().page_object_number;
                if !always && told.borrow().get(&pon) == Some(&viewports) {
                    return Ok(());
                }
                told.borrow_mut().insert(pon, viewports.clone());
                let fallback = fallback_of(&core.settings.get(), layout)?;
                core.annotation.set_page_viewports(
                    &layout.page().to_json(),
                    Some(viewports),
                    fallback,
                );
                Ok(())
            }
        };
        let tell = Rc::new(tell);
        let tell_changed = {
            let (this, tell) = (Rc::downgrade(&self.0), tell.clone());
            move || -> Result<(), Error> {
                let Some(core) = this.upgrade() else { return Ok(()) };
                for page in core.registry.refs() {
                    if let Some(layout) = core.registry.find(PageInput::Ref(page)) {
                        tell(&layout, false)?;
                    }
                }
                Ok(())
            }
        };
        let tell_changed = Rc::new(tell_changed);
        let _ = tell_changed();
        // `ctx.watch(() => ctx.document()?.pages, tellChanged)`.
        let registry = core.registry.clone();
        let on_pages = tell_changed.clone();
        let _ = core.cx.watch(move || Ok(registry.pages()), move |_, _| on_pages(), same);
        // `ctx.watch(() => ctx.state.get().localViewports, tellChanged)`.
        let reader = core.cx.clone();
        let on_local = tell_changed.clone();
        let _ = core.cx.watch(
            move || Ok(reader.state().local_viewports.clone()),
            move |_, _| on_local(),
            |a, b| a == b,
        );
        // A new default scale reaches the pages without one: the annotation
        // plugin measures new drawings on them with it.
        let this = Rc::downgrade(&self.0);
        let off: Unsubscribe = core.settings.events.on(move |event| {
            if !event.changed.iter().any(|key| key == "defaultScale") {
                return Ok(());
            }
            let Some(core) = this.upgrade() else { return Ok(()) };
            for page in core.registry.refs() {
                if let Some(layout) = core.registry.find(PageInput::Ref(page)) {
                    tell(&layout, true)?;
                }
            }
            Ok(())
        });
        core.cx.listen(off);
    }
}
