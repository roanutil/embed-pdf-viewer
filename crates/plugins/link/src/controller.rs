//! `controller.ts`: the link controller. The data source is decided per
//! call: the annotation plugin's folded model when it is installed (always
//! current), else this plugin's own per-page reads.
//!
//! `anno()` is `ctx.tryGet(AnnotationHostToken)`. The kernel's `try_get`
//! errs only when a capability's constructor threw, which fails the open
//! before any of these calls can run, so the port reads it as absence.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::{PageObjectNumber, PageRef, PdfLinkTarget};
use epdf_geometry::Point;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error, Unsubscribe};
use epdf_plugin_actions::{ACTIONS, ActionContext, ActionOrigin, ActionSource, ActionTriggerEvent};
use epdf_plugin_annotation::promise::{all, spawn, then};
use epdf_plugin_annotation::value::{get, str_of, truthy};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stage::{
    PageLayout, RevealOptions, STAGE, ScrollBehaviorKind, destination_to_reveal, without_undefined,
};
use serde_json::{Value, json};

use crate::contract::*;
use crate::model::{LinkAction, LinkState};
use crate::source::load_links_page;

fn user_origin() -> Value {
    json!({ "locality": "local", "trigger": "user", "sessionId": null, "actorId": null })
}

fn contains(b: &epdf_annotation_core::Rect, p: Point) -> bool {
    p.x >= b.x && p.x <= b.x + b.width && p.y >= b.y && p.y <= b.y + b.height
}

struct Core {
    cx: Controller<LinkState, LinkAction>,
    activated: EventHookSource<LinkActivatedEvent>,
    loaded: EventHookSource<LinkLoadedEvent>,
    loads: RefCell<HashMap<PageObjectNumber, Promise<()>>>,
}

/// `LinkHostCapability`, which includes the public `LinkCapability`. Cheap
/// to clone; clones share state.
#[derive(Clone)]
pub struct LinkCapability(Rc<Core>);

/// `createLinkController(ctx)`: the capability; the plugin's `connect`
/// then calls [`LinkCapability::connect`].
pub fn create_link_controller(
    cx: Controller<LinkState, LinkAction>,
    config: &LinkConfig,
) -> LinkCapability {
    let console_error = config.host.console_error.clone();
    let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
        if let Some(sink) = &console_error {
            sink("[link] event listener failed:", error);
        }
    });
    let activated = EventHookSource::new(Some(report.clone()));
    let loaded = EventHookSource::new(Some(report));
    let (a, l) = (activated.clone(), loaded.clone());
    cx.defer(Box::new(move || {
        a.dispose();
        l.dispose();
        Ok(None)
    }));
    LinkCapability(Rc::new(Core { cx, activated, loaded, loads: RefCell::default() }))
}

impl LinkCapability {
    fn anno(&self) -> Option<AnnotationCapability> {
        self.0.cx.try_get(&ANNOTATION).ok().flatten()
    }

    fn is_cached(&self, page: &PageRef) -> bool {
        self.0.cx.state().pages.contains_key(&page.page_object_number)
    }

    /// The document's layout rows.
    fn layouts(&self) -> Vec<Value> {
        self.0.cx.document().and_then(|d| d.pages.as_array().cloned()).unwrap_or_default()
    }

    fn page_refs(&self) -> Vec<PageRef> {
        self.layouts()
            .iter()
            .filter_map(|l| serde_json::from_value(l.get("ref")?.clone()).ok())
            .collect()
    }

    /// Whether the annotation plugin supplies the links (its `read/render.ts`
    /// builds them), rather than this plugin's own `source.ts`.
    pub fn uses_annotation_links(&self) -> bool {
        self.anno().is_some()
    }

    /// Clickable areas on a page, page space.
    pub fn list_links(&self, page: &PageRef) -> Vec<Link> {
        if let Some(host) = self.anno() {
            return host.list_link_items(*page);
        }
        self.0.cx.state().pages.get(&page.page_object_number).cloned().unwrap_or_default()
    }

    /// One link by its stable id.
    pub fn get_link(&self, page: &PageRef, link_id: &str) -> Option<Link> {
        self.list_links(page).into_iter().find(|l| l.id == link_id)
    }

    /// The topmost (smallest) link under a page point; the first of equal
    /// areas.
    pub fn get_link_at(&self, page: &PageRef, point: Point) -> Option<Link> {
        let mut best: Option<Link> = None;
        for link in self.list_links(page) {
            if !contains(&link.bounds, point) {
                continue;
            }
            let area = |l: &Link| l.bounds.width * l.bounds.height;
            if best.as_ref().is_none_or(|b| area(&link) < area(b)) {
                best = Some(link);
            }
        }
        best
    }

    /// Every link in the document; loads pages as needed.
    pub fn list_all_links(&self) -> Promise<Vec<Link>> {
        let this = self.clone();
        spawn(async move {
            let pages = this.page_refs();
            all(pages.iter().map(|p| this.ensure_loaded(p)).collect()).await?;
            Ok(pages.iter().flat_map(|p| this.list_links(p)).collect())
        })
    }

    /// Load a page's links. Resolves at once when the annotation plugin owns
    /// them or the page is cached; a second call while a read is in flight
    /// joins it. A load that rejected stays in flight, as the TypeScript's
    /// map keeps it.
    pub fn ensure_loaded(&self, page: &PageRef) -> Promise<()> {
        if self.anno().is_some() || self.is_cached(page) {
            return Promise::resolved(());
        }
        let pon = page.page_object_number;
        if let Some(in_flight) = self.0.loads.borrow().get(&pon) {
            return in_flight.clone();
        }
        let this = self.clone();
        let page = *page;
        let read = load_links_page(&self.0.cx, page);
        let load = spawn(async move {
            read.await?;
            this.0.loads.borrow_mut().remove(&pon);
            if this.is_cached(&page) {
                this.0.loaded.emit(&LinkLoadedEvent { page });
            }
            Ok(())
        });
        self.0.loads.borrow_mut().insert(pon, load.clone());
        load
    }

    pub fn is_loaded(&self, page: &PageRef) -> bool {
        self.anno().is_some() || self.is_cached(page)
    }

    /// The layout row of the page `page` names.
    fn layout_of(&self, page: &PageRef) -> Option<Value> {
        self.layouts().into_iter().find(|l| {
            l.get("ref").and_then(|r| r.get("pageObjectNumber")).and_then(Value::as_u64)
                == Some(u64::from(page.page_object_number))
        })
    }

    /// What activation would do, with no side effect. `Err` is a
    /// destination whose page cannot be inverted (`userUnit` 0).
    pub fn resolve(&self, target: &PdfLinkTarget) -> Result<LinkResolution, Error> {
        Ok(match target {
            PdfLinkTarget::Goto { destination } => {
                let page = epdf_kernel::js::to_value(destination)
                    .get("page")
                    .and_then(|p| serde_json::from_value::<PageRef>(p.clone()).ok());
                let Some((page, layout)) = page.and_then(|p| Some((p, self.layout_of(&p)?))) else {
                    return Ok(LinkResolution::Destination { destination: destination.clone() });
                };
                let layout: PageLayout = serde_json::from_value(without_undefined(&layout))
                    .map_err(|e| Error::new(format!("[link] layout: {e}")))?;
                let staged = serde_json::from_value(without_undefined(&epdf_kernel::js::to_value(
                    destination,
                )))
                .map_err(|e| Error::new(format!("[link] destination: {e}")))?;
                let reveal = destination_to_reveal(&staged, &layout, None)
                    .map_err(|e| Error::new(e.to_string()))?;
                LinkResolution::Reveal {
                    page,
                    page_index: reveal.page_index,
                    options: reveal.options,
                }
            }
            PdfLinkTarget::Uri { uri } => LinkResolution::Uri { uri: uri.clone() },
            PdfLinkTarget::Named { name } => LinkResolution::Named { name: name.clone() },
            other => LinkResolution::Reported { target: other.clone() },
        })
    }

    /// Perform the activation. A `goto` reveals through the stage; `uri`
    /// and `named` are reported for the host; a link with an `/A` tree
    /// dispatches through the actions plugin. `Err` is a link whose target
    /// does not decode, or a destination that cannot be inverted.
    pub fn activate(
        &self,
        input: &LinkInput,
        context: Option<&LinkActivateContext>,
    ) -> Result<LinkActivation, Error> {
        let (target, ctx_of) = match input {
            LinkInput::Link(link) => {
                let target: PdfLinkTarget = serde_json::from_value(link.target.clone())
                    .map_err(|e| Error::new(format!("[link] target: {e}")))?;
                let own = LinkActivateContext {
                    activate: link.activate.clone(),
                    ref_: link.ref_.as_ref().map(epdf_kernel::js::to_value),
                    page: None,
                };
                let merged = match context {
                    Some(context) => own.spread(context),
                    None => own,
                };
                (target, Some(merged))
            }
            LinkInput::Target(target) => (target.clone(), context.cloned()),
        };
        let activation = self.activation_of(&target, ctx_of.as_ref())?;
        self.0.activated.emit(&LinkActivatedEvent {
            target,
            activation: activation.clone(),
            origin: user_origin(),
        });
        Ok(activation)
    }

    fn activation_of(
        &self,
        target: &PdfLinkTarget,
        ctx_of: Option<&LinkActivateContext>,
    ) -> Result<LinkActivation, Error> {
        let actions = self.0.cx.try_get(&ACTIONS)?;
        let tree = ctx_of.and_then(|c| c.activate.as_ref()).filter(|t| truthy(Some(t)));
        if let (Some(actions), Some(tree), Some(ctx_of)) = (actions, tree, ctx_of) {
            let dispatch = actions.execute(
                tree.clone(),
                ActionContext {
                    origin: ActionOrigin::User,
                    source: ActionSource::Link {
                        annotation: ctx_of
                            .ref_
                            .as_ref()
                            .and_then(|r| serde_json::from_value(r.clone()).ok()),
                        page: ctx_of
                            .page
                            .as_ref()
                            .and_then(epdf_kernel::geometry::PageRef::from_json),
                    },
                    event: ActionTriggerEvent::Activate,
                },
            );
            return Ok(LinkActivation::Dispatched { dispatch });
        }
        Ok(match self.resolve(target)? {
            LinkResolution::Reveal { page_index, options, .. } => {
                let stage = self.0.cx.try_get(&STAGE)?;
                let (Some(stage), PdfLinkTarget::Goto { .. }) = (stage, target) else {
                    return Ok(match target {
                        PdfLinkTarget::Goto { destination } => {
                            LinkActivation::Destination { destination: destination.clone() }
                        }
                        other => LinkActivation::Reported { target: other.clone() },
                    });
                };
                stage.reveal_index(
                    page_index as isize,
                    Some(RevealOptions { behavior: Some(ScrollBehaviorKind::Smooth), ..options }),
                );
                LinkActivation::Revealed
            }
            LinkResolution::Destination { destination } => {
                LinkActivation::Destination { destination }
            }
            LinkResolution::Uri { uri } => LinkActivation::Uri { uri },
            LinkResolution::Named { name } => LinkActivation::Named { name },
            LinkResolution::Reported { target } => LinkActivation::Reported { target },
        })
    }

    /// Hit test, then activate with the page in the context. `None` when
    /// nothing is there.
    pub fn activate_at(
        &self,
        page: &PageRef,
        point: Point,
        context: Option<&LinkActivateContext>,
    ) -> Result<Option<LinkActivation>, Error> {
        let Some(link) = self.get_link_at(page, point) else { return Ok(None) };
        let with_page = LinkActivateContext {
            page: Some(epdf_kernel::js::to_value(page)),
            ..LinkActivateContext::default()
        };
        let merged = match context {
            Some(context) => with_page.spread(context),
            None => with_page,
        };
        self.activate(&LinkInput::Link(Box::new(link)), Some(&merged)).map(Some)
    }

    /// A human label for tooltips, from the target's `kind` as JSON. A
    /// whole link (an object with `target` and `bounds`, the TypeScript's
    /// `isLink`) answers for its target.
    pub fn get_label(&self, input: &Value) -> String {
        let target = if is_link(input) { &input["target"] } else { input };
        match str_of(target, "kind") {
            Some("uri") => get(target, "uri").map_or("undefined".into(), |u| {
                u.as_str().map_or_else(|| u.to_string(), str::to_string)
            }),
            Some("goto") => "Go to destination".into(),
            Some("named") => get(target, "name").map_or("undefined".into(), |n| {
                n.as_str().map_or_else(|| n.to_string(), str::to_string)
            }),
            _ => "Link".into(),
        }
    }

    /// [`get_label`](Self::get_label) for a typed input.
    pub fn get_input_label(&self, input: &LinkInput) -> String {
        match input {
            LinkInput::Link(link) => self.get_label(&link.target),
            LinkInput::Target(target) => self.get_label(&epdf_kernel::js::to_value(target)),
        }
    }

    /// After the built-in handling of any activation.
    pub fn on_activated(
        &self,
        listener: impl Fn(&LinkActivatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.activated.on(listener)
    }

    /// A page's links arrived.
    pub fn on_loaded(
        &self,
        listener: impl Fn(&LinkLoadedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.loaded.on(listener)
    }

    /// Does the active tool navigate links (the layer paints anchors only
    /// then)?
    pub fn is_navigation_engaged(&self) -> bool {
        self.0
            .cx
            .try_get(&INTERACTION)
            .ok()
            .flatten()
            .is_some_and(|i| i.get_active_tool().enables("link-nav"))
    }

    /// `connect()`: stand-alone source only, a confirmed annotation change
    /// re-reads the page it touched (the annotation model owns the data
    /// otherwise).
    pub(crate) fn connect(&self) {
        let Ok(doc) = self.0.cx.doc() else { return };
        let this = self.clone();
        let off = doc.subscribe(Rc::new(move |event: &Value| {
            let page = match str_of(event, "type") {
                Some("annotation.created") => get(event, "created").and_then(|c| get(c, "page")),
                Some("annotation.updated") => get(event, "updated").and_then(|u| get(u, "page")),
                Some("annotation.moved") => get(event, "moved")
                    .and_then(Value::as_array)
                    .and_then(|m| m.first())
                    .and_then(|first| get(first, "page")),
                Some("annotation.deleted") => get(event, "page"),
                _ => None,
            };
            if let Some(page) = page.and_then(|p| serde_json::from_value::<PageRef>(p.clone()).ok())
            {
                this.refetch(page);
            }
        }));
        self.0.cx.listen(off);
    }

    fn refetch(&self, page: PageRef) {
        if self.anno().is_some() || !self.is_cached(&page) {
            return;
        }
        let loaded = self.0.loaded.clone();
        then(load_links_page(&self.0.cx, page), move |result| {
            if result.is_ok() {
                loaded.emit(&LinkLoadedEvent { page });
            }
        });
    }
}

/// `isLink`: `'target' in value && 'bounds' in value`.
pub(crate) fn is_link(value: &Value) -> bool {
    value.get("target").is_some() && value.get("bounds").is_some()
}
