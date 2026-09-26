//! `tools/markup-bridge.ts`: the bridge that turns a text selection into
//! markup or a text-edit annotation. Selection is the producer, annotation
//! the consumer; selection never knows annotation exists.

use std::rc::Rc;

use epdf_annotation_core::js::Record;
use epdf_annotation_core::{PageRef, TextEndAnchor};
use epdf_plugin_interaction::InteractionCapability;
use epdf_plugin_selection::SelectionCapability;

use crate::controller::AnnotationCapability;
use crate::tools::definitions::{SelectionAuthoring, TextEditOperation};

fn core_page(p: epdf_kernel::geometry::PageRef) -> PageRef {
    PageRef::new(p.page_object_number)
}

/// `wireMarkup(annotation, selection, interaction)`: keep the live preview
/// in step with the active tool and the selection, and author on commit.
pub fn wire_markup(
    annotation: AnnotationCapability,
    selection: SelectionCapability,
    interaction: InteractionCapability,
) {
    let sync = {
        let (annotation, selection, interaction) =
            (annotation.clone(), selection.clone(), interaction.clone());
        move || {
            let tool = annotation.get_resolved_tool(&interaction.get_active_tool_id());
            let authoring = tool.as_ref().and_then(|t| t.selection);
            let preview_subtype = match authoring {
                Some(SelectionAuthoring::Markup) => tool.as_ref().map(|t| t.subtype.clone()),
                Some(SelectionAuthoring::TextEdit { operation: TextEditOperation::Replace }) => {
                    Some("strikeout".to_string())
                }
                _ => None,
            };
            selection.set_highlight_visible(preview_subtype.is_none());
            match (&preview_subtype, &tool) {
                (Some(subtype), Some(tool)) if selection.has_selection() => {
                    let mut quads_by_page = Record::new();
                    for entry in selection.get_snapshot().pages {
                        quads_by_page.insert(
                            entry.page.page_object_number.to_string(),
                            entry.segments.iter().map(|s| s.quad).collect(),
                        );
                    }
                    annotation.preview_markup(subtype, quads_by_page, Some(&tool.preset));
                }
                _ => annotation.clear_markup_preview(),
            }
        }
    };
    let sync = Rc::new(sync);
    let on_change = sync.clone();
    let _ = selection.on_changed(move |_| {
        on_change();
        Ok(())
    });
    let on_tool = sync.clone();
    let _ = interaction.on_tool_changed(move |_| {
        on_tool();
        Ok(())
    });

    let (commit_annotation, commit_selection) = (annotation, selection.clone());
    let _ = selection.on_committed(move |_| {
        let annotation = &commit_annotation;
        let selection = &commit_selection;
        let Some(tool) = annotation.get_resolved_tool(&interaction.get_active_tool_id()) else {
            return Ok(());
        };
        let Some(authoring) = tool.selection else { return Ok(()) };
        let snapshot = selection.get_snapshot();
        match authoring {
            SelectionAuthoring::TextEdit { operation: TextEditOperation::Insert } => {
                if let Some(end) = &snapshot.end {
                    annotation.create_caret(
                        core_page(end.page),
                        TextEndAnchor {
                            glyph_quad: end.glyph_quad,
                            advance: f64::from(end.advance),
                        },
                    );
                }
                selection.clear();
            }
            SelectionAuthoring::TextEdit { operation: TextEditOperation::Replace } => {
                for entry in &snapshot.pages {
                    let Some(last) = entry.segments.last() else { continue };
                    let anchor = match &snapshot.end {
                        Some(end)
                            if end.page.page_object_number == entry.page.page_object_number =>
                        {
                            TextEndAnchor {
                                glyph_quad: end.glyph_quad,
                                advance: f64::from(end.advance),
                            }
                        }
                        _ => TextEndAnchor {
                            glyph_quad: last.quad,
                            advance: f64::from(last.advance),
                        },
                    };
                    annotation.create_replace_text(
                        core_page(entry.page),
                        entry.segments.iter().map(|s| s.quad).collect(),
                        anchor,
                        Some(&tool.preset),
                    );
                }
                selection.clear();
            }
            SelectionAuthoring::Markup => {
                for entry in &snapshot.pages {
                    annotation.create_markup(
                        &tool.subtype,
                        core_page(entry.page),
                        entry.segments.iter().map(|s| s.quad).collect(),
                        Some(&tool.preset),
                    );
                }
                selection.clear();
            }
        }
        Ok(())
    });
}
