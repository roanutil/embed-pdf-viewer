//! `contract.ts`: the workbench's surface state, and nothing else. A surface
//! is anything the app shows or hides by name; the plugin stores which are
//! open and the app owns their UI. An `exclusive` tag closes every other
//! open surface carrying the same tag.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde::{Deserialize, Serialize as DeriveSerialize};
use serde_json::Value;

use crate::controller::ShellCapability;
use crate::model::{SnapshotSurface, SurfaceRecord};
use crate::record::JsRecord;

/// Opaque props passed at open (which annotation a style panel edits, say).
pub type SurfaceProps = serde_json::Map<String, Value>;

/// `SurfaceState`: `{ id, ...record }`, the record's members in its own
/// order.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceState {
    pub id: String,
    /// `None` only for an id naming an `Object.prototype` member the
    /// surface records do not own: the TypeScript's lookup finds the
    /// inherited function, and its spread adds nothing to `{ id }`.
    pub record: Option<Rc<SurfaceRecord>>,
}

impl SurfaceState {
    /// `open`; `false` for an inherited name.
    pub fn open(&self) -> bool {
        self.record.as_ref().is_some_and(|record| record.open)
    }

    /// The exclusivity tag the surface was opened with, if any.
    pub fn exclusive(&self) -> Option<&str> {
        self.record.as_ref()?.exclusive.as_deref()
    }

    /// Opaque props passed at open.
    pub fn props(&self) -> Option<&SurfaceProps> {
        self.record.as_ref()?.props.as_ref()
    }

    /// How many opens asked for focus since the surface opened: 0 when none
    /// did, and again once it closes.
    pub fn focus_request(&self) -> f64 {
        self.record.as_ref().map_or(0.0, |record| record.focus_request)
    }
}

impl Serialize for SurfaceState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        if let Some(record) = &self.record {
            record.serialize_into(&mut map)?;
        }
        map.end()
    }
}

/// `OpenSurfaceOptions`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct OpenSurfaceOptions {
    #[serde(default)]
    pub exclusive: Option<String>,
    #[serde(default)]
    pub props: Option<SurfaceProps>,
    /// Ask for the cursor: raises the surface's `focusRequest` by one, also
    /// when it is open already.
    #[serde(default)]
    pub focus: bool,
}

/// `ShellSnapshot`: everything the shell holds but the focus requests,
/// serializable for persist and restore.
#[derive(Clone, Debug, Default, PartialEq, DeriveSerialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    pub surfaces: JsRecord<SnapshotSurface>,
    /// Open dropdown menus, in opening order (last = topmost).
    /// `snapshotOf` shares the state's array, as the TypeScript does.
    pub open_menus: Rc<Vec<String>>,
}

/// `SurfaceOpenedEvent`.
#[derive(Clone, Debug, PartialEq, DeriveSerialize)]
pub struct SurfaceOpenedEvent {
    pub id: String,
    /// The props it was opened with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<SurfaceProps>,
}

/// `SurfaceClosedEvent`.
#[derive(Clone, Debug, PartialEq, DeriveSerialize)]
pub struct SurfaceClosedEvent {
    pub id: String,
}

/// `MenuOpenedEvent`.
#[derive(Clone, Debug, PartialEq, DeriveSerialize)]
pub struct MenuOpenedEvent {
    pub id: String,
}

/// `MenuClosedEvent`.
#[derive(Clone, Debug, PartialEq, DeriveSerialize)]
pub struct MenuClosedEvent {
    pub id: String,
}

pub static SHELL: LazyLock<Token<ShellCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "shell",
        "add shellPlugin() from '@embedpdf/plugin-shell' to your plugins list",
    )
});
