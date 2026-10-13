//! The selection plugin through uniffi: one handle over the selection, its
//! geometry and text extraction, and one event enum.
//!
//! Every record carries a kernel `PageRef` or an `epdf-geometry` quad, so
//! each is a mirror here. Character indices stay `i64`: they are PDFium
//! character indices (the space search hits address), not UTF-16 offsets
//! into extracted text. The selection's `ChangeOrigin` is always local,
//! with no session or actor, so only its trigger crosses.
//!
//! Every method hops to the kernel thread through `KernelBound`.

use epdf_ffi::{PageRef, Rect, TextQuad};

/// A position in a page's character space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextPosition {
    pub page: PageRef,
    /// A PDFium character index, not a UTF-16 offset.
    pub index: i64,
}

impl From<crate::TextPosition> for SelectionTextPosition {
    fn from(p: crate::TextPosition) -> Self {
        SelectionTextPosition { page: p.page.into(), index: p.index }
    }
}

impl From<SelectionTextPosition> for crate::TextPosition {
    fn from(p: SelectionTextPosition) -> Self {
        crate::TextPosition { page: p.page.into(), index: p.index }
    }
}

/// A half-open range in character space, in document order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextRange {
    pub start: SelectionTextPosition,
    pub end: SelectionTextPosition,
}

impl From<crate::TextRange> for SelectionTextRange {
    fn from(r: crate::TextRange) -> Self {
        SelectionTextRange { start: r.start.into(), end: r.end.into() }
    }
}

impl From<SelectionTextRange> for crate::TextRange {
    fn from(r: SelectionTextRange) -> Self {
        crate::TextRange { start: r.start.into(), end: r.end.into() }
    }
}

/// What `select` takes: a cross-page range, or a span on one page (a
/// search hit's shape: `char_start` and `char_count`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionInput {
    Range { range: SelectionTextRange },
    Span { page: PageRef, start: i64, count: i64 },
}

impl From<SelectionInput> for crate::SelectionRangeInput {
    fn from(i: SelectionInput) -> Self {
        match i {
            SelectionInput::Range { range } => crate::SelectionRangeInput::Range(range.into()),
            SelectionInput::Span { page, start, count } => {
                crate::SelectionRangeInput::Span { page: page.into(), start, count }
            }
        }
    }
}

/// One merged visual line of a selection, in page space.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// Reading direction along the baseline: 1 or -1.
    pub advance: i32,
}

impl From<&crate::SelectionSegment> for SelectionSegment {
    fn from(s: &crate::SelectionSegment) -> Self {
        SelectionSegment { quad: s.quad.into(), rect: s.rect.into(), advance: s.advance }
    }
}

/// A selection boundary on its boundary glyph's own cell.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionEndpoint {
    pub page: PageRef,
    pub glyph_quad: TextQuad,
    pub advance: i32,
    pub rect: Rect,
}

impl From<&crate::SelectionEndpoint> for SelectionEndpoint {
    fn from(e: &crate::SelectionEndpoint) -> Self {
        SelectionEndpoint {
            page: e.page.into(),
            glyph_quad: e.glyph_quad.into(),
            advance: e.advance,
            rect: e.rect.into(),
        }
    }
}

/// Where selection-scoped floating UI attaches.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionAnchor {
    pub page: PageRef,
    pub bounds: Rect,
}

impl From<crate::SelectionAnchor> for SelectionAnchor {
    fn from(a: crate::SelectionAnchor) -> Self {
        SelectionAnchor { page: a.page.into(), bounds: a.bounds.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionDirection {
    Forward,
    Backward,
}

impl From<crate::SelectionDirection> for SelectionDirection {
    fn from(d: crate::SelectionDirection) -> Self {
        match d {
            crate::SelectionDirection::Forward => SelectionDirection::Forward,
            crate::SelectionDirection::Backward => SelectionDirection::Backward,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshotPage {
    pub page: PageRef,
    pub segments: Vec<SelectionSegment>,
}

/// Everything a selection overlay draws.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshot {
    pub pages: Vec<SelectionSnapshotPage>,
    pub start: Option<SelectionEndpoint>,
    pub end: Option<SelectionEndpoint>,
    pub direction: SelectionDirection,
    /// A valid `select` input; `None` with nothing selected.
    pub range: Option<SelectionTextRange>,
}

impl From<&crate::SelectionSnapshot> for SelectionSnapshot {
    fn from(s: &crate::SelectionSnapshot) -> Self {
        SelectionSnapshot {
            pages: s
                .pages
                .iter()
                .map(|p| SelectionSnapshotPage {
                    page: p.page.into(),
                    segments: p.segments.iter().map(Into::into).collect(),
                })
                .collect(),
            start: s.start.as_ref().map(Into::into),
            end: s.end.as_ref().map(Into::into),
            direction: s.direction.into(),
            range: s.range.map(Into::into),
        }
    }
}

/// Who changed the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionChangeTrigger {
    User,
    Api,
    Script,
    System,
    Unknown,
}

impl From<crate::ChangeTrigger> for SelectionChangeTrigger {
    fn from(t: crate::ChangeTrigger) -> Self {
        match t {
            crate::ChangeTrigger::User => SelectionChangeTrigger::User,
            crate::ChangeTrigger::Api => SelectionChangeTrigger::Api,
            crate::ChangeTrigger::Script => SelectionChangeTrigger::Script,
            crate::ChangeTrigger::System => SelectionChangeTrigger::System,
            crate::ChangeTrigger::Unknown => SelectionChangeTrigger::Unknown,
        }
    }
}
