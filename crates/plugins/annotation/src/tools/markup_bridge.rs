//! `markup.ts`: the bridge that turns a text selection into markup or a
//! text-edit annotation. Selection is the producer, annotation the consumer.

use epdf_annotation_core::TextEndAnchor;
use epdf_annotation_core::js::Record;
use epdf_plugin_interaction::InteractionCapability;
use epdf_plugin_selection::SelectionCapability;

use crate::capability::AnnotationCapability;
use crate::tools::{SelectionAuthoring, TextEditOperation};

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
            let tool = annotation.tool(&interaction.active_tool_id());
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
                    for page in selection.snapshot().pages {
                        quads_by_page.insert(
                            page.pon.to_string(),
                            page.segments.iter().map(|s| s.quad).collect(),
                        );
                    }
                    annotation.preview_markup(subtype, quads_by_page, Some(&tool.preset));
                }
                _ => annotation.clear_markup_preview(),
            }
        }
    };
    let sync = std::rc::Rc::new(sync);
    let on_change = sync.clone();
    selection.on_change(move || on_change());
    let on_tool = sync.clone();
    interaction.on_tool_change(move || on_tool());

    let (commit_annotation, commit_selection) = (annotation, selection.clone());
    selection.on_commit(move || {
        let annotation = &commit_annotation;
        let selection = &commit_selection;
        let Some(tool) = annotation.tool(&interaction.active_tool_id()) else { return };
        let Some(authoring) = tool.selection else { return };
        let snapshot = selection.snapshot();
        match authoring {
            SelectionAuthoring::TextEdit { operation: TextEditOperation::Insert } => {
                if let Some(end) = &snapshot.end {
                    annotation.create_caret(
                        end.pon,
                        TextEndAnchor {
                            glyph_quad: end.glyph_quad,
                            advance: f64::from(end.advance),
                        },
                    );
                }
                selection.clear();
            }
            SelectionAuthoring::TextEdit { operation: TextEditOperation::Replace } => {
                for page in &snapshot.pages {
                    let Some(last) = page.segments.last() else { continue };
                    let anchor = match &snapshot.end {
                        Some(end) if end.pon == page.pon => TextEndAnchor {
                            glyph_quad: end.glyph_quad,
                            advance: f64::from(end.advance),
                        },
                        _ => TextEndAnchor {
                            glyph_quad: last.quad,
                            advance: f64::from(last.advance),
                        },
                    };
                    annotation.create_replace_text(
                        page.pon,
                        page.segments.iter().map(|s| s.quad).collect(),
                        anchor,
                        Some(&tool.preset),
                    );
                }
                selection.clear();
            }
            SelectionAuthoring::Markup => {
                for page in &snapshot.pages {
                    annotation.create_markup(
                        &tool.subtype,
                        page.pon,
                        page.segments.iter().map(|s| s.quad).collect(),
                        Some(&tool.preset),
                    );
                }
                selection.clear();
            }
        }
    });
}
