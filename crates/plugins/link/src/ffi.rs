//! The link plugin through uniffi: a page's links, what each targets, and
//! activating them, as one handle and one event enum.
//!
//! Targets and destinations are `epdf-ffi`'s annotation-core mirrors. A
//! link's `target` is JSON in the plugin; it decodes here, and a target
//! that does not decode is an `invalid-state` error. A link's action tree
//! stays in Rust: Swift activates a link by page and id, and the handle
//! looks it up again so the tree goes with it. A `goto` resolution carries
//! the stage's reveal options, so this module needs stage's `ffi`.
//!
//! Every method hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, KernelBound, KernelPost, PageRef, PdfDestination, PdfLinkTarget, Point, Rect,
    Subscription, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_stage::ffi::StageRevealOptions;

use crate::{LinkCapability, LinkNavItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LinkHoverFlags {
    pub enter: bool,
    pub exit: bool,
}

/// One clickable link area.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkItem {
    /// Stable per page: `obj:<n>` for an object-numbered annotation.
    pub id: String,
    pub page: PageRef,
    /// Page space, y-down.
    pub bounds: Rect,
    pub target: PdfLinkTarget,
    /// A reply grouped under another annotation.
    pub attached: bool,
    pub annotation: Option<AnnotationRef>,
    /// The link carries an action tree, which runs instead of the target
    /// when the actions plugin is installed.
    pub has_action: bool,
    pub hover: Option<LinkHoverFlags>,
}

fn invalid(what: String) -> Error {
    Error::named("InvalidState", format!("[link] {what}")).with("code", "invalid-state")
}

fn target_of(item: &LinkNavItem) -> Result<epdf_annotation_core::PdfLinkTarget, Error> {
    serde_json::from_value(item.target.clone()).map_err(|e| {
        invalid(format!(
            "link {} has a target that does not decode ({e}): {}",
            item.id, item.target
        ))
    })
}

fn item(page: PageRef, link: &LinkNavItem) -> Result<LinkItem, Error> {
    Ok(LinkItem {
        id: link.id.clone(),
        page,
        bounds: link.bounds.into(),
        target: (&target_of(link)?).into(),
        attached: link.attached,
        annotation: link.ref_.as_ref().map(AnnotationRef::try_from).transpose()?,
        has_action: link.activate.is_some(),
        hover: link.hover_events.map(|h| LinkHoverFlags { enter: h.enter, exit: h.exit }),
    })
}

/// A bare target, or a link named by its page and id.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkInput {
    Target { target: PdfLinkTarget },
    Link { page: PageRef, id: String },
}

/// What activating a target would do, with no side effect.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkResolution {
    Reveal { page: PageRef, page_index: u32, options: StageRevealOptions },
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
    Reported { target: PdfLinkTarget },
}

impl TryFrom<crate::LinkResolution> for LinkResolution {
    type Error = Error;
    fn try_from(r: crate::LinkResolution) -> Result<Self, Error> {
        Ok(match r {
            crate::LinkResolution::Reveal { page, page_index, options } => LinkResolution::Reveal {
                page: page.into(),
                page_index: u32::try_from(page_index)
                    .map_err(|_| invalid(format!("a page index of {page_index}")))?,
                options: options.into(),
            },
            crate::LinkResolution::Destination { destination } => {
                LinkResolution::Destination { destination: (&destination).into() }
            }
            crate::LinkResolution::Uri { uri } => LinkResolution::Uri { uri },
            crate::LinkResolution::Named { name } => LinkResolution::Named { name },
            crate::LinkResolution::Reported { target } => {
                LinkResolution::Reported { target: (&target).into() }
            }
        })
    }
}

/// What an activation did, or hands to the app to do. `Uri` and `Named`
/// are the app's: the plugin never opens a URL. `Dispatched` means the
/// link's action tree went to the actions plugin.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkActivation {
    Revealed,
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
    Reported { target: PdfLinkTarget },
    Dispatched,
    None,
}

impl From<&crate::LinkActivation> for LinkActivation {
    fn from(a: &crate::LinkActivation) -> Self {
        match a {
            crate::LinkActivation::Revealed => LinkActivation::Revealed,
            crate::LinkActivation::Destination { destination } => {
                LinkActivation::Destination { destination: destination.into() }
            }
            crate::LinkActivation::Uri { uri } => LinkActivation::Uri { uri: uri.clone() },
            crate::LinkActivation::Named { name } => LinkActivation::Named { name: name.clone() },
            crate::LinkActivation::Reported { target } => {
                LinkActivation::Reported { target: target.into() }
            }
            crate::LinkActivation::Dispatched { .. } => LinkActivation::Dispatched,
            crate::LinkActivation::None => LinkActivation::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkActivatedEvent {
    pub target: PdfLinkTarget,
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

/// The plugin's input for a foreign one: a link is looked up again so its
/// action tree goes with it.
fn input_of(cap: &LinkCapability, input: LinkInput) -> Result<crate::LinkInput, Error> {
    Ok(match input {
        LinkInput::Target { target } => crate::LinkInput::Target(target.into()),
        LinkInput::Link { page, id } => {
            let link = cap.get_link(&page.into(), &id).ok_or_else(|| {
                plugin_error(
                    code::NOT_FOUND,
                    "link",
                    format!("no link {id} on that page"),
                    None,
                    None,
                )
            })?;
            crate::LinkInput::Link(Box::new(link))
        }
    })
}

#[derive(uniffi::Object)]
pub struct LinkHandle {
    cap: KernelBound<LinkCapability>,
    post: KernelPost,
}

impl LinkHandle {
    /// On the kernel thread.
    pub fn new(cap: LinkCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(LinkHandle { cap: KernelBound::new(cap, post.clone()), post })
    }

    async fn on<T: Send + 'static>(
        &self,
        f: impl FnOnce(&LinkCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.cap.call(move |c| f(open(c)?)).await
    }
}

#[uniffi::export]
impl LinkHandle {
    /// The page's links; empty until the page loads (`ensure_loaded`).
    pub async fn links(&self, page: PageRef) -> Result<Vec<LinkItem>, ViewerError> {
        self.on(move |c| c.list_links(&page.into()).iter().map(|l| item(page, l)).collect()).await
    }

    pub async fn link(&self, page: PageRef, id: String) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| c.get_link(&page.into(), &id).as_ref().map(|l| item(page, l)).transpose())
            .await
    }

    /// The smallest link under a page point.
    pub async fn link_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| {
            c.get_link_at(&page.into(), point.into()).as_ref().map(|l| item(page, l)).transpose()
        })
        .await
    }

    /// Every link in the document, loading pages as needed. The plugin's
    /// items don't carry their page, so once every page has loaded this
    /// walks the pages itself.
    pub async fn all_links(&self) -> Result<Vec<LinkItem>, ViewerError> {
        self.cap.call_promise(|c| Ok(open(c)?.list_all_links()), |_| ()).await?;
        self.on(|c| {
            let mut out = Vec::new();
            for page in c.page_refs() {
                let ffi_page: PageRef = page.into();
                for l in c.list_links(&page) {
                    out.push(item(ffi_page, &l)?);
                }
            }
            Ok(out)
        })
        .await
    }

    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap.call_promise(move |c| Ok(open(c)?.ensure_loaded(&page.into())), |()| ()).await
    }

    pub async fn is_loaded(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_loaded(&page.into()))).await
    }

    pub async fn resolve(&self, target: PdfLinkTarget) -> Result<LinkResolution, ViewerError> {
        self.on(move |c| c.resolve(&target.into())?.try_into()).await
    }

    /// Activate a link or a bare target. A `uri` or `named` outcome is the
    /// app's to carry out.
    pub async fn activate(&self, input: LinkInput) -> Result<LinkActivation, ViewerError> {
        self.on(move |c| Ok((&c.activate(&input_of(c, input)?, None)?).into())).await
    }

    /// Activate the link under a page point; `None` when there is none.
    pub async fn activate_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<LinkActivation>, ViewerError> {
        self.on(move |c| {
            Ok(c.activate_at(&page.into(), point.into(), None)?.as_ref().map(Into::into))
        })
        .await
    }

    /// A human label for a target: the URL, `Go to destination`, ...
    pub async fn label(&self, input: LinkInput) -> Result<String, ViewerError> {
        self.on(move |c| {
            let target = match input_of(c, input)? {
                crate::LinkInput::Target(target) => epdf_kernel::js::to_value(&target),
                crate::LinkInput::Link(link) => link.target.clone(),
            };
            Ok(c.get_label(&target))
        })
        .await
    }

    /// Whether the active tool lets taps follow links (`link-nav`).
    pub async fn is_navigation_engaged(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_navigation_engaged())).await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn LinkEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let activated = {
                        let listener = listener.clone();
                        c.on_activated(move |e| {
                            listener.on_event(LinkEvent::Activated {
                                event: LinkActivatedEvent {
                                    target: (&e.target).into(),
                                    activation: (&e.activation).into(),
                                },
                            });
                            Ok(())
                        })
                    };
                    let loaded = {
                        let listener = listener.clone();
                        c.on_loaded(move |e| {
                            listener.on_event(LinkEvent::Loaded {
                                event: LinkLoadedEvent { page: e.page.into() },
                            });
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
