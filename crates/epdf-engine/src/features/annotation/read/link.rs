//! The link.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readLinkAnnotation.ts`.
//!
//! A link is a hit rectangle that navigates. What is on the wire is the
//! NORMALIZED target: `/Dest` and `/A /GoTo` are the same intent to a viewer,
//! so both collapse onto `goto` and a client never sees the two-shape split.
//!
//! # One truth, one projection
//!
//! The target is derived from the SAME action tree the scripting plane reads,
//! `base.actions.activate`, rather than from a second native read of `/A`.
//! Two reads of one entry can disagree; a projection of one read cannot.
//!
//! # A broken action is a dead link
//!
//! An `/A` that exists but cannot be executed, because the tree is
//! `incomplete` or its root was dropped, projects [`PdfLinkTarget::
//! Unsupported`]. It does NOT fall through to `/Dest`. §12.5.6.5 gives `/A`
//! precedence, and falling back would navigate somewhere the document did not
//! ask for, which is worse than not navigating.

use pdfium::{Annotation, Document};
use serde::{Deserialize, Serialize};

use super::base::AnnotationBase;
use crate::features::actions::destination::{PdfDestination, read_destination};
use crate::features::actions::reader::{ActionNodeKind, ActionTree};

/// Where a link points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum PdfLinkTarget {
    Goto {
        destination: PdfDestination,
    },
    Uri {
        uri: String,
    },
    /// Reported, never followed.
    GotoRemote {
        file: String,
    },
    /// Reported, never executed.
    Launch {
        path: String,
    },
    /// Carries no script ON PURPOSE. The text already rides
    /// `actions.activate`, which is the scripting plane's one home, and a
    /// second copy here would be a second thing to keep in step.
    Javascript,
    /// A viewer verb: `NextPage`, `FirstPage` and the rest. Common in
    /// tables of contents.
    Named {
        name: String,
    },
    /// An action type this projection does not model, or one it refuses to.
    /// Keeps a round trip honest rather than dropping the link.
    Unsupported,
}

/// A link annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkFields {
    /// `None` is a DEAD link: no `/A` and no `/Dest`. Legal PDF, reported as
    /// it is rather than invented into something.
    pub target: Option<PdfLinkTarget>,
}

/// Read them.
///
/// Takes the base because the activate tree is already in it. Reading `/A`
/// again here is what "one truth, one projection" rules out.
pub fn read_link(
    document: &Document,
    annotation: &Annotation<'_>,
    base: &AnnotationBase,
) -> LinkFields {
    let activate = base.actions.as_ref().and_then(|actions| actions.activate.as_ref());
    if let Some(tree) = activate {
        return LinkFields { target: Some(link_target_from_action_tree(tree)) };
    }

    LinkFields {
        target: document.link_destination(annotation).map(|raw| {
            read_destination(&raw).map_or(PdfLinkTarget::Unsupported, |destination| {
                PdfLinkTarget::Goto { destination }
            })
        }),
    }
}

/// The navigation reading of an action tree.
///
/// Public because it is a law rather than a detail: a consumer that has a
/// tree and wants to know where it goes must get the same answer this reader
/// did, or the two disagree about what a link does.
pub fn link_target_from_action_tree(tree: &ActionTree) -> PdfLinkTarget {
    // Enforced at the projection as well as at execution: an incomplete tree
    // is not navigated, not even at its root.
    if tree.incomplete {
        return PdfLinkTarget::Unsupported;
    }
    let Some(root) = tree.root.as_ref() else {
        return PdfLinkTarget::Unsupported;
    };
    match &root.kind {
        ActionNodeKind::Goto { destination } => PdfLinkTarget::Goto { destination: *destination },
        ActionNodeKind::Uri { uri, .. } => PdfLinkTarget::Uri { uri: uri.clone() },
        ActionNodeKind::GotoRemote { file_path } => {
            PdfLinkTarget::GotoRemote { file: file_path.clone() }
        }
        ActionNodeKind::Launch { file_path } => PdfLinkTarget::Launch { path: file_path.clone() },
        ActionNodeKind::Javascript { .. } => PdfLinkTarget::Javascript,
        ActionNodeKind::Named { name } => PdfLinkTarget::Named { name: name.clone() },
        _ => PdfLinkTarget::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::actions::reader::ActionNode;
    use crate::features::actions::vocabulary::ActionWarningFlags;

    fn tree(kind: ActionNodeKind) -> ActionTree {
        ActionTree {
            root: Some(ActionNode { kind, subtype: "Test".into(), next: Vec::new() }),
            incomplete: false,
            warning_flags: ActionWarningFlags(0),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn an_incomplete_tree_is_never_navigated_not_even_at_its_root() {
        // The corpus cannot produce this. A `/Next` cycle, which is the
        // obvious way to try, is one PDFium breaks and still reports as
        // complete: `annotation_kinds.pdf` has a self-referential action and
        // it comes back with `cycle-dropped` set and `incomplete` clear. So
        // the law is pinned here, on the pure projection, rather than left to
        // a document nobody has managed to write.
        let mut broken =
            tree(ActionNodeKind::Uri { uri: "https://example.test".into(), is_map: false });
        broken.incomplete = true;
        assert_eq!(link_target_from_action_tree(&broken), PdfLinkTarget::Unsupported);
    }

    #[test]
    fn a_tree_whose_root_was_dropped_is_a_dead_end_rather_than_a_dead_link() {
        // `Unsupported`, not `None`. `None` means the annotation named
        // nowhere at all; this one named something the reader could not keep.
        let empty = ActionTree {
            root: None,
            incomplete: false,
            warning_flags: ActionWarningFlags(0),
            warnings: Vec::new(),
        };
        assert_eq!(link_target_from_action_tree(&empty), PdfLinkTarget::Unsupported);
    }

    #[test]
    fn the_read_only_arms_are_reported_rather_than_dropped() {
        assert_eq!(
            link_target_from_action_tree(&tree(ActionNodeKind::GotoRemote {
                file_path: "elsewhere.pdf".into(),
            })),
            PdfLinkTarget::GotoRemote { file: "elsewhere.pdf".into() },
        );
        assert_eq!(
            link_target_from_action_tree(&tree(ActionNodeKind::Launch {
                file_path: "/bin/sh".into(),
            })),
            PdfLinkTarget::Launch { path: "/bin/sh".into() },
        );
    }

    #[test]
    fn a_script_link_carries_no_script() {
        // The text rides `actions.activate`, which is the scripting plane's
        // one home. A copy here would be a second thing to keep in step, and
        // the first one to go stale.
        let projected = link_target_from_action_tree(&tree(ActionNodeKind::Javascript {
            script: "app.alert('hi');".into(),
        }));
        assert_eq!(projected, PdfLinkTarget::Javascript);
        assert_eq!(
            serde_json::to_value(&projected).unwrap(),
            serde_json::json!({ "kind": "javascript" }),
        );
    }

    #[test]
    fn an_action_type_with_no_navigation_reading_is_unsupported() {
        // A link whose `/A` submits a form goes nowhere. Reporting the
        // submission as a target would invite a viewer to follow it.
        assert_eq!(
            link_target_from_action_tree(&tree(ActionNodeKind::SubmitForm { payload: None })),
            PdfLinkTarget::Unsupported,
        );
        assert_eq!(
            link_target_from_action_tree(&tree(ActionNodeKind::Unknown)),
            PdfLinkTarget::Unsupported,
        );
    }
}
