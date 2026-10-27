//! `contract.ts` and `host-contract.ts`: the public link vocabulary, and
//! `resolveLinkTarget`, the rule every link and outline item follows.
//!
//! A link, its target, its destination and its `/A` tree are the engine's
//! records as JSON (`Value`), as the annotation plugin carries them since
//! Task 13.17: the annotation plugin's `listLinkItems` builds the same
//! `Link` this plugin's stand-alone source does.

use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::ActionDispatchResult;
use epdf_plugin_stage::StageCapability;
use serde_json::{Value, json};

pub use epdf_kernel::geometry::PageRef;
pub use epdf_kernel::mirror::ResourceStatus;
pub use epdf_plugin_stage::PageTarget;

/// `Link` (`LinkNavItem`): `{id, bounds, target, activate?, hoverEvents?,
/// ref?, attached}`, page space.
pub type Link = Value;
/// `PdfLinkTarget`: `{kind: 'goto' | 'uri' | 'named' | ..., ...}`.
pub type PdfLinkTarget = Value;
/// `PageDestination`: a destination in page space.
pub type PageDestination = Value;
/// `PdfActionTree`.
pub type PdfActionTree = Value;
/// `AnnotationRef`.
pub type AnnotationRef = Value;

/// `OperationOptions`: what the loading calls take.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// What activation did. `Destination`, `Named` and a `Uri` with no
/// framework to open it are handed to the caller to perform. `None` is in
/// the TypeScript union and never produced.
#[derive(Clone)]
pub enum LinkActivation {
    Revealed,
    Destination { destination: PageDestination },
    Uri { uri: Value },
    Named { name: Value },
    Reported { target: PdfLinkTarget },
    Dispatched { dispatch: Promise<ActionDispatchResult> },
    None,
}

impl LinkActivation {
    /// The `outcome` tag.
    pub fn outcome(&self) -> &'static str {
        match self {
            LinkActivation::Revealed => "revealed",
            LinkActivation::Destination { .. } => "destination",
            LinkActivation::Uri { .. } => "uri",
            LinkActivation::Named { .. } => "named",
            LinkActivation::Reported { .. } => "reported",
            LinkActivation::Dispatched { .. } => "dispatched",
            LinkActivation::None => "none",
        }
    }

    /// The object the TypeScript returns, with the dispatch promise spelled
    /// by `promise`.
    pub fn to_json_with(
        &self,
        promise: impl FnOnce(&Promise<ActionDispatchResult>) -> Value,
    ) -> Value {
        let outcome = self.outcome();
        match self {
            LinkActivation::Revealed | LinkActivation::None => json!({ "outcome": outcome }),
            LinkActivation::Destination { destination } => {
                json!({ "outcome": outcome, "destination": destination })
            }
            LinkActivation::Uri { uri } => json!({ "outcome": outcome, "uri": uri }),
            LinkActivation::Named { name } => json!({ "outcome": outcome, "name": name }),
            LinkActivation::Reported { target } => json!({ "outcome": outcome, "target": target }),
            LinkActivation::Dispatched { dispatch } => {
                json!({ "outcome": outcome, "dispatch": promise(dispatch) })
            }
        }
    }
}

/// What activating a target would do, with no side effect. A `goto` to a
/// page of this document is its destination, ready for
/// `stage.goToDestination`; one to a page the document doesn't have is
/// reported.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkResolution {
    Destination { destination: PageDestination },
    Uri { uri: Value },
    Named { name: Value },
    Reported { target: PdfLinkTarget },
}

impl LinkResolution {
    pub fn to_json(&self) -> Value {
        match self {
            LinkResolution::Destination { destination } => {
                json!({ "kind": "destination", "destination": destination })
            }
            LinkResolution::Uri { uri } => json!({ "kind": "uri", "uri": uri }),
            LinkResolution::Named { name } => json!({ "kind": "named", "name": name }),
            LinkResolution::Reported { target } => json!({ "kind": "reported", "target": target }),
        }
    }
}

/// `target.member`: absent reads as `undefined`.
fn member(target: &Value, key: &str) -> Value {
    target.get(key).cloned().unwrap_or_else(epdf_kernel::undefined)
}

/// `resolveLinkTarget(target, hasPage)`: a `goto` to a page `has_page`
/// knows is its destination, one to a page the document doesn't have is
/// reported; a website and a named verb are handed on; anything else is
/// reported. `has_page` gets the destination's `page` as the target holds
/// it (`undefined` when it has none).
pub fn resolve_link_target(
    target: &PdfLinkTarget,
    has_page: impl FnOnce(&Value) -> bool,
) -> LinkResolution {
    match target.get("kind").and_then(Value::as_str) {
        Some("goto") => {
            let destination = member(target, "destination");
            if has_page(&member(&destination, "page")) {
                LinkResolution::Destination { destination }
            } else {
                LinkResolution::Reported { target: target.clone() }
            }
        }
        Some("uri") => LinkResolution::Uri { uri: member(target, "uri") },
        Some("named") => LinkResolution::Named { name: member(target, "name") },
        _ => LinkResolution::Reported { target: target.clone() },
    }
}

/// `LinkActivateContext`. Each member is `None` when absent and `Some`
/// (possibly `null` or `undefined`) when present, because activation
/// spreads the context over the link's own members and a present member
/// wins whatever its value.
#[derive(Clone, Default)]
pub struct LinkActivateContext {
    /// The link's `/A` tree when it has one; the actions plugin runs it
    /// instead of the target.
    pub activate: Option<Value>,
    pub ref_: Option<Value>,
    pub page: Option<Value>,
    /// The view the link was followed in: a page destination moves this
    /// Stage. Defaults to the document's own stage.
    pub stage: Option<StageCapability>,
}

impl LinkActivateContext {
    /// `{ ...self, ...over }`.
    pub fn spread(&self, over: &LinkActivateContext) -> LinkActivateContext {
        LinkActivateContext {
            activate: over.activate.clone().or_else(|| self.activate.clone()),
            ref_: over.ref_.clone().or_else(|| self.ref_.clone()),
            page: over.page.clone().or_else(|| self.page.clone()),
            stage: over.stage.clone().or_else(|| self.stage.clone()),
        }
    }

    /// The JSON members of a context object, as a spread copies them.
    pub fn from_json(value: &Value) -> LinkActivateContext {
        LinkActivateContext {
            activate: value.get("activate").cloned(),
            ref_: value.get("ref").cloned(),
            page: value.get("page").cloned(),
            stage: None,
        }
    }
}

/// `isLink`: `'target' in value && 'bounds' in value`.
pub fn is_link(value: &Value) -> bool {
    value.get("target").is_some() && value.get("bounds").is_some()
}

/// `LinkActivatedEvent`: a link target was activated through this
/// capability.
#[derive(Clone)]
pub struct LinkActivatedEvent {
    pub target: PdfLinkTarget,
    pub activation: LinkActivation,
}

/// `LinkLoadedEvent`: a page's links were read from the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkLoadedEvent {
    pub page: Value,
}

impl LinkLoadedEvent {
    pub fn to_json(&self) -> Value {
        json!({ "page": self.page })
    }
}

/// `UriOpener`: opens a website for `activate()`, in the user's gesture.
/// Returns whether it opened: false for an address it won't open (a
/// `javascript:` or `file:` URI), which activation then reports.
pub type UriOpener = Rc<dyn Fn(&Value) -> bool>;
