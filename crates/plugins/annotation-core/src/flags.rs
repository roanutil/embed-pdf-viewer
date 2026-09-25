//! `flags.ts`: the one interpretation of the `/F` annotation flags.

use serde::{Deserialize, Serialize};

use crate::kinds::caps_for;
use crate::types::{Annot, AnnotationFlags, AnnotationFlagsPatch, Authority};

/// `NO_ANNOTATION_FLAGS` (engine-core): every flag clear.
pub const NO_ANNOTATION_FLAGS: AnnotationFlags = AnnotationFlags {
    invisible: false,
    hidden: false,
    print: false,
    no_zoom: false,
    no_rotate: false,
    no_view: false,
    read_only: false,
    locked: false,
    toggle_no_view: false,
    locked_contents: false,
};

/// The `/F` every freshly drawn annotation starts with: `print` set.
pub const DRAWN_FLAGS: AnnotationFlags = AnnotationFlags { print: true, ..NO_ANNOTATION_FLAGS };

/// All ten flags, in the primitives' order.
pub const FLAG_KEYS: [&str; 10] = [
    "invisible",
    "hidden",
    "print",
    "noZoom",
    "noRotate",
    "noView",
    "readOnly",
    "locked",
    "toggleNoView",
    "lockedContents",
];

pub fn flags_equal(a: &AnnotationFlags, b: &AnnotationFlags) -> bool {
    a == b
}

/// `{ ...base, ...patch }`.
pub fn merge_flags(base: &AnnotationFlags, patch: &AnnotationFlagsPatch) -> AnnotationFlags {
    AnnotationFlags {
        invisible: patch.invisible.unwrap_or(base.invisible),
        hidden: patch.hidden.unwrap_or(base.hidden),
        print: patch.print.unwrap_or(base.print),
        no_zoom: patch.no_zoom.unwrap_or(base.no_zoom),
        no_rotate: patch.no_rotate.unwrap_or(base.no_rotate),
        no_view: patch.no_view.unwrap_or(base.no_view),
        read_only: patch.read_only.unwrap_or(base.read_only),
        locked: patch.locked.unwrap_or(base.locked),
        toggle_no_view: patch.toggle_no_view.unwrap_or(base.toggle_no_view),
        locked_contents: patch.locked_contents.unwrap_or(base.locked_contents),
    }
}

/// On-screen visibility: `hidden` beats everything; `noView` hides unless
/// `toggleNoView` and the annotation is engaged (selected).
pub fn viewable(f: &AnnotationFlags, engaged: bool) -> bool {
    !f.hidden && (!f.no_view || (f.toggle_no_view && engaged))
}

/// Any pointer interaction. ReadOnly kills it.
pub fn interactive(f: &AnnotationFlags) -> bool {
    !f.hidden && !f.no_view && !f.read_only
}

/// The members of an annotation the flag predicates read.
pub trait FlagBearer {
    fn subtype(&self) -> &str;
    fn flags(&self) -> &AnnotationFlags;
    fn authority(&self) -> Option<Authority>;
}

impl FlagBearer for Annot {
    fn subtype(&self) -> &str {
        &self.subtype
    }
    fn flags(&self) -> &AnnotationFlags {
        &self.flags
    }
    fn authority(&self) -> Option<Authority> {
        self.authority
    }
}

/// `FlagBearer` as a value: a subtype, its flags and optional authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlagFields {
    pub subtype: String,
    pub flags: AnnotationFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<Authority>,
}

impl FlagBearer for FlagFields {
    fn subtype(&self) -> &str {
        &self.subtype
    }
    fn flags(&self) -> &AnnotationFlags {
        &self.flags
    }
    fn authority(&self) -> Option<Authority> {
        self.authority
    }
}

/// Interaction gate for a concrete annotation: widget kinds ignore `readOnly`.
pub fn annot_interactive(a: &impl FlagBearer) -> bool {
    let f = a.flags();
    if caps_for(a.subtype()).ignores_read_only { !f.hidden && !f.no_view } else { interactive(f) }
}

/// Geometry and style mutations: interactive, not `locked`, and the session
/// holds update authority (absent authority allows).
pub fn annot_transformable(a: &impl FlagBearer) -> bool {
    annot_interactive(a) && !a.flags().locked && a.authority().is_none_or(|x| x.update)
}

/// Deletion: the delete half of the authority split.
pub fn annot_deletable(a: &impl FlagBearer) -> bool {
    annot_interactive(a) && !a.flags().locked && a.authority().is_none_or(|x| x.delete)
}

/// `/Contents` text edits: the contents counterpart of `locked`.
pub fn annot_contents_editable(a: &impl FlagBearer) -> bool {
    annot_interactive(a) && !a.flags().locked_contents && a.authority().is_none_or(|x| x.update)
}
