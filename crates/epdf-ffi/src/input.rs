//! A normalized pointer event, as a native source reports it to the
//! interaction hub. `viewport` is the source's own points; `page` is the
//! page under the pointer and its page-space point, absent over a gap.
//! Conversions to the interaction plugin's types live in that crate.

use crate::{PageRef, Point};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerKind {
    Mouse,
    Pen,
    Touch,
}

/// A sample synthesized from a recognized gesture rather than a raw press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerGesture {
    LongPress,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct Modifiers {
    #[uniffi(default = false)]
    pub shift: bool,
    #[uniffi(default = false)]
    pub alt: bool,
    #[uniffi(default = false)]
    pub ctrl: bool,
    #[uniffi(default = false)]
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct PointerPageHit {
    pub page: PageRef,
    /// Page space: y-down PDF units.
    pub point: Point,
    /// View points per page unit.
    #[uniffi(default = None)]
    pub scale: Option<f64>,
    /// The page's total display rotation in degrees.
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
    /// The page's zoom relative to its 100% baseline.
    #[uniffi(default = None)]
    pub zoom: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PointerSample {
    pub phase: PointerPhase,
    pub viewport: Point,
    #[uniffi(default = None)]
    pub page: Option<PointerPageHit>,
    /// No default: a record field can't default to a record. Swift passes
    /// `Modifiers()`, whose fields all default to false.
    pub modifiers: Modifiers,
    /// For a down: 1 single, 2 double, 3 triple. `None` reads as 1.
    #[uniffi(default = None)]
    pub click_count: Option<u32>,
    /// `None` reads as mouse.
    #[uniffi(default = None)]
    pub pointer_type: Option<PointerKind>,
    #[uniffi(default = None)]
    pub gesture: Option<PointerGesture>,
    /// The lens (stage plugin id) the sample came from; `None` routes to
    /// every handler.
    #[uniffi(default = None)]
    pub source: Option<String>,
}
