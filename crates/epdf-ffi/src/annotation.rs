//! Records from `epdf-annotation-core` that several plugins' exports
//! return: where a link points, an explicit destination, and how the engine
//! addresses an annotation. The `From` impls live here because neither side
//! is a plugin type.
//!
//! A destination's coordinates are `Nullable` in the core (absent, `null`,
//! or a number). Absent and `null` both mean "keep the current value" to
//! every consumer, so both arrive as `None`, and `None` goes back as absent.

use epdf_annotation_core as ac;
use epdf_annotation_core::js::Nullable;

use crate::PageRef;

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
