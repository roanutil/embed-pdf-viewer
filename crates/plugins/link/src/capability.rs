//! `capability.ts`: the navigation plane's surface. The data source is
//! decided per call, so registration order can't matter: the annotation
//! plugin's folded model when it is present (its `link_items_on` already
//! drops hidden links), else this plugin's own engine-backed page cache
//! (see [`crate::source`]).

use std::rc::Rc;

use epdf_kernel::{Ctx, Error};
use epdf_plugin_actions::{ACTIONS, ActionContext, ActionOrigin, ActionSource, ActionTriggerEvent};
use epdf_plugin_annotation::value::truthy;
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stage::{
    PageLayout, RevealOptions, STAGE, ScrollBehaviorKind, destination_to_reveal, without_undefined,
};
use serde_json::Value;

use crate::source::load_links_page;
use crate::types::{
    LinkAction, LinkActivateContext, LinkActivateEvent, LinkActivation, LinkNavItem,
    LinkPluginConfig, LinkState, PageObjectNumber, PdfDestination, PdfLinkTarget,
};

struct Inner {
    ctx: Ctx<LinkState, LinkAction>,
    config: LinkPluginConfig,
}

/// `LinkCapability`. Cheap to clone.
#[derive(Clone)]
pub struct LinkCapability(Rc<Inner>);

/// `createLinkCapability(ctx, config)`.
pub fn create_link_capability(
    ctx: Ctx<LinkState, LinkAction>,
    config: LinkPluginConfig,
) -> LinkCapability {
    LinkCapability(Rc::new(Inner { ctx, config }))
}

/// The destination's page object number, as the page lookup compares it.
fn destination_pon(destination: &PdfDestination) -> Option<f64> {
    epdf_kernel::js::to_value(destination).get("pageObjectNumber").and_then(Value::as_f64)
}

impl LinkCapability {
    fn anno(&self) -> Result<Option<AnnotationCapability>, Error> {
        self.0.ctx.try_get(&ANNOTATION)
    }

    /// The document's layout row for a page object number.
    fn layout_of(&self, pon: Option<f64>) -> Option<PageLayout> {
        let document = self.0.ctx.document()?;
        let pages = document.pages.as_array()?;
        let page =
            pages.iter().find(|p| p.get("pageObjectNumber").and_then(Value::as_f64) == pon)?;
        serde_json::from_value(without_undefined(page)).ok()
    }

    /// The clickable link areas of a page (content space, y-down).
    pub fn links_on(&self, pon: PageObjectNumber) -> Result<Vec<LinkNavItem>, Error> {
        if let Some(host) = self.anno()? {
            return Ok(host.link_items_on(pon));
        }
        Ok(self.0.ctx.state().pages.get(&pon).cloned().unwrap_or_default())
    }

    /// Lazy-load a page's links: the annotation plugin's (a no-op since its
    /// whole-document hydration) when it owns the data, else one engine
    /// read unless the page is cached.
    pub fn ensure_page(&self, pon: PageObjectNumber) -> Result<(), Error> {
        if let Some(host) = self.anno()? {
            host.ensure_page(pon);
            return Ok(());
        }
        if !self.0.ctx.state().pages.contains_key(&pon) {
            load_links_page(&self.0.ctx, pon)?;
        }
        Ok(())
    }

    /// Whether the navigation plane owns links now: the active tool enables
    /// `link-nav`.
    pub fn engaged(&self) -> Result<bool, Error> {
        Ok(self.0.ctx.try_get(&INTERACTION)?.is_some_and(|i| i.active_tool().enables("link-nav")))
    }

    fn handle(
        &self,
        target: &PdfLinkTarget,
        context: Option<&LinkActivateContext>,
    ) -> Result<LinkActivation, Error> {
        // The action engine takes precedence when it is installed and the
        // item carries its payload tree; without it the classic
        // root-projection path below is the fallback.
        let actions = self.0.ctx.try_get(&ACTIONS)?;
        let tree = context.and_then(|c| c.activate.as_ref()).filter(|t| truthy(Some(t)));
        if let (Some(actions), Some(tree), Some(context)) = (actions, tree, context) {
            let dispatch = actions.execute(
                tree.clone(),
                ActionContext {
                    origin: ActionOrigin::User,
                    source: ActionSource::Link {
                        annotation: context.ref_.as_ref().map(epdf_kernel::js::to_value),
                        pon: context.pon.map(f64::from),
                    },
                    event: ActionTriggerEvent::Activate,
                },
            );
            return Ok(LinkActivation::Dispatched { dispatch });
        }
        Ok(match target {
            PdfLinkTarget::Goto { destination } => {
                let stage = self.0.ctx.try_get(&STAGE)?;
                let layout = self.layout_of(destination_pon(destination));
                let (Some(stage), Some(layout)) = (stage, layout) else {
                    // No camera to drive: hand the embedder the destination.
                    return Ok(LinkActivation::Destination { destination: destination.clone() });
                };
                let staged = serde_json::from_value(without_undefined(&epdf_kernel::js::to_value(
                    destination,
                )))
                .map_err(|e| Error::new(format!("[link] destination: {e}")))?;
                let reveal = destination_to_reveal(&staged, &layout, None)
                    .map_err(|e| Error::new(e.to_string()))?;
                stage.reveal(
                    reveal.page_index as isize,
                    Some(RevealOptions {
                        behavior: Some(ScrollBehaviorKind::Smooth),
                        ..reveal.options
                    }),
                );
                LinkActivation::Revealed
            }
            // Opening is the framework layer's job; this plugin has no DOM.
            PdfLinkTarget::Uri { uri } => LinkActivation::Uri { uri: uri.clone() },
            PdfLinkTarget::Named { name } => LinkActivation::Named { name: name.clone() },
            // javascript, goto-remote, launch, unsupported: reported, never
            // executed here.
            other => LinkActivation::Reported { target: other.clone() },
        })
    }

    /// The one activation entry point (framework layers call it on click).
    /// `Err` is a destination whose page cannot be inverted (`userUnit` 0),
    /// or `on_activate` throwing after the activation ran.
    pub fn activate(
        &self,
        target: &PdfLinkTarget,
        context: Option<&LinkActivateContext>,
    ) -> Result<LinkActivation, Error> {
        let activation = self.handle(target, context)?;
        if let Some(on_activate) = &self.0.config.on_activate {
            on_activate(&LinkActivateEvent {
                target: target.clone(),
                activation: activation.clone(),
            })?;
        }
        Ok(activation)
    }
}
