//! `types.ts`: the workbench's surface state, and nothing else. A surface
//! is anything the app shows or hides by name; the plugin stores which are
//! open and the app owns their UI. An `exclusive` tag closes every other
//! open surface carrying the same tag.

use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::ShellCapability;
use crate::record::JsRecord;

/// Opaque props passed at open (which annotation a style panel edits, say).
pub type SurfaceProps = serde_json::Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceState {
    pub open: bool,
    /// The exclusivity tag the surface was opened with, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<SurfaceProps>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    pub surfaces: JsRecord<SurfaceState>,
    /// Open dropdown menus, in opening order (last = topmost).
    pub open_menus: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenSurfaceOptions {
    pub exclusive: Option<String>,
    pub props: Option<SurfaceProps>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShellAction {
    #[serde(rename = "SHELL/OPEN_SURFACE")]
    OpenSurface {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<SurfaceProps>,
    },
    #[serde(rename = "SHELL/CLOSE_SURFACE")]
    CloseSurface { id: String },
    #[serde(rename = "SHELL/TOGGLE_SURFACE")]
    ToggleSurface {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<SurfaceProps>,
    },
    #[serde(rename = "SHELL/OPEN_MENU")]
    OpenMenu { id: String },
    #[serde(rename = "SHELL/CLOSE_MENU")]
    CloseMenu { id: String },
    #[serde(rename = "SHELL/CLOSE_ALL_MENUS")]
    CloseAllMenus,
}

pub static SHELL: LazyLock<Token<ShellCapability>> = LazyLock::new(|| Token::new("shell"));
