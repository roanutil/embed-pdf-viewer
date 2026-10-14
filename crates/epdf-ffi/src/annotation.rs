//! Records from `epdf-annotation-core` that several plugins' exports
//! return: where a link points, an explicit destination, and how the engine
//! addresses an annotation. The `From` impls live here because neither side
//! is a plugin type.
//!
//! A destination's coordinates are `Nullable` in the core (absent, `null`,
//! or a number). Absent and `null` both mean "keep the current value" to
//! every consumer, so both arrive as `None`, and `None` goes back as absent.

use epdf_annotation_core as ac;
use epdf_annotation_core as core;
use epdf_annotation_core::js::Nullable;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};

use crate::PageRef;
use crate::geometry::Point;

impl From<ac::PageRef> for PageRef {
    fn from(p: ac::PageRef) -> Self {
        PageRef { page_object_number: p.page_object_number }
    }
}

impl From<PageRef> for ac::PageRef {
    fn from(p: PageRef) -> Self {
        ac::PageRef::new(p.page_object_number)
    }
}

fn opt(n: &Nullable<f64>) -> Option<f64> {
    match n {
        Nullable::Value(v) => Some(*v),
        Nullable::Null | Nullable::Undefined => None,
    }
}

fn nullable(n: Option<f64>) -> Nullable<f64> {
    n.map_or(Nullable::Undefined, Nullable::Value)
}

/// An explicit in-document destination: a page and how to show it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfDestination {
    Xyz { page: PageRef, left: Option<f64>, top: Option<f64>, zoom: Option<f64> },
    Fit { page: PageRef },
    FitH { page: PageRef, top: Option<f64> },
    FitV { page: PageRef, left: Option<f64> },
    FitR { page: PageRef, left: f64, bottom: f64, right: f64, top: f64 },
    FitB { page: PageRef },
    FitBH { page: PageRef, top: Option<f64> },
    FitBV { page: PageRef, left: Option<f64> },
}

impl From<&ac::PdfDestination> for PdfDestination {
    fn from(d: &ac::PdfDestination) -> Self {
        use ac::PdfDestination as D;
        match d {
            D::Xyz { page, left, top, zoom } => PdfDestination::Xyz {
                page: (*page).into(),
                left: opt(left),
                top: opt(top),
                zoom: opt(zoom),
            },
            D::Fit { page } => PdfDestination::Fit { page: (*page).into() },
            D::FitH { page, top } => PdfDestination::FitH { page: (*page).into(), top: opt(top) },
            D::FitV { page, left } => {
                PdfDestination::FitV { page: (*page).into(), left: opt(left) }
            }
            D::FitR { page, left, bottom, right, top } => PdfDestination::FitR {
                page: (*page).into(),
                left: *left,
                bottom: *bottom,
                right: *right,
                top: *top,
            },
            D::FitB { page } => PdfDestination::FitB { page: (*page).into() },
            D::FitBH { page, top } => PdfDestination::FitBH { page: (*page).into(), top: opt(top) },
            D::FitBV { page, left } => {
                PdfDestination::FitBV { page: (*page).into(), left: opt(left) }
            }
        }
    }
}

impl From<PdfDestination> for ac::PdfDestination {
    fn from(d: PdfDestination) -> Self {
        use ac::PdfDestination as D;
        match d {
            PdfDestination::Xyz { page, left, top, zoom } => D::Xyz {
                page: page.into(),
                left: nullable(left),
                top: nullable(top),
                zoom: nullable(zoom),
            },
            PdfDestination::Fit { page } => D::Fit { page: page.into() },
            PdfDestination::FitH { page, top } => D::FitH { page: page.into(), top: nullable(top) },
            PdfDestination::FitV { page, left } => {
                D::FitV { page: page.into(), left: nullable(left) }
            }
            PdfDestination::FitR { page, left, bottom, right, top } => {
                D::FitR { page: page.into(), left, bottom, right, top }
            }
            PdfDestination::FitB { page } => D::FitB { page: page.into() },
            PdfDestination::FitBH { page, top } => {
                D::FitBH { page: page.into(), top: nullable(top) }
            }
            PdfDestination::FitBV { page, left } => {
                D::FitBV { page: page.into(), left: nullable(left) }
            }
        }
    }
}

/// Where a link points, normalized.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfLinkTarget {
    Goto { destination: PdfDestination },
    Uri { uri: String },
    GotoRemote { file: String },
    Launch { path: String },
    Javascript,
    Named { name: String },
    Unsupported,
}

impl From<&ac::PdfLinkTarget> for PdfLinkTarget {
    fn from(t: &ac::PdfLinkTarget) -> Self {
        use ac::PdfLinkTarget as T;
        match t {
            T::Goto { destination } => PdfLinkTarget::Goto { destination: destination.into() },
            T::Uri { uri } => PdfLinkTarget::Uri { uri: uri.clone() },
            T::GotoRemote { file } => PdfLinkTarget::GotoRemote { file: file.clone() },
            T::Launch { path } => PdfLinkTarget::Launch { path: path.clone() },
            T::Javascript => PdfLinkTarget::Javascript,
            T::Named { name } => PdfLinkTarget::Named { name: name.clone() },
            T::Unsupported => PdfLinkTarget::Unsupported,
        }
    }
}

impl From<PdfLinkTarget> for ac::PdfLinkTarget {
    fn from(t: PdfLinkTarget) -> Self {
        use ac::PdfLinkTarget as T;
        match t {
            PdfLinkTarget::Goto { destination } => T::Goto { destination: destination.into() },
            PdfLinkTarget::Uri { uri } => T::Uri { uri },
            PdfLinkTarget::GotoRemote { file } => T::GotoRemote { file },
            PdfLinkTarget::Launch { path } => T::Launch { path },
            PdfLinkTarget::Javascript => T::Javascript,
            PdfLinkTarget::Named { name } => T::Named { name },
            PdfLinkTarget::Unsupported => T::Unsupported,
        }
    }
}

/// The page generation an index-addressed annotation was read at.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationRevisionToken {
    pub doc_session_id: String,
    pub page: PageRef,
    pub generation: f64,
}

/// How the engine addresses an annotation.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationRef {
    ObjectNumber { page: PageRef, annot_object_number: u32 },
    Nm { page: PageRef, nm: String },
    Index { page: PageRef, index: u32, revision: AnnotationRevisionToken },
}

fn whole(n: f64, what: &str) -> Result<u32, epdf_kernel::Error> {
    if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) {
        Ok(n as u32)
    } else {
        Err(epdf_kernel::Error::named("InvalidState", format!("{what} {n} is not a whole u32"))
            .with("code", "invalid-state"))
    }
}

impl TryFrom<&ac::AnnotationRef> for AnnotationRef {
    type Error = epdf_kernel::Error;

    fn try_from(r: &ac::AnnotationRef) -> Result<Self, Self::Error> {
        Ok(match r {
            ac::AnnotationRef::ObjectNumber { page, annot_object_number } => {
                AnnotationRef::ObjectNumber {
                    page: (*page).into(),
                    annot_object_number: whole(
                        *annot_object_number,
                        "an annotation object number",
                    )?,
                }
            }
            ac::AnnotationRef::Nm { page, nm } => {
                AnnotationRef::Nm { page: (*page).into(), nm: nm.clone() }
            }
            ac::AnnotationRef::Index { page, index, revision } => AnnotationRef::Index {
                page: (*page).into(),
                index: whole(*index, "an annotation index")?,
                revision: AnnotationRevisionToken {
                    doc_session_id: revision.doc_session_id.clone(),
                    page: revision.page.into(),
                    generation: revision.generation,
                },
            },
        })
    }
}

impl From<AnnotationRef> for ac::AnnotationRef {
    fn from(r: AnnotationRef) -> Self {
        match r {
            AnnotationRef::ObjectNumber { page, annot_object_number } => {
                ac::AnnotationRef::ObjectNumber {
                    page: page.into(),
                    annot_object_number: f64::from(annot_object_number),
                }
            }
            AnnotationRef::Nm { page, nm } => ac::AnnotationRef::Nm { page: page.into(), nm },
            AnnotationRef::Index { page, index, revision } => ac::AnnotationRef::Index {
                page: page.into(),
                index: f64::from(index),
                revision: ac::RevisionToken {
                    doc_session_id: revision.doc_session_id,
                    page: revision.page.into(),
                    generation: revision.generation,
                },
            },
        }
    }
}

// By value as well as by reference: plan 05's code converts owned values.

impl From<ac::PdfDestination> for PdfDestination {
    fn from(d: ac::PdfDestination) -> Self {
        PdfDestination::from(&d)
    }
}

impl From<ac::PdfLinkTarget> for PdfLinkTarget {
    fn from(t: ac::PdfLinkTarget) -> Self {
        PdfLinkTarget::from(&t)
    }
}

impl TryFrom<ac::AnnotationRef> for AnnotationRef {
    type Error = epdf_kernel::Error;

    fn try_from(r: ac::AnnotationRef) -> Result<Self, Self::Error> {
        AnnotationRef::try_from(&r)
    }
}

impl From<ac::RevisionToken> for AnnotationRevisionToken {
    fn from(t: ac::RevisionToken) -> Self {
        AnnotationRevisionToken {
            doc_session_id: t.doc_session_id,
            page: t.page.into(),
            generation: t.generation,
        }
    }
}

impl From<AnnotationRevisionToken> for ac::RevisionToken {
    fn from(t: AnnotationRevisionToken) -> Self {
        ac::RevisionToken {
            doc_session_id: t.doc_session_id,
            page: t.page.into(),
            generation: t.generation,
        }
    }
}

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message, None, None)
}

// `AnnotationRef`, `AnnotationRevisionToken`, `PdfDestination` and
// `PdfLinkTarget`, with their conversions both ways, are plan 04's, earlier
// in this file. This block adds the rest.

impl AnnotationRef {
    pub fn page(&self) -> PageRef {
        match self {
            AnnotationRef::ObjectNumber { page, .. }
            | AnnotationRef::Nm { page, .. }
            | AnnotationRef::Index { page, .. } => *page,
        }
    }
}

/// The `/F` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationFlags {
    pub invisible: bool,
    pub hidden: bool,
    pub print: bool,
    pub no_zoom: bool,
    pub no_rotate: bool,
    pub no_view: bool,
    pub read_only: bool,
    pub locked: bool,
    pub toggle_no_view: bool,
    pub locked_contents: bool,
}

macro_rules! flags_both_ways {
    ($ffi:ident, $core:path, $($field:ident),*) => {
        impl From<$core> for $ffi {
            fn from(f: $core) -> Self { $ffi { $($field: f.$field),* } }
        }
        impl From<$ffi> for $core {
            fn from(f: $ffi) -> Self { Self { $($field: f.$field),* } }
        }
    };
}

flags_both_ways!(
    AnnotationFlags,
    core::AnnotationFlags,
    invisible,
    hidden,
    print,
    no_zoom,
    no_rotate,
    no_view,
    read_only,
    locked,
    toggle_no_view,
    locked_contents
);

/// A partial flags write. `None` leaves the flag alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationFlagsPatch {
    #[uniffi(default = None)]
    pub invisible: Option<bool>,
    #[uniffi(default = None)]
    pub hidden: Option<bool>,
    #[uniffi(default = None)]
    pub print: Option<bool>,
    #[uniffi(default = None)]
    pub no_zoom: Option<bool>,
    #[uniffi(default = None)]
    pub no_rotate: Option<bool>,
    #[uniffi(default = None)]
    pub no_view: Option<bool>,
    #[uniffi(default = None)]
    pub read_only: Option<bool>,
    #[uniffi(default = None)]
    pub locked: Option<bool>,
    #[uniffi(default = None)]
    pub toggle_no_view: Option<bool>,
    #[uniffi(default = None)]
    pub locked_contents: Option<bool>,
}

flags_both_ways!(
    AnnotationFlagsPatch,
    core::AnnotationFlagsPatch,
    invisible,
    hidden,
    print,
    no_zoom,
    no_rotate,
    no_view,
    read_only,
    locked,
    toggle_no_view,
    locked_contents
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationLineEnding {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash,
}

macro_rules! enum_both_ways {
    ($ffi:ident, $core:path, $($variant:ident),*) => {
        impl From<$core> for $ffi {
            fn from(v: $core) -> Self { match v { $(<$core>::$variant => $ffi::$variant),* } }
        }
        impl From<$ffi> for $core {
            fn from(v: $ffi) -> Self { match v { $($ffi::$variant => <$core>::$variant),* } }
        }
    };
}

enum_both_ways!(
    AnnotationLineEnding,
    core::LineEnding,
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationLineEndings {
    pub start: AnnotationLineEnding,
    pub end: AnnotationLineEnding,
}

impl From<core::LineEndings> for AnnotationLineEndings {
    fn from(e: core::LineEndings) -> Self {
        AnnotationLineEndings { start: e.start.into(), end: e.end.into() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationLineEndingsPatch {
    #[uniffi(default = None)]
    pub start: Option<AnnotationLineEnding>,
    #[uniffi(default = None)]
    pub end: Option<AnnotationLineEnding>,
}

impl From<AnnotationLineEndingsPatch> for core::LineEndingsPatch {
    fn from(p: AnnotationLineEndingsPatch) -> Self {
        core::LineEndingsPatch { start: p.start.map(Into::into), end: p.end.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

enum_both_ways!(
    AnnotationBlendMode,
    core::BlendMode,
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextAlign {
    Left,
    Center,
    Right,
}

enum_both_ways!(AnnotationTextAlign, core::TextAlign, Left, Center, Right);

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationBorder {
    Solid,
    Dashed { dash: Vec<f64> },
    Cloudy { intensity: f64 },
}

impl From<core::Border> for AnnotationBorder {
    fn from(b: core::Border) -> Self {
        match b {
            core::Border::Solid => AnnotationBorder::Solid,
            core::Border::Dashed { dash } => AnnotationBorder::Dashed { dash },
            core::Border::Cloudy { intensity } => AnnotationBorder::Cloudy { intensity },
        }
    }
}

impl From<AnnotationBorder> for core::Border {
    fn from(b: AnnotationBorder) -> Self {
        match b {
            AnnotationBorder::Solid => core::Border::Solid,
            AnnotationBorder::Dashed { dash } => core::Border::Dashed { dash },
            AnnotationBorder::Cloudy { intensity } => core::Border::Cloudy { intensity },
        }
    }
}

/// A key of `AnnotationProps`, for `AnnotationPropsPatch.clear` and for a
/// tool's prop specs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AnnotationPropKey {
    Color,
    InteriorColor,
    StrokeWidth,
    Opacity,
    BlendMode,
    Border,
    LineEndings,
    FontFamily,
    FontSize,
    FontColor,
    TextAlign,
    Bold,
    Italic,
    Underline,
    Icon,
    Link,
}

enum_both_ways!(
    AnnotationPropKey,
    core::PropKey,
    Color,
    InteriorColor,
    StrokeWidth,
    Opacity,
    BlendMode,
    Border,
    LineEndings,
    FontFamily,
    FontSize,
    FontColor,
    TextAlign,
    Bold,
    Italic,
    Underline,
    Icon,
    Link
);

/// Editable appearance, as a read returns it: `Partial<AnnotationProps>`,
/// the keys the annotation's subtype has (a square has no font, a note no
/// stroke). A key the subtype lacks is `None`, and so is a cleared
/// `interior_color` or `link`. Keys the core keeps in `extra` (vendor keys
/// no `PropKey` names) are dropped: there is no typed home for them.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationProps {
    pub color: Option<String>,
    pub interior_color: Option<String>,
    pub stroke_width: Option<f64>,
    pub opacity: Option<f64>,
    pub blend_mode: Option<AnnotationBlendMode>,
    pub border: Option<AnnotationBorder>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_color: Option<String>,
    pub text_align: Option<AnnotationTextAlign>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub line_endings: Option<AnnotationLineEndings>,
    pub icon: Option<String>,
    pub link: Option<PdfLinkTarget>,
}

impl From<core::AnnotationProps> for AnnotationProps {
    fn from(p: core::AnnotationProps) -> Self {
        AnnotationProps {
            color: Some(p.color),
            interior_color: p.interior_color,
            stroke_width: Some(p.stroke_width),
            opacity: Some(p.opacity),
            blend_mode: Some(p.blend_mode.into()),
            border: Some(p.border.into()),
            font_family: Some(p.font_family),
            font_size: Some(p.font_size),
            font_color: Some(p.font_color),
            text_align: Some(p.text_align.into()),
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            line_endings: Some(p.line_endings.into()),
            icon: p.icon,
            link: match p.link {
                Nullable::Value(link) => Some(link.into()),
                Nullable::Null | Nullable::Undefined => None,
            },
        }
    }
}

/// A partial props bag (what a read carries) read through the core's patch
/// type, which already parses every key and keeps the rest in `extra`. Line
/// endings come back only when both sides are present, as a read writes
/// them.
impl From<core::AnnotationPropsPatch> for AnnotationProps {
    fn from(p: core::AnnotationPropsPatch) -> Self {
        let line_endings = p.line_endings.and_then(|e| match (e.start, e.end) {
            (Some(start), Some(end)) => {
                Some(AnnotationLineEndings { start: start.into(), end: end.into() })
            }
            _ => None,
        });
        AnnotationProps {
            color: p.color,
            interior_color: match p.interior_color {
                Nullable::Value(c) => Some(c),
                Nullable::Null | Nullable::Undefined => None,
            },
            stroke_width: p.stroke_width,
            opacity: p.opacity,
            blend_mode: p.blend_mode.map(Into::into),
            border: p.border.map(Into::into),
            font_family: p.font_family,
            font_size: p.font_size,
            font_color: p.font_color,
            text_align: p.text_align.map(Into::into),
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            line_endings,
            icon: p.icon,
            link: match p.link {
                Nullable::Value(link) => Some(link.into()),
                Nullable::Null | Nullable::Undefined => None,
            },
        }
    }
}

/// A partial property write. A `None` field is left alone, a value sets it,
/// and a key in `clear` is removed. Only `InteriorColor` and `Link` can be
/// cleared; naming another, or naming a key that is also set, is refused.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationPropsPatch {
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub interior_color: Option<String>,
    #[uniffi(default = None)]
    pub stroke_width: Option<f64>,
    #[uniffi(default = None)]
    pub opacity: Option<f64>,
    #[uniffi(default = None)]
    pub blend_mode: Option<AnnotationBlendMode>,
    #[uniffi(default = None)]
    pub border: Option<AnnotationBorder>,
    #[uniffi(default = None)]
    pub line_endings: Option<AnnotationLineEndingsPatch>,
    #[uniffi(default = None)]
    pub font_family: Option<String>,
    #[uniffi(default = None)]
    pub font_size: Option<f64>,
    #[uniffi(default = None)]
    pub font_color: Option<String>,
    #[uniffi(default = None)]
    pub text_align: Option<AnnotationTextAlign>,
    #[uniffi(default = None)]
    pub bold: Option<bool>,
    #[uniffi(default = None)]
    pub italic: Option<bool>,
    #[uniffi(default = None)]
    pub underline: Option<bool>,
    #[uniffi(default = None)]
    pub icon: Option<String>,
    #[uniffi(default = None)]
    pub link: Option<PdfLinkTarget>,
    #[uniffi(default = [])]
    pub clear: Vec<AnnotationPropKey>,
}

impl TryFrom<AnnotationPropsPatch> for core::AnnotationPropsPatch {
    type Error = Error;

    fn try_from(p: AnnotationPropsPatch) -> Result<Self, Error> {
        let mut interior_color = p.interior_color.map_or(Nullable::Undefined, Nullable::Value);
        let mut link = p.link.map(Into::into).map_or(Nullable::Undefined, Nullable::Value);
        for key in &p.clear {
            let slot_is_set = match key {
                AnnotationPropKey::InteriorColor => {
                    let set = !interior_color.is_undefined();
                    interior_color = Nullable::Null;
                    set
                }
                AnnotationPropKey::Link => {
                    let set = !link.is_undefined();
                    link = Nullable::Null;
                    set
                }
                other => {
                    return Err(invalid(format!(
                        "props patch clears {:?}, which has no cleared state",
                        core::PropKey::from(*other).as_str()
                    )));
                }
            };
            if slot_is_set {
                return Err(invalid(format!(
                    "props patch both sets and clears \"{}\"",
                    core::PropKey::from(*key).as_str()
                )));
            }
        }
        let mut out = core::AnnotationPropsPatch {
            color: p.color,
            interior_color,
            stroke_width: p.stroke_width,
            opacity: p.opacity,
            blend_mode: p.blend_mode.map(Into::into),
            border: p.border.map(Into::into),
            line_endings: p.line_endings.map(Into::into),
            font_family: p.font_family,
            font_size: p.font_size,
            font_color: p.font_color,
            text_align: p.text_align.map(Into::into),
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            icon: p.icon,
            link,
            ..Default::default()
        };
        // Declaration order, the order `keys()` falls back to.
        out.order = core::PropKey::ALL
            .into_iter()
            .map(|k| k.as_str().to_string())
            .filter(|k| out.has(k))
            .collect();
        Ok(out)
    }
}

/// A free-text callout's leader.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCallout {
    pub tip: Point,
    pub knee: Option<Point>,
    pub ending: AnnotationLineEnding,
}

impl From<core::Callout> for AnnotationCallout {
    fn from(c: core::Callout) -> Self {
        AnnotationCallout {
            tip: c.tip.into(),
            knee: c.knee.map(Into::into),
            ending: c.ending.into(),
        }
    }
}

impl From<AnnotationCallout> for core::Callout {
    fn from(c: AnnotationCallout) -> Self {
        core::Callout { tip: c.tip.into(), knee: c.knee.map(Into::into), ending: c.ending.into() }
    }
}

/// An in-progress polygon or polyline: what a "Finish" button needs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreationDraftAnchor {
    /// `true` for a polygon, `false` for a polyline.
    pub closed: bool,
    pub page: PageRef,
    pub bounds: crate::geometry::Rect,
    pub point_count: u32,
    pub min_points: u32,
    pub can_finish: bool,
}

impl From<core::CreationDraftAnchor> for CreationDraftAnchor {
    fn from(a: core::CreationDraftAnchor) -> Self {
        CreationDraftAnchor {
            closed: a.subtype == core::PolySubtype::Polygon,
            page: a.page.into(),
            bounds: a.bounds.into(),
            point_count: u32::try_from(a.point_count).unwrap_or(u32::MAX),
            min_points: u32::try_from(a.min_points).unwrap_or(u32::MAX),
            can_finish: a.can_finish,
        }
    }
}
