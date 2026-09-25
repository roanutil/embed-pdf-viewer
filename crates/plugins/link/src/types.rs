//! `types.ts`: the slice, its actions, the activation outcome and context,
//! the config, and the token. `LinkNavItem`, `PdfDestination` and
//! `PdfLinkTarget` are re-exported, as the TypeScript re-exports them.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use epdf_plugin_actions::{ActionDispatchResult, PdfActionTree};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use epdf_annotation_core::{AnnotationRef, PageObjectNumber, PdfDestination, PdfLinkTarget};
pub use epdf_plugin_annotation::{HoverFlags, LinkNavItem};

use crate::capability::LinkCapability;

/// What one activation did.
///
/// `Revealed`: the stage moved the camera. `Destination`: no stage (or no
/// such page), so the embedder gets the explicit destination. `Uri`:
/// opening is the framework layer's job. `Named`: a viewer verb, reported.
/// `Reported`: javascript, goto-remote, launch and unsupported targets,
/// surfaced and never executed here. `Dispatched`: the action engine took
/// the activation, and `dispatch` is its in-flight result; the framework
/// opener must do nothing for it. `None` is in the TypeScript union and
/// never produced.
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

/// Optional activation context: the payload-carrying `/A` tree and its
/// annotation identity from the `LinkNavItem`, which let the action engine
/// take the activation.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LinkActivateContext {
    #[serde(default)]
    pub activate: Option<PdfActionTree>,
    #[serde(default, rename = "ref")]
    pub ref_: Option<AnnotationRef>,
    #[serde(default)]
    pub pon: Option<PageObjectNumber>,
}

/// What `onActivate` receives.
#[derive(Clone)]
pub struct LinkActivateEvent {
    pub target: PdfLinkTarget,
    pub activation: LinkActivation,
}

/// An `onActivate` listener. An `Err` is the listener throwing, which
/// escapes `activate` after the built-in handling ran.
pub type ActivateListener = Rc<dyn Fn(&LinkActivateEvent) -> Result<(), Error>>;

#[derive(Clone, Default)]
pub struct LinkPluginConfig {
    /// Every activation, navigations and reported targets alike. Called
    /// after the built-in handling ran.
    pub on_activate: Option<ActivateListener>,
}

/// Engine-backed page cache (viewer-only deployments), keyed by page
/// object number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkState {
    #[serde(deserialize_with = "pages_by_key")]
    pub pages: BTreeMap<PageObjectNumber, Vec<LinkNavItem>>,
}

/// Object keys are strings on the wire; the cache only ever holds page
/// object numbers.
fn pages_by_key<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PageObjectNumber, Vec<LinkNavItem>>, D::Error> {
    BTreeMap::<String, Vec<LinkNavItem>>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, items)| {
            key.parse().map(|pon| (pon, items)).map_err(|_| {
                serde::de::Error::custom(format!("page key {key:?} is not a page object number"))
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LinkAction {
    #[serde(rename = "SET_PAGE")]
    SetPage { pon: PageObjectNumber, items: Vec<LinkNavItem> },
    #[serde(rename = "DROP_PAGE")]
    DropPage { pon: PageObjectNumber },
}

pub static LINK: LazyLock<Token<LinkCapability>> = LazyLock::new(|| Token::new("link"));
