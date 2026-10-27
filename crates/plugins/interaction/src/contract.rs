//! `contract.ts` and `host-contract.ts`: the tool vocabulary, the settings,
//! the events, the pointer protocol every interactive plugin speaks, and
//! `samplePointOn`.
//!
//! The TypeScript types one runtime object two ways, `InteractionCapability`
//! for applications and `InteractionHostCapability` for pointer sources and
//! feature plugins. Rust has one type, [`InteractionCapability`], and one
//! tool type, [`Tool`], which is the host's `HostTool`: a public tool plus
//! its behavior tags and gap cursor.

use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::{PageRotation, Point};
use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};

use crate::controller::InteractionCapability;

pub type ToolId = String;

/// A CSS cursor: a keyword such as `crosshair`, or an image cursor.
pub type Cursor = String;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
}

/// The physical device class behind a pointer: `PointerEvent.pointerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerKind {
    Mouse,
    Pen,
    Touch,
}

/// What one finger does on a touch screen while the tool is active. `Draw`:
/// one finger drives the tool, two scroll and zoom. `Tap`: a tap is a click
/// and a drag scrolls. The hub never reads it; touch arbitration above the
/// hub does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolTouch {
    Draw,
    Tap,
}

/// `ToolPointerEvent`: the pointer on a page, as a tool's own pointer
/// methods receive it, in page coordinates. During a gesture the tool took,
/// `page` is the page the gesture started on, and `point` may lie outside
/// that page's box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPointerEvent {
    pub page: PageRef,
    pub point: Point,
    pub modifiers: Modifiers,
    pub pointer_type: PointerKind,
}

/// `onPointerDown`: `true` takes the gesture.
pub type ToolPointerDown = Rc<dyn Fn(&ToolPointerEvent) -> bool>;
/// `onPointerMove`, `onPointerUp` and `onHover`.
pub type ToolPointerFn = Rc<dyn Fn(&ToolPointerEvent)>;

/// `HostTool`: what the pointer does on the page while it is active.
/// `pointer` and `pan` are built in; plugins add their own with
/// [`InteractionCapability::register_tool`].
///
/// A tool turns on behavior tags (`enables`) that handlers opt into, and
/// may handle the pointer itself with its own pointer members, in page
/// coordinates. The hub keeps tools as `Rc<Tool>`; a registration is
/// identified by its `Rc`, as the TypeScript identifies the object.
#[derive(Clone, Default)]
pub struct Tool {
    /// The tool's name, for `activate_tool(id)`.
    pub id: ToolId,
    /// The cursor over the page while the tool is active.
    pub cursor: Cursor,
    /// What one finger does on a touch screen. Absent reads as `Tap`.
    pub touch: Option<ToolTouch>,
    /// The cursor over the space between pages; the plain arrow when absent.
    pub gap_cursor: Option<Cursor>,
    /// The behavior tags, in insertion order, each once (a `ReadonlySet`).
    pub enables: Vec<String>,
    /// A press on a page; `true` takes the gesture, whose moves and release
    /// then come to `on_pointer_move` and `on_pointer_up`.
    pub on_pointer_down: Option<ToolPointerDown>,
    /// The pointer moved during a gesture the tool took.
    pub on_pointer_move: Option<ToolPointerFn>,
    /// The gesture the tool took ended: released or cancelled.
    pub on_pointer_up: Option<ToolPointerFn>,
    /// The pointer moved over a page with no button pressed.
    pub on_hover: Option<ToolPointerFn>,
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
        Tool { id: id.into(), cursor: cursor.into(), enables: tags, ..Tool::default() }
    }

    pub fn with_gap_cursor(mut self, cursor: impl Into<String>) -> Self {
        self.gap_cursor = Some(cursor.into());
        self
    }

    pub fn with_touch(mut self, touch: ToolTouch) -> Self {
        self.touch = Some(touch);
        self
    }

    /// `tool.enables.has(tag)`.
    pub fn enables(&self, tag: &str) -> bool {
        self.enables.iter().any(|t| t == tag)
    }

    /// Whether the tool handles the pointer itself: it brings its own
    /// handler only when it has `on_pointer_down` or `on_hover`.
    pub fn handles_pointer(&self) -> bool {
        self.on_pointer_down.is_some() || self.on_hover.is_some()
    }
}

impl fmt::Debug for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let member = |present: bool| if present { Some("fn") } else { None };
        f.debug_struct("Tool")
            .field("id", &self.id)
            .field("cursor", &self.cursor)
            .field("touch", &self.touch)
            .field("gap_cursor", &self.gap_cursor)
            .field("enables", &self.enables)
            .field("on_pointer_down", &member(self.on_pointer_down.is_some()))
            .field("on_pointer_move", &member(self.on_pointer_move.is_some()))
            .field("on_pointer_up", &member(self.on_pointer_up.is_some()))
            .field("on_hover", &member(self.on_hover.is_some()))
            .finish()
    }
}

/// `ToolCursors`: a tool's cursors replaced by name ("while this tool is
/// active, show cursor X as Y").
pub type ToolCursors = BTreeMap<Cursor, Cursor>;

// ── settings ──────────────────────────────────────────────────────────────────

/// `InteractionSettings`. `interaction_plugin(config)` registers them over
/// [`interaction_defaults`]; `update_settings` changes them for every
/// document while the app runs.
#[derive(Clone, Debug)]
pub struct InteractionSettings {
    /// The tool that's active when a document opens.
    pub default_tool: ToolId,
    /// Tools of your own, registered from the start beside the built-in ones.
    pub tools: Vec<Rc<Tool>>,
}

/// `INTERACTION_DEFAULTS`: `pointer`, and no tools of your own.
pub fn interaction_defaults() -> InteractionSettings {
    InteractionSettings { default_tool: "pointer".to_string(), tools: Vec::new() }
}

/// `InteractionConfig`, the `DeepPartial<InteractionSettings>` that
/// `interaction_plugin(config)` and `update_settings` take: a `None` member
/// leaves that setting as it is; `tools` replaces the list whole.
#[derive(Clone, Debug, Default)]
pub struct InteractionConfig {
    pub default_tool: Option<ToolId>,
    pub tools: Option<Vec<Rc<Tool>>>,
}

/// `SettingsChangedEvent<InteractionSettings>`: the settings after the
/// change, and which top-level settings changed (`defaultTool`, `tools`).
#[derive(Clone, Debug)]
pub struct InteractionSettingsChanged {
    pub settings: InteractionSettings,
    pub changed: Vec<String>,
}

/// `RegisterToolOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisterToolOptions {
    /// Replace a tool registered under the same id instead of refusing.
    pub replace: bool,
}

/// `registerHandler`'s options: `source` scopes a handler to one lens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisterHandlerOptions {
    pub source: Option<String>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `ToolChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolChangedEvent {
    pub tool_id: ToolId,
    pub previous_tool_id: ToolId,
}

/// `GestureStartedEvent`, `GestureEndedEvent` and `GestureCancelledEvent`,
/// which the TypeScript declares as three identical interfaces.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GestureEvent {
    /// The tool that was active when the gesture began.
    pub tool_id: ToolId,
    /// The page the gesture began on, or `None` over the space between pages.
    pub page: Option<PageRef>,
    pub pointer_type: PointerKind,
}

pub type GestureStartedEvent = GestureEvent;
pub type GestureEndedEvent = GestureEvent;
pub type GestureCancelledEvent = GestureEvent;

/// What `onCursorChanged` carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CursorChangedEvent {
    pub cursor: Cursor,
}

// ── the host lens: the pointer protocol ───────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// A sample synthesized from a recognized gesture rather than a raw press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gesture {
    #[serde(rename = "long-press")]
    LongPress,
}

/// `PointerSample.page`: the page under the pointer, its page-space point,
/// and the environment the source could report.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageHit {
    #[serde(rename = "ref")]
    pub page: PageRef,
    pub point: Point,
    /// View px per page unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// The page's total display rotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<PageRotation>,
    /// The page's zoom relative to its 100% baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
}

/// `PointerSample.project`: this event on a specific page's frame,
/// unclamped, or `None` when the source cannot project onto that page.
pub type Project = Rc<dyn Fn(PageRef) -> Option<Point>>;

/// One normalized pointer event. `viewport` is the source container's px.
#[derive(Clone)]
pub struct PointerSample {
    pub phase: Phase,
    pub viewport: Point,
    /// Present over a page, absent over gaps.
    pub page: Option<PageHit>,
    pub modifiers: Modifiers,
    /// For a down: 1 single, 2 double, 3 triple. Absent reads as 1.
    pub click_count: Option<f64>,
    /// Absent when the source can't say; treat as mouse.
    pub pointer_type: Option<PointerKind>,
    pub gesture: Option<Gesture>,
    /// The lens (stage plugin id) the sample came from; absent routes everywhere.
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

/// A pointer handler contributed by a feature plugin, registered through the
/// host lens. The hub routes each gesture to the first eligible handler, by
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
    /// Touch consent, second rung: claim this contact as a tool gesture. A pure read.
    fn claims_touch(&self, _sample: &PointerSample) -> bool {
        false
    }
}

/// The package token. One runtime token serves the public contract and the
/// host lens; in Rust both are [`InteractionCapability`].
pub static INTERACTION: LazyLock<Token<InteractionCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "interaction",
        "add interactionPlugin() from '@embedpdf/plugin-interaction' to your plugins list",
    )
});

/// `samplePointOn`: resolve a sample against a gesture's home page. The
/// source's unclamped projection when it has one, else the page hit when it
/// is that page; `None` when this sample can't speak for the home page.
pub fn sample_point_on(sample: &PointerSample, page: PageRef) -> Option<Point> {
    sample.project.as_ref().and_then(|project| project(page)).or_else(|| {
        sample
            .page
            .as_ref()
            .filter(|hit| hit.page.page_object_number == page.page_object_number)
            .map(|hit| hit.point)
    })
}
