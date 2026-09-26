//! `contract.ts` and `host-contract.ts`: the public link vocabulary.
//! `Link` is the annotation plugin's `LinkNavItem`; `PdfDestination` and
//! `PdfLinkTarget` are re-exported, as the TypeScript re-exports them.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::ActionDispatchResult;
use epdf_plugin_stage::RevealOptions;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

pub use epdf_annotation_core::{AnnotationRef, PageRef, PdfDestination, PdfLinkTarget};
pub use epdf_plugin_annotation::{HoverFlags, LinkNavItem};

/// A clickable link area: page-space `bounds`, its target, and its
/// annotation (when it is one).
pub type Link = LinkNavItem;

/// `ChangeOrigin`, as JSON.
pub type ChangeOrigin = Value;

/// What activation did, or hands to the host to do (a `Uri` or `Named`
/// outcome is the host's). `Dispatched` carries the action engine's
/// in-flight result. `None` is in the TypeScript union and never produced.
#[derive(Clone)]
pub enum LinkActivation {
    Revealed,
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
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
                json!({ "outcome": outcome, "destination": epdf_kernel::js::to_value(destination) })
            }
            LinkActivation::Uri { uri } => json!({ "outcome": outcome, "uri": uri }),
            LinkActivation::Named { name } => json!({ "outcome": outcome, "name": name }),
            LinkActivation::Reported { target } => {
                json!({ "outcome": outcome, "target": epdf_kernel::js::to_value(target) })
            }
            LinkActivation::Dispatched { dispatch } => {
                json!({ "outcome": outcome, "dispatch": promise(dispatch) })
            }
        }
    }
}

/// What activating a target would do, with no side effect.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkResolution {
    Reveal { page: PageRef, page_index: usize, options: RevealOptions },
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
    Reported { target: PdfLinkTarget },
}

impl LinkResolution {
    pub fn to_json(&self) -> Value {
        use epdf_kernel::js::to_value;
        match self {
            LinkResolution::Reveal { page, page_index, options } => json!({
                "kind": "reveal",
                "page": to_value(page),
                "pageIndex": page_index,
                "options": to_value(options),
            }),
            LinkResolution::Destination { destination } => {
                json!({ "kind": "destination", "destination": to_value(destination) })
            }
            LinkResolution::Uri { uri } => json!({ "kind": "uri", "uri": uri }),
            LinkResolution::Named { name } => json!({ "kind": "named", "name": name }),
            LinkResolution::Reported { target } => {
                json!({ "kind": "reported", "target": to_value(target) })
            }
        }
    }
}

/// A member that is present, `null` included: a spread copies it over.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// `LinkActivateContext`. Each member is `None` when absent and `Some`
/// (possibly `null`) when present, because activation spreads the context
/// over the link's own members and a present `null` wins.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LinkActivateContext {
    /// The link's `/A` tree; the actions plugin runs it instead of the
    /// target.
    #[serde(default, deserialize_with = "present")]
    pub activate: Option<Value>,
    #[serde(default, rename = "ref", deserialize_with = "present")]
    pub ref_: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    pub page: Option<Value>,
}

impl LinkActivateContext {
    /// `{ ...self, ...over }`.
    pub fn spread(&self, over: &LinkActivateContext) -> LinkActivateContext {
        LinkActivateContext {
            activate: over.activate.clone().or_else(|| self.activate.clone()),
            ref_: over.ref_.clone().or_else(|| self.ref_.clone()),
            page: over.page.clone().or_else(|| self.page.clone()),
        }
    }
}

/// What `activate`, `get_label` and `activate_at` take: a bare target or a
/// whole link.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkInput {
    Target(PdfLinkTarget),
    Link(Box<Link>),
}

/// `LinkActivatedEvent`: after the built-in handling of any activation.
#[derive(Clone)]
pub struct LinkActivatedEvent {
    pub target: PdfLinkTarget,
    pub activation: LinkActivation,
    pub origin: ChangeOrigin,
}

/// `LinkLoadedEvent`: a page's links arrived.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LinkLoadedEvent {
    pub page: PageRef,
}

/// The one ambient value the TypeScript reads from the global scope.
#[derive(Clone, Default)]
pub struct LinkHost {
    /// `console.error(message, error)`: receives a throwing event
    /// listener's error. `None` drops it.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `linkPlugin()` takes no config; `host` is the Rust port's way in for
/// what the TypeScript reads from the global scope.
#[derive(Clone, Default)]
pub struct LinkConfig {
    pub host: LinkHost,
}
