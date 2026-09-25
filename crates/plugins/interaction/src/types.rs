//! `types.ts`: the pointer protocol every interactive plugin speaks.

use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::{PageRotation, Point};
use epdf_kernel::Token;
use serde::{Deserialize, Serialize};

use crate::capability::InteractionCapability;

pub type ToolId = String;
pub type Cursor = String;
/// `PageObjectNumber`, as the engine contract spells a page.
pub type PageObjectNumber = u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
}

/// The single active arbiter of what the pointer does. A tool carries no
/// behaviour: it turns on capability tags (`enables`) that handlers opt into.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub id: ToolId,
    pub cursor: Cursor,
    /// Cursor over the viewport's page gaps; the neutral arrow when absent.
    pub gap_cursor: Option<Cursor>,
    /// The tags, in insertion order, each once (a `ReadonlySet<string>`).
    pub enables: Vec<String>,
    /// Arming this tool is consent to create with a finger. The hub never
    /// reads it; touch arbitration above the hub does.
    pub touch_direct: Option<bool>,
}

impl Tool {
    /// A tool with these tags; duplicates collapse as a `Set` would.
    pub fn new<'a>(
        id: impl Into<String>,
        cursor: impl Into<String>,
        enables: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut tags: Vec<String> = Vec::new();
        for tag in enables {
            if !tags.iter().any(|t| t == tag) {
                tags.push(tag.to_string());
            }
        }
        Tool {
            id: id.into(),
            cursor: cursor.into(),
            gap_cursor: None,
            enables: tags,
            touch_direct: None,
        }
    }

    pub fn with_gap_cursor(mut self, cursor: impl Into<String>) -> Self {
        self.gap_cursor = Some(cursor.into());
        self
    }

    /// `tool.enables.has(tag)`.
    pub fn enables(&self, tag: &str) -> bool {
        self.enables.iter().any(|t| t == tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// `PointerEvent.pointerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerKind {
    Mouse,
    Pen,
    Touch,
}

/// A sample synthesized from a recognized gesture rather than a raw press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gesture {
    #[serde(rename = "long-press")]
    LongPress,
}

/// The page under the pointer: its object number and the content point
/// (y-down, PDF units), plus the environment the source could report.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageHit {
    pub pon: PageObjectNumber,
    pub point: Point,
    /// View px per content unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Total display rotation (document `/Rotate` plus view rotation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<PageRotation>,
    /// Zoom relative to the page's 100% baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
}

/// `PointerSample.project`: this event on a specific page's content space,
/// unclamped, or `None` when the source cannot project onto that page.
pub type Project = Rc<dyn Fn(PageObjectNumber) -> Option<Point>>;

/// One normalized pointer event.
#[derive(Clone)]
pub struct PointerSample {
    pub phase: Phase,
    /// The source container's px.
    pub viewport: Point,
    /// Present over a page, absent over gaps.
    pub page: Option<PageHit>,
    pub modifiers: Modifiers,
    /// For a down: 1 single, 2 double, 3 triple. Absent reads as 1.
    pub click_count: Option<f64>,
    pub pointer_type: Option<PointerKind>,
    pub gesture: Option<Gesture>,
    /// The lens (stage plugin id) the sample came from.
    pub source: Option<String>,
    pub project: Option<Project>,
}

impl PointerSample {
    /// A sample with every optional member absent.
    pub fn new(phase: Phase, viewport: Point, page: Option<PageHit>) -> Self {
        PointerSample {
            phase,
            viewport,
            page,
            modifiers: Modifiers::default(),
            click_count: None,
            pointer_type: None,
            gesture: None,
            source: None,
            project: None,
        }
    }
}

impl fmt::Debug for PointerSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerSample")
            .field("phase", &self.phase)
            .field("viewport", &self.viewport)
            .field("page", &self.page)
            .field("modifiers", &self.modifiers)
            .field("click_count", &self.click_count)
            .field("pointer_type", &self.pointer_type)
            .field("gesture", &self.gesture)
            .field("source", &self.source)
            .field("project", &self.project.as_ref().map(|_| "fn"))
            .finish()
    }
}

/// A pointer handler contributed by a feature plugin: the one registration
/// mechanism. The hub routes each gesture to the first eligible handler, by
/// priority, whose `on_down` captures it.
///
/// The optional members of the TypeScript interface are default methods
/// whose defaults are what the hub does when the member is absent: nothing
/// for `on_move`, `on_up` and `on_hover`, `on_up` for `on_cancel` (the
/// `onCancel ?? onUp` fallback), and no claim for `claims_touch`.
pub trait InteractionHandler {
    fn id(&self) -> &str;
    /// Higher wins the gesture.
    fn priority(&self) -> f64;
    /// Usually `tool.enables("my-tag")`.
    fn enabled_for(&self, tool: &Tool) -> bool;
    /// Return true to capture: move and up route here until the up.
    fn on_down(&self, sample: &PointerSample) -> bool;
    fn on_move(&self, _sample: &PointerSample) {}
    fn on_up(&self, _sample: &PointerSample) {}
    /// The gesture was aborted, not completed: discard in-flight work.
    fn on_cancel(&self, sample: &PointerSample) {
        self.on_up(sample);
    }
    /// Pointer moved with no active gesture: cursor feedback only.
    fn on_hover(&self, _sample: &PointerSample) {}
    /// Touch pre-flight: claim this contact as a tool gesture. A pure read.
    fn claims_touch(&self, _sample: &PointerSample) -> bool {
        false
    }
}

/// A tool's runtime cursor skin: keyword to restyled cursor.
pub type ToolCursorSkin = BTreeMap<Cursor, Cursor>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub active_tool_id: ToolId,
    pub cursor: Cursor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InteractionAction {
    #[serde(rename = "SET_TOOL", rename_all = "camelCase")]
    SetTool { tool_id: ToolId },
    #[serde(rename = "SET_CURSOR")]
    SetCursor { cursor: Cursor },
}

pub static INTERACTION: LazyLock<Token<InteractionCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "interaction",
        "add interactionPlugin() from '@embedpdf/plugin-interaction' to your plugins list",
    )
});

/// Resolve a sample against a gesture's home page: the source's unclamped
/// projection when it has one, else the page hit when it is that page.
pub fn sample_point_on(sample: &PointerSample, pon: PageObjectNumber) -> Option<Point> {
    sample
        .project
        .as_ref()
        .and_then(|project| project(pon))
        .or_else(|| sample.page.as_ref().filter(|page| page.pon == pon).map(|page| page.point))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionConfig {
    /// Tool active when a document opens. Default `pointer`.
    #[serde(default)]
    pub default_tool: Option<ToolId>,
}
