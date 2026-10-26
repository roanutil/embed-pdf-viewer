//! `controller.ts`: the link controller. Two sources for the clickable
//! areas: the annotation plugin's folded model when it is installed (always
//! current), else a page mirror of this plugin's own per-page reads, re-read
//! when a confirmed change (an annotation write, a redaction, a flatten)
//! touches a loaded page. Activation is pure resolution (`resolve`) plus the
//! one side effect this plugin owns, a stage reveal; everything else is
//! reported to the host, which keeps the user gesture.
//!
//! `ctx.tryGet(token)` errs only when a capability's constructor threw,
//! which fails the open before any of these calls can run, so the port
//! reads an error there as absence.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::js::number_to_string;
use epdf_annotation_core::json::{number, truthy};
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::Point;
use epdf_kernel::page_mirror::{Affected, PageMirror, PageMirrorChange, PageMirrorSpec};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::{ControllerContext, Error, Unsubscribe, is_undefined};
use epdf_plugin_actions::{ACTIONS, ActionContext, ActionOrigin, ActionSource, ActionTriggerEvent};
use epdf_plugin_annotation::promise::all;
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stage::STAGE;
use serde_json::{Value, json};

use crate::contract::*;
use crate::source::links_of;

pub(crate) const CAPABILITY: &str = "link";

/// `bounds.member` as a number; anything else is `NaN`, which no
/// comparison holds.
fn coordinate(bounds: &Value, key: &str) -> f64 {
    bounds.get(key).and_then(number).unwrap_or(f64::NAN)
}

/// `contains(bounds, point)`: edges included.
fn contains(bounds: &Value, point: Point) -> bool {
    let (x, y) = (coordinate(bounds, "x"), coordinate(bounds, "y"));
    let (width, height) = (coordinate(bounds, "width"), coordinate(bounds, "height"));
    point.x >= x && point.x <= x + width && point.y >= y && point.y <= y + height
}

fn area(link: &Value) -> f64 {
    let bounds = link.get("bounds").unwrap_or(&Value::Null);
    coordinate(bounds, "width") * coordinate(bounds, "height")
}

/// `pagesChangedBy(event)`: the pages whose annotation lists an event
/// changed.
pub fn pages_changed_by(event: &Value) -> Option<Vec<Value>> {
    let page_of = |value: Option<&Value>| value.cloned().unwrap_or_else(epdf_kernel::undefined);
    let applied = |result: &Value| result.get("status").and_then(Value::as_str) == Some("applied");
    let results = || event.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    match event.get("type").and_then(Value::as_str)? {
        "annotations.created" | "annotations.updated" => {
            Some(vec![page_of(event.get("annotation").and_then(|a| a.get("page")))])
        }
        "annotations.deleted" | "annotations.reordered" => Some(vec![page_of(event.get("page"))]),
        // These can remove annotations, links included, from the pages they
        // applied to.
        "redaction.applied" | "pages.flattened" => {
            Some(results().iter().filter(|r| applied(r)).map(|r| page_of(r.get("page"))).collect())
        }
        "annotations.flattened" => {
            results().iter().any(applied).then(|| vec![page_of(event.get("page"))])
        }
        _ => None,
    }
}

pub(crate) struct Core {
    pub(crate) cx: ControllerContext,
    activated: EventHookSource<LinkActivatedEvent>,
    loaded: EventHookSource<LinkLoadedEvent>,
    pages: PageMirror,
    /// The framework's website openers, the latest first: the plugin never
    /// opens one itself.
    uri_openers: RefCell<Vec<UriOpener>>,
}

/// `LinkHostCapability`, which includes the public `LinkCapability`: one
/// runtime object. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct LinkCapability(pub(crate) Rc<Core>);

/// `createLinkController(ctx)`: the capability and its `connect` half.
pub fn create_link_controller(cx: ControllerContext) -> Result<(LinkCapability, ConnectFn), Error> {
    let activated = cx.event_source::<LinkActivatedEvent>();
    let loaded = cx.event_source::<LinkLoadedEvent>();
    let reader = cx.clone();
    let announce = loaded.clone();
    let pages = cx.page_mirror(PageMirrorSpec {
        name: "links".into(),
        load: Rc::new(move |doc, page, _signal| {
            let cx = reader.clone();
            Promise::spawn(async move {
                let pon = PageRef::from_json(&page);
                if pon.and_then(|p| cx.get_page(p)).is_none() {
                    return Err(plugin_error(
                        code::NOT_FOUND,
                        CAPABILITY,
                        format!(
                            "page {} is not in this document",
                            page.get("objectNumber")
                                .and_then(number)
                                .map_or_else(|| "undefined".to_string(), number_to_string)
                        ),
                        None,
                        None,
                    ));
                }
                let doc = doc.ok_or_else(|| {
                    plugin_error(code::NOT_READY, CAPABILITY, "no document", None, None)
                })?;
                let pon = pon.map(|p| p.page_object_number);
                let snapshot = doc.call("page.annotations.list", pon, Vec::new()).await?;
                let annotations = snapshot.get("annotations").cloned().unwrap_or(Value::Null);
                Ok(Value::Array(links_of(&annotations)?))
            })
        }),
        affected: Rc::new(|event| match pages_changed_by(event) {
            Some(pages) => Affected::Pages(pages),
            None => Affected::None,
        }),
        fold: None,
        predict: None,
        changed: Some(Rc::new(move |change: PageMirrorChange| {
            if change.cause == "load" {
                announce.emit(&LinkLoadedEvent { page: change.page });
            }
            Ok(())
        })),
    })?;
    let api = LinkCapability(Rc::new(Core {
        cx,
        activated,
        loaded,
        pages,
        uri_openers: RefCell::default(),
    }));
    let connected = api.clone();
    let connect: ConnectFn = Rc::new(move || crate::connect::connect_link(&connected.0.cx));
    Ok((api, connect))
}

impl LinkCapability {
    fn cx(&self) -> &ControllerContext {
        &self.0.cx
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.cx().lifetime()
    }

    /// The document's pointer hub, which `linkPlugin()` requires.
    #[cfg(feature = "uniffi")]
    pub(crate) fn cx_interaction(&self) -> Option<epdf_plugin_interaction::InteractionCapability> {
        self.cx().try_get(&INTERACTION).ok().flatten()
    }

    fn annotation_host(&self) -> Option<AnnotationCapability> {
        self.cx().try_get(&ANNOTATION).ok().flatten()
    }

    /// The document's page list (`ctx.document()?.pages ?? []`).
    fn layouts(&self) -> Vec<Value> {
        self.cx().document().and_then(|meta| meta.pages.as_array().cloned()).unwrap_or_default()
    }

    /// `ctx.getPage(page)`: the page's registry row, or `None` for a page
    /// that isn't in the document. An index reads `pages[index]`.
    pub fn get_page(&self, page: PageTarget) -> Option<Value> {
        let meta = self.cx().document()?;
        let rows = meta.pages.as_array()?;
        match page {
            PageTarget::Ref(page) => rows.iter().find(|row| page.names(row)).cloned(),
            PageTarget::Index(index) if index >= 0.0 && index.fract() == 0.0 => {
                rows.get(index as usize).cloned()
            }
            PageTarget::Index(_) => None,
        }
    }

    /// `ctx.getPage(page)?.ref`.
    fn ref_of(&self, page: PageTarget) -> Option<PageRef> {
        self.get_page(page).and_then(|row| row.get("ref").and_then(PageRef::from_json))
    }

    /// `ctx.pageOf(page).ref`: `not-found` for a page that isn't there.
    fn page_of(&self, page: PageTarget) -> Result<PageRef, Error> {
        self.ref_of(page).ok_or_else(|| {
            let message = match page {
                PageTarget::Ref(page) => {
                    format!("page {} is not in this document", page.page_object_number)
                }
                PageTarget::Index(index) => {
                    format!("there is no page {} in this document", number_to_string(index))
                }
            };
            plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
        })
    }

    /// `hasPage` for [`resolve_link_target`]: a destination's page as the
    /// target holds it, a ref or an index.
    fn has_page(&self, page: &Value) -> bool {
        let target = match page {
            Value::Number(_) => number(page).map(PageTarget::Index),
            other => PageRef::from_json(other).map(PageTarget::Ref),
        };
        target.is_some_and(|target| self.get_page(target).is_some())
    }

    /// Clickable areas on a page (its ref or its index), page space.
    /// Empty for a page that isn't in the document.
    pub fn list_links(&self, page: impl Into<PageTarget>) -> Rc<Value> {
        let Some(page) = self.ref_of(page.into()) else { return Rc::new(json!([])) };
        if let Some(host) = self.annotation_host() {
            return Rc::new(host.list_link_items(&page.to_json()));
        }
        self.0.pages.get(&page.to_json()).unwrap_or_else(|| Rc::new(json!([])))
    }

    /// One link by its stable id, or `None`.
    pub fn get_link(&self, page: impl Into<PageTarget>, link_id: &str) -> Option<Link> {
        let links = self.list_links(page);
        links
            .as_array()?
            .iter()
            .find(|link| link.get("id").and_then(Value::as_str) == Some(link_id))
            .cloned()
    }

    /// The topmost (smallest) link under a page point; the first of equal
    /// areas.
    pub fn get_link_at(&self, page: impl Into<PageTarget>, point: Point) -> Option<Link> {
        let links = self.list_links(page);
        let mut best: Option<&Value> = None;
        for link in links.as_array()? {
            if !contains(link.get("bounds").unwrap_or(&Value::Null), point) {
                continue;
            }
            if best.is_none_or(|b| area(link) < area(b)) {
                best = Some(link);
            }
        }
        best.cloned()
    }

    /// Every link in the document; loads pages as needed. Rejects when a
    /// page's read fails, and `operation-cancelled` when the signal fires.
    pub fn list_all_links(&self, options: OperationOptions) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let pages: Vec<PageRef> = this
                .layouts()
                .iter()
                .filter_map(|layout| layout.get("ref").and_then(PageRef::from_json))
                .collect();
            let loads =
                pages.iter().map(|page| this.ensure_loaded(*page, options.clone())).collect();
            cancellable(CAPABILITY, options.signal.clone(), all(loads)).await?;
            let mut links = Vec::new();
            for page in pages {
                if let Value::Array(items) = &*this.list_links(page) {
                    links.extend(items.iter().cloned());
                }
            }
            Ok(Value::Array(links))
        })
    }

    /// Load a page's links. Resolves at once when the annotation plugin
    /// owns them; rejects when the read fails, which `get_status` then
    /// reports, `not-found` for a page that isn't in the document, and
    /// `operation-cancelled` when the signal fires. A caller's cancel
    /// leaves the shared read running for the others.
    pub fn ensure_loaded(
        &self,
        page: impl Into<PageTarget>,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.clone();
        let page = page.into();
        Promise::spawn(async move {
            let page = this.page_of(page)?;
            // With the annotation plugin installed, its model owns the data.
            if this.annotation_host().is_some() {
                return Ok(());
            }
            cancellable(CAPABILITY, options.signal, this.0.pages.ensure_loaded(page.to_json()))
                .await
        })
    }

    /// The page's links were read and are current. Always true with the
    /// annotation plugin.
    pub fn is_loaded(&self, page: impl Into<PageTarget>) -> bool {
        self.ref_of(page.into()).is_some_and(|page| {
            self.annotation_host().is_some()
                || self.0.pages.get_status(&page.to_json()) == ResourceStatus::Ready
        })
    }

    /// Load state of a page's links.
    pub fn get_status(&self, page: impl Into<PageTarget>) -> ResourceStatus {
        let Some(page) = self.ref_of(page.into()) else { return ResourceStatus::Idle };
        if self.annotation_host().is_some() {
            ResourceStatus::Ready
        } else {
            self.0.pages.get_status(&page.to_json())
        }
    }

    /// What activation would do, with no side effect. A page the document
    /// doesn't have can't be shown.
    pub fn resolve(&self, target: &PdfLinkTarget) -> LinkResolution {
        resolve_link_target(target, |page| self.has_page(page))
    }

    fn perform(
        &self,
        target: &PdfLinkTarget,
        context: Option<&LinkActivateContext>,
    ) -> LinkActivation {
        let actions = self.cx().try_get(&ACTIONS).ok().flatten();
        let tree = context.and_then(|c| c.activate.as_ref()).filter(|t| truthy(Some(t)));
        if let (Some(actions), Some(tree), Some(context)) = (actions, tree, context) {
            let defined = |v: &Option<Value>| v.clone().filter(|v| !is_undefined(v));
            let dispatch = actions.execute(
                tree.clone(),
                ActionContext {
                    origin: ActionOrigin::User,
                    source: ActionSource::Link {
                        annotation: defined(&context.ref_),
                        page: defined(&context.page).as_ref().and_then(PageRef::from_json),
                    },
                    event: ActionTriggerEvent::Activate,
                },
                None,
            );
            return LinkActivation::Dispatched { dispatch };
        }
        match self.resolve(target) {
            LinkResolution::Destination { destination } => {
                // The view the link was followed in, by its own scroll
                // behavior.
                let stage = context
                    .and_then(|c| c.stage.clone())
                    .or_else(|| self.cx().try_get(&STAGE).ok().flatten());
                let Some(stage) = stage else {
                    return LinkActivation::Destination { destination };
                };
                if let Ok(destination) = serde_json::from_value(destination) {
                    stage.go_to_destination(&destination, None);
                }
                LinkActivation::Revealed
            }
            LinkResolution::Uri { uri } => {
                let open = self.0.uri_openers.borrow().first().cloned();
                match open {
                    Some(open) if !open(&uri) => {
                        LinkActivation::Reported { target: target.clone() }
                    }
                    _ => LinkActivation::Uri { uri },
                }
            }
            LinkResolution::Named { name } => LinkActivation::Named { name },
            LinkResolution::Reported { .. } => LinkActivation::Reported { target: target.clone() },
        }
    }

    /// Follow a link, or a link's target, as a click does, and return what
    /// happened at once. A `goto` moves the stage, by its `scrollBehavior`
    /// setting; a website opens through the framework; a link with an `/A`
    /// tree dispatches through the actions plugin. Fires `on_activated`.
    pub fn activate(&self, input: &Value, context: Option<&LinkActivateContext>) -> LinkActivation {
        let link = is_link(input);
        let target =
            if link { input.get("target").cloned().unwrap_or(Value::Null) } else { input.clone() };
        let link_context = if link {
            let own = LinkActivateContext {
                activate: input.get("activate").cloned(),
                ref_: input.get("ref").cloned(),
                ..LinkActivateContext::default()
            };
            Some(match context {
                Some(context) => own.spread(context),
                None => own,
            })
        } else {
            context.cloned()
        };
        let activation = self.perform(&target, link_context.as_ref());
        self.0.activated.emit(&LinkActivatedEvent { target, activation: activation.clone() });
        activation
    }

    /// Hit test, then activate with the page in the context. `None` when no
    /// link is there; `not-found` for a page that isn't in the document.
    pub fn activate_at(
        &self,
        page: impl Into<PageTarget>,
        point: Point,
        context: Option<&LinkActivateContext>,
    ) -> Result<Option<LinkActivation>, Error> {
        let page = self.page_of(page.into())?;
        let Some(link) = self.get_link_at(page, point) else { return Ok(None) };
        let with_page =
            LinkActivateContext { page: Some(page.to_json()), ..LinkActivateContext::default() };
        let merged = match context {
            Some(context) => with_page.spread(context),
            None => with_page,
        };
        Ok(Some(self.activate(&link, Some(&merged))))
    }

    /// A human label for tooltips.
    pub fn get_label(&self, input: &Value) -> String {
        let target = if is_link(input) { &input["target"] } else { input };
        let text = |key: &str| match target.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(other) if !is_undefined(other) => other.to_string(),
            _ => "undefined".into(),
        };
        match target.get("kind").and_then(Value::as_str) {
            Some("uri") => text("uri"),
            Some("goto") => "Go to destination".into(),
            Some("named") => text("name"),
            _ => "Link".into(),
        }
    }

    /// After the built-in handling of any activation.
    pub fn on_activated(
        &self,
        listener: impl Fn(&LinkActivatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.activated.on(listener)
    }

    /// A page's links were read, first or again after an annotation change
    /// on it. Never fires for a failed read, nor while the annotation
    /// plugin owns the links.
    pub fn on_loaded(
        &self,
        listener: impl Fn(&LinkLoadedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.loaded.on(listener)
    }

    /// Register the framework's website opener. Without one, a website link
    /// activates as `uri` for the caller to open. The latest registration
    /// wins until it is removed.
    pub fn register_uri_opener(&self, opener: UriOpener) -> Unsubscribe {
        self.0.uri_openers.borrow_mut().insert(0, opener.clone());
        let core = Rc::downgrade(&self.0);
        Unsubscribe::new(move || {
            if let Some(core) = core.upgrade() {
                let mut openers = core.uri_openers.borrow_mut();
                if let Some(index) = openers.iter().position(|o| Rc::ptr_eq(o, &opener)) {
                    openers.remove(index);
                }
            }
        })
    }

    /// Does the active tool navigate links (the layer paints anchors only
    /// then)?
    pub fn is_navigation_engaged(&self) -> bool {
        self.cx()
            .try_get(&INTERACTION)
            .ok()
            .flatten()
            .is_some_and(|interaction| interaction.active_tool_enables("link-nav"))
    }
}
