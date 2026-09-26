//! `contract.ts`: the workbench's surface state, and nothing else. A surface
//! is anything the app shows or hides by name; the plugin stores which are
//! open and the app owns their UI. An `exclusive` tag closes every other
//! open surface carrying the same tag.

use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::controller::ShellCapability;
use crate::model::SurfaceRecord;
use crate::record::JsRecord;

/// Opaque props passed at open (which annotation a style panel edits, say).
pub type SurfaceProps = serde_json::Map<String, Value>;

/// `SurfaceState`: a surface record with its id.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SurfaceState {
    pub id: String,
    /// `None` only for an id naming an `Object.prototype` member the
    /// surface records do not own: the TypeScript's lookup finds the
    /// inherited function, and its view is `{id}` alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// The exclusivity tag the surface was opened with, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<String>,
    /// Opaque props passed at open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<SurfaceProps>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct OpenSurfaceOptions {
    #[serde(default)]
    pub exclusive: Option<String>,
    #[serde(default)]
    pub props: Option<SurfaceProps>,
}

/// `ShellSnapshot`: everything the shell holds, serializable for persist
/// and restore. It is the state itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    pub surfaces: JsRecord<SurfaceRecord>,
    /// Open dropdown menus, in opening order (last = topmost).
    pub open_menus: Vec<String>,
}

/// `SurfaceEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SurfaceEvent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<SurfaceProps>,
}

/// `MenuEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MenuEvent {
    pub id: String,
}

pub static SHELL: LazyLock<Token<ShellCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "shell",
        "add shellPlugin() from '@embedpdf/plugin-shell' to your plugins list",
    )
});
