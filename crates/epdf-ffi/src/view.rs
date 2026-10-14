//! The view vocabulary from `epdf-stage-core` and `epdf-geometry` that more
//! than one plugin's exports use: the camera, the zoom intent, a
//! page-relative anchor, alignment, and the layout enums. The `From` impls
//! live here because neither side is a plugin type.

use epdf_stage_core as sc;

use crate::mirror_enum;

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl From<sc::Camera> for Camera {
    fn from(c: sc::Camera) -> Self {
        Camera { x: c.x, y: c.y, zoom: c.zoom }
    }
}

impl From<Camera> for sc::Camera {
    fn from(c: Camera) -> Self {
        sc::Camera { x: c.x, y: c.y, zoom: c.zoom }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ZoomMode {
    Automatic,
    FitPage,
    FitWidth,
    /// Fit the whole scene.
    FitAll,
}

mirror_enum!(ZoomMode <=> sc::ZoomMode { Automatic, FitPage, FitWidth, FitAll });

/// The zoom intent: a fixed level, a fraction of the page's width or
/// height, or a fit mode.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum ZoomSpec {
    Level { level: f64 },
    PageWidth { page_width: f64 },
    PageHeight { page_height: f64 },
    Mode { mode: ZoomMode },
}

impl From<sc::ZoomSpec> for ZoomSpec {
    fn from(z: sc::ZoomSpec) -> Self {
        match z {
            sc::ZoomSpec::Level { level } => ZoomSpec::Level { level },
            sc::ZoomSpec::PageWidth { page_width } => ZoomSpec::PageWidth { page_width },
            sc::ZoomSpec::PageHeight { page_height } => ZoomSpec::PageHeight { page_height },
            sc::ZoomSpec::Mode { mode } => ZoomSpec::Mode { mode: mode.into() },
        }
    }
}

impl From<ZoomSpec> for sc::ZoomSpec {
    fn from(z: ZoomSpec) -> Self {
        match z {
            ZoomSpec::Level { level } => sc::ZoomSpec::Level { level },
            ZoomSpec::PageWidth { page_width } => sc::ZoomSpec::PageWidth { page_width },
            ZoomSpec::PageHeight { page_height } => sc::ZoomSpec::PageHeight { page_height },
            ZoomSpec::Mode { mode } => sc::ZoomSpec::Mode { mode: mode.into() },
        }
    }
}

/// What I'm looking at: a page and a fraction across and down it.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ViewAnchor {
    pub page_index: u32,
    pub fx: f64,
    pub fy: f64,
}

impl TryFrom<sc::Anchor> for ViewAnchor {
    type Error = epdf_kernel::Error;

    fn try_from(a: sc::Anchor) -> Result<Self, Self::Error> {
        Ok(ViewAnchor {
            page_index: crate::to_u32(a.page_index, "an anchor's page index")?,
            fx: a.fx,
            fy: a.fy,
        })
    }
}

impl From<ViewAnchor> for sc::Anchor {
    fn from(a: ViewAnchor) -> Self {
        sc::Anchor { page_index: a.page_index as usize, fx: a.fx, fy: a.fy }
    }
}

/// Reserved chrome around each page, one thickness per side.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageFrameInsets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl From<epdf_geometry::PageFrame> for PageFrameInsets {
    fn from(f: epdf_geometry::PageFrame) -> Self {
        PageFrameInsets { top: f.top, right: f.right, bottom: f.bottom, left: f.left }
    }
}

impl From<PageFrameInsets> for epdf_geometry::PageFrame {
    fn from(f: PageFrameInsets) -> Self {
        epdf_geometry::PageFrame { top: f.top, right: f.right, bottom: f.bottom, left: f.left }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageAlign {
    Start,
    Center,
    End,
}

mirror_enum!(StageAlign <=> sc::Align { Start, Center, End });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StageAlignment {
    pub x: StageAlign,
    pub y: StageAlign,
}

impl From<sc::Alignment> for StageAlignment {
    fn from(a: sc::Alignment) -> Self {
        StageAlignment { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageAlignment> for sc::Alignment {
    fn from(a: StageAlignment) -> Self {
        sc::Alignment { x: a.x.into(), y: a.y.into() }
    }
}

/// A named stop, or a viewport fraction for the subject's center.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageAlignValue {
    Named { align: StageAlign },
    Fraction { value: f64 },
}

impl From<sc::AlignValue> for StageAlignValue {
    fn from(v: sc::AlignValue) -> Self {
        match v {
            sc::AlignValue::Named(align) => StageAlignValue::Named { align: align.into() },
            sc::AlignValue::Fraction(value) => StageAlignValue::Fraction { value },
        }
    }
}

impl From<StageAlignValue> for sc::AlignValue {
    fn from(v: StageAlignValue) -> Self {
        match v {
            StageAlignValue::Named { align } => sc::AlignValue::Named(align.into()),
            StageAlignValue::Fraction { value } => sc::AlignValue::Fraction(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageAlignmentValue {
    pub x: StageAlignValue,
    pub y: StageAlignValue,
}

impl From<sc::AlignmentValue> for StageAlignmentValue {
    fn from(a: sc::AlignmentValue) -> Self {
        StageAlignmentValue { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageAlignmentValue> for sc::AlignmentValue {
    fn from(a: StageAlignmentValue) -> Self {
        sc::AlignmentValue { x: a.x.into(), y: a.y.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageDirection {
    Ltr,
    Rtl,
}

mirror_enum!(StageDirection <=> sc::Direction { Ltr, Rtl });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSpreadMode {
    None,
    Odd,
    Even,
}

mirror_enum!(StageSpreadMode <=> sc::SpreadMode { None, Odd, Even });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSizingMode {
    Intrinsic,
    Uniform,
}

mirror_enum!(StageSizingMode <=> sc::SizingMode { Intrinsic, Uniform });

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageScrollMetrics {
    pub scroll_left: f64,
    pub scroll_top: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub scrollable_x: bool,
    pub scrollable_y: bool,
}

impl From<sc::ScrollMetrics> for StageScrollMetrics {
    fn from(m: sc::ScrollMetrics) -> Self {
        StageScrollMetrics {
            scroll_left: m.scroll_left,
            scroll_top: m.scroll_top,
            scroll_width: m.scroll_width,
            scroll_height: m.scroll_height,
            client_width: m.client_width,
            client_height: m.client_height,
            scrollable_x: m.scrollable_x,
            scrollable_y: m.scrollable_y,
        }
    }
}
