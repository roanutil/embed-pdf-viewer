//! The link plugin through uniffi: a page's links, what each targets, and
//! activating them, as one handle and one event enum.
//!
//! Compiled and tested since Task 13.33. Since Task
//! 13.21 the plugin carries links, targets and destinations as the engine's
//! records (JSON), as the annotation plugin does since 13.17, so they cross
//! here as JSON strings (`target_json`, `destination_json`). They are in page
//! space, so a host decodes a target with `epdf_ffi::PageLinkTarget::from_json`
//! and a destination with `epdf_ffi::PageDestination::from_json`;
//! `epdf_ffi::PdfLinkTarget` is the readers' file-space form and drops a
//! page-space `x`/`y`. A link's action tree stays in
//! Rust: Swift activates a link by page and id, and the handle looks it up
//! again so the tree goes with it. A `goto` resolves to its page-space
//! destination; the stage reveal happens on activation.
//!
//! Every method hops to the kernel thread through `KernelBound`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    KernelBound, KernelPost, PageRef, Point, Rect, RegistrationHandle, Subscription, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_annotation::ANNOTATION_EDIT_PRIORITY;
use epdf_plugin_interaction::{InteractionHandler, PointerSample, RegisterHandlerOptions, Tool};
use serde_json::{Value, json};

use crate::{LinkActivateContext, LinkCapability, OperationOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LinkHoverFlags {
    pub enter: bool,
    pub exit: bool,
}

/// One clickable link area.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkItem {
    /// Stable per page: `obj:<n>`, or `base:<page>:<n>` for an annotation
    /// of the uploaded base PDF.
    pub id: String,
    pub page: PageRef,
    /// Page space, y-down.
    pub bounds: Rect,
    /// The `PdfLinkTarget` record.
    pub target_json: String,
    /// A reply grouped under another annotation.
    pub attached: bool,
    /// The `AnnotationRef` record, when the link is an annotation.
    pub annotation_json: Option<String>,
    /// The link carries an action tree, which runs instead of the target
    /// when the actions plugin is installed.
    pub has_action: bool,
    pub hover: Option<LinkHoverFlags>,
}

fn invalid(what: String) -> Error {
    plugin_error(code::INVALID_INPUT, "link", format!("[link] {what}"), None, None)
}

fn page_json(page: PageRef) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": page.page_object_number })
}

fn json_of(text: &str, what: &str) -> Result<Value, Error> {
    serde_json::from_str(text).map_err(|e| invalid(format!("{what} is not JSON: {e}")))
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

fn item(page: PageRef, link: &Value) -> Result<LinkItem, Error> {
    let bounds = link.get("bounds").cloned().unwrap_or(Value::Null);
    let hover = link.get("hoverEvents").map(|h| LinkHoverFlags {
        enter: h.get("enter").and_then(Value::as_bool).unwrap_or(false),
        exit: h.get("exit").and_then(Value::as_bool).unwrap_or(false),
    });
    Ok(LinkItem {
        id: link
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("a link without an id: {link}")))?
            .to_string(),
        page,
        bounds: Rect {
            x: number(&bounds, "x"),
            y: number(&bounds, "y"),
            width: number(&bounds, "width"),
            height: number(&bounds, "height"),
        },
        target_json: link.get("target").cloned().unwrap_or(Value::Null).to_string(),
        attached: link.get("attached").and_then(Value::as_bool).unwrap_or(false),
        annotation_json: link.get("ref").filter(|r| !r.is_null()).map(Value::to_string),
        has_action: link.get("activate").is_some(),
        hover,
    })
}

/// The page a link's annotation ref names, for a link of `listAllLinks`.
fn page_of_link(link: &Value) -> Option<PageRef> {
    let pon = link.pointer("/ref/page/objectNumber")?.as_u64()?;
    Some(PageRef { page_object_number: u32::try_from(pon).ok()? })
}

/// A bare target, or a link named by its page and id.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkInput {
    Target { target_json: String },
    Link { page: PageRef, id: String },
}

/// What activating a target would do, with no side effect.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkResolution {
    Destination { destination_json: String },
    Uri { uri: String },
    Named { name: String },
    Reported { target_json: String },
}

fn text(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_string)
}

impl From<crate::LinkResolution> for LinkResolution {
    fn from(r: crate::LinkResolution) -> Self {
        match r {
            crate::LinkResolution::Destination { destination } => {
                LinkResolution::Destination { destination_json: destination.to_string() }
            }
            crate::LinkResolution::Uri { uri } => LinkResolution::Uri { uri: text(&uri) },
            crate::LinkResolution::Named { name } => LinkResolution::Named { name: text(&name) },
            crate::LinkResolution::Reported { target } => {
                LinkResolution::Reported { target_json: target.to_string() }
            }
        }
    }
}

/// What an activation did, or hands to the app to do. `Uri`, `Named` and
/// `Destination` are the app's: the plugin never opens a URL. `Dispatched`
/// means the link's action tree went to the actions plugin.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkActivation {
    Revealed,
    Destination { destination_json: String },
    Uri { uri: String },
    Named { name: String },
    Reported { target_json: String },
    Dispatched,
    None,
}

impl From<&crate::LinkActivation> for LinkActivation {
    fn from(a: &crate::LinkActivation) -> Self {
        match a {
            crate::LinkActivation::Revealed => LinkActivation::Revealed,
            crate::LinkActivation::Destination { destination } => {
                LinkActivation::Destination { destination_json: destination.to_string() }
            }
            crate::LinkActivation::Uri { uri } => LinkActivation::Uri { uri: text(uri) },
            crate::LinkActivation::Named { name } => LinkActivation::Named { name: text(name) },
            crate::LinkActivation::Reported { target } => {
                LinkActivation::Reported { target_json: target.to_string() }
            }
            crate::LinkActivation::Dispatched { .. } => LinkActivation::Dispatched,
            crate::LinkActivation::None => LinkActivation::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkActivatedEvent {
    pub target_json: String,
    pub activation: LinkActivation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LinkLoadedEvent {
    pub page: PageRef,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkEvent {
    Activated { event: LinkActivatedEvent },
    Loaded { event: LinkLoadedEvent },
}

#[uniffi::export(with_foreign)]
pub trait LinkEventListener: Send + Sync {
    fn on_event(&self, event: LinkEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

fn open(cap: &LinkCapability) -> Result<&LinkCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "link",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn kernel_page(page: PageRef) -> epdf_kernel::geometry::PageRef {
    epdf_kernel::geometry::PageRef::new(page.page_object_number)
}

fn kernel_point(point: Point) -> epdf_kernel::geometry::Point {
    epdf_kernel::geometry::Point { x: point.x, y: point.y }
}

/// The plugin's input for a foreign one: a link is looked up again so its
/// action tree goes with it.
fn input_of(cap: &LinkCapability, input: LinkInput) -> Result<Value, Error> {
    match input {
        LinkInput::Target { target_json } => json_of(&target_json, "a link target"),
        LinkInput::Link { page, id } => cap.get_link(kernel_page(page), &id).ok_or_else(|| {
            plugin_error(code::NOT_FOUND, "link", format!("no link {id} on that page"), None, None)
        }),
    }
}

/// A link's tap on a native surface, as the web's link layer
/// (`packages/framework/react/src/link.tsx`) takes a click on its anchors:
/// only while the active tool enables `link-nav`, with an attached link
/// standing down while the tool can edit annotations, and the smallest link
/// winning. The down captures the gesture, as the anchor's `pointerdown`
/// keeps it from the hub; the up activates when it lands on the same link
/// within [`TAP_SLOP`] of the down, as a click does (a browser drops the
/// click after a scroll). A touch is never claimed, so a drag that starts
/// on a link still scrolls. The annotation `/AA` events the web's anchors
/// also dispatch (`mouseDown`, `mouseUp`, hover) are not sent from here.
struct TapHandler {
    link: LinkCapability,
    /// The pressed link's page and id, and where the down was.
    pressed: RefCell<Option<(epdf_kernel::geometry::PageRef, String, (f64, f64))>>,
}

/// How far, in viewport units, an up may land from its down and still be a
/// tap rather than a drag.
const TAP_SLOP: f64 = 10.0;

impl TapHandler {
    fn link_under(
        &self,
        sample: &PointerSample,
    ) -> Option<(epdf_kernel::geometry::PageRef, Value)> {
        let hit = sample.page.as_ref()?;
        let page = hit.page;
        // Through the link rather than a held hub: the hub holds this
        // handler, so holding the hub back would be a cycle.
        let editing = self
            .link
            .cx_interaction()
            .is_some_and(|hub| hub.active_tool_enables("annotation-edit"));
        let (x, y) = (hit.point.x, hit.point.y);
        let inside = |l: &Value| {
            let b = l.get("bounds").cloned().unwrap_or(Value::Null);
            let (bx, by) = (number(&b, "x"), number(&b, "y"));
            x >= bx && x <= bx + number(&b, "width") && y >= by && y <= by + number(&b, "height")
        };
        let area = |l: &Value| {
            let b = l.get("bounds").cloned().unwrap_or(Value::Null);
            number(&b, "width") * number(&b, "height")
        };
        let links = self.link.list_links(page);
        let mut best: Option<&Value> = None;
        for link in links.as_array()? {
            let attached = link.get("attached").and_then(Value::as_bool).unwrap_or(false);
            if (editing && attached) || !inside(link) {
                continue;
            }
            if best.is_none_or(|kept| area(link) < area(kept)) {
                best = Some(link);
            }
        }
        best.cloned().map(|link| (page, link))
    }
}

impl InteractionHandler for TapHandler {
    fn id(&self) -> &str {
        "link-tap"
    }

    /// Above the annotation plane: on the web the anchor sits over it and
    /// stops the down before the hub sees it.
    fn priority(&self) -> f64 {
        ANNOTATION_EDIT_PRIORITY + 1.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("link-nav")
    }

    fn on_down(&self, sample: &PointerSample) -> bool {
        let pressed = self.link_under(sample).map(|(page, link)| {
            let id = link.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            (page, id, (sample.viewport.x, sample.viewport.y))
        });
        let captured = pressed.is_some();
        *self.pressed.borrow_mut() = pressed;
        captured
    }

    fn on_up(&self, sample: &PointerSample) {
        let Some((page, id, down)) = self.pressed.borrow_mut().take() else { return };
        let moved = (sample.viewport.x - down.0).hypot(sample.viewport.y - down.1);
        let Some((at, link)) = self.link_under(sample) else { return };
        if at != page || link.get("id").and_then(Value::as_str) != Some(&id) || moved > TAP_SLOP {
            return;
        }
        let context =
            LinkActivateContext { page: Some(page.to_json()), ..LinkActivateContext::default() };
        self.link.activate(&link, Some(&context));
    }

    fn on_cancel(&self, _sample: &PointerSample) {
        self.pressed.borrow_mut().take();
    }
}

struct Bound {
    cap: LinkCapability,
}

#[derive(uniffi::Object)]
pub struct LinkHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl LinkHandle {
    /// On the kernel thread.
    pub fn new(cap: LinkCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(LinkHandle { bound: KernelBound::new(Bound { cap }, post.clone()), post })
    }

    async fn on<T: Send + 'static>(
        &self,
        f: impl FnOnce(&LinkCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.bound.call(move |b| f(open(&b.cap)?)).await
    }
}

fn items_on(page: PageRef, links: &Value) -> Result<Vec<LinkItem>, Error> {
    links.as_array().into_iter().flatten().map(|l| item(page, l)).collect()
}

#[uniffi::export]
impl LinkHandle {
    /// The page's links; empty until the page loads (`ensure_loaded`).
    pub async fn links(&self, page: PageRef) -> Result<Vec<LinkItem>, ViewerError> {
        self.on(move |c| items_on(page, &c.list_links(kernel_page(page)))).await
    }

    pub async fn link(&self, page: PageRef, id: String) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| {
            c.get_link(kernel_page(page), &id).as_ref().map(|l| item(page, l)).transpose()
        })
        .await
    }

    /// The smallest link under a page point.
    pub async fn link_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| {
            c.get_link_at(kernel_page(page), kernel_point(point))
                .as_ref()
                .map(|l| item(page, l))
                .transpose()
        })
        .await
    }

    /// Every link in the document, loading pages as needed. Each item's
    /// page is the one its annotation ref names; a link whose ref names no
    /// page is an `invalid-input` error rather than dropped.
    pub async fn all_links(&self) -> Result<Vec<LinkItem>, ViewerError> {
        self.bound
            .call_promise(
                |b| Ok(open(&b.cap)?.list_all_links(OperationOptions::default())),
                |links: Value| {
                    links
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|l| {
                            let page = page_of_link(l).ok_or_else(|| {
                                invalid(format!("a link whose ref names no page: {l}"))
                            })?;
                            item(page, l)
                        })
                        .collect::<Result<Vec<_>, Error>>()
                },
            )
            .await?
            .map_err(Into::into)
    }

    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.bound
            .call_promise(
                move |b| {
                    Ok(open(&b.cap)?.ensure_loaded(kernel_page(page), OperationOptions::default()))
                },
                |()| (),
            )
            .await
    }

    pub async fn is_loaded(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_loaded(kernel_page(page)))).await
    }

    /// `idle`, `loading`, `ready`, `error` or `forbidden`.
    pub async fn status(&self, page: PageRef) -> Result<String, ViewerError> {
        self.on(move |c| Ok(c.get_status(kernel_page(page)).as_str().to_string())).await
    }

    pub async fn resolve(&self, target_json: String) -> Result<LinkResolution, ViewerError> {
        self.on(move |c| Ok(c.resolve(&json_of(&target_json, "a link target")?).into())).await
    }

    /// Activate a link or a bare target. A `uri`, `named` or `destination`
    /// outcome is the app's to carry out. A link named by its page
    /// activates with that page in its context, as the web's link layer
    /// passes it, so its action tree runs against the link's own page.
    pub async fn activate(&self, input: LinkInput) -> Result<LinkActivation, ViewerError> {
        self.on(move |c| {
            let context = match &input {
                LinkInput::Link { page, .. } => Some(LinkActivateContext {
                    page: Some(page_json(*page)),
                    ..LinkActivateContext::default()
                }),
                LinkInput::Target { .. } => None,
            };
            Ok((&c.activate(&input_of(c, input)?, context.as_ref())).into())
        })
        .await
    }

    /// Activate the link under a page point; `None` when there is none.
    pub async fn activate_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<LinkActivation>, ViewerError> {
        self.on(move |c| {
            Ok(c.activate_at(kernel_page(page), kernel_point(point), None)?
                .as_ref()
                .map(Into::into))
        })
        .await
    }

    /// A human label for a target: the URL, `Go to destination`, ...
    pub async fn label(&self, input: LinkInput) -> Result<String, ViewerError> {
        self.on(move |c| Ok(c.get_label(&input_of(c, input)?))).await
    }

    /// Whether the active tool lets taps follow links (`link-nav`).
    pub async fn is_navigation_engaged(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_navigation_engaged())).await
    }

    /// Register the link tap handler with the document's pointer hub, the
    /// native reader's version of the web's link anchors: a tap on a link
    /// under a `link-nav` tool activates it, and the outcome arrives on
    /// `subscribe` as `Activated` (a `uri` is the app's to open). Every lens
    /// shares it. Unregister through the handle; dropping it keeps the
    /// handler.
    pub async fn register_tap_handler(&self) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let link = open(&b.cap)?.clone();
                let hub = link.cx_interaction().ok_or_else(|| {
                    plugin_error(code::NOT_READY, "link", "no interaction hub", None, None)
                })?;
                let handler = Rc::new(TapHandler { link, pressed: RefCell::new(None) });
                let off = hub.register_handler(handler, RegisterHandlerOptions::default());
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn LinkEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.bound.fire(move |b| {
            let c = &b.cap;
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let activated = {
                        let listener = listener.clone();
                        c.on_activated(move |e| {
                            listener.on_event(LinkEvent::Activated {
                                event: LinkActivatedEvent {
                                    target_json: e.target.to_string(),
                                    activation: (&e.activation).into(),
                                },
                            });
                            Ok(())
                        })
                    };
                    let loaded = {
                        let listener = listener.clone();
                        c.on_loaded(move |e| {
                            if let Some(page) = epdf_kernel::geometry::PageRef::from_json(&e.page) {
                                listener.on_event(LinkEvent::Loaded {
                                    event: LinkLoadedEvent {
                                        page: PageRef {
                                            page_object_number: page.page_object_number,
                                        },
                                    },
                                });
                            }
                            Ok(())
                        })
                    };
                    let offs: Vec<Unsubscribe> = vec![activated, loaded];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
