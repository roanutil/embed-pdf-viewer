//! `write/markup.ts`: text markup, carets and replace-text pairs. The
//! selection plugin's commit path (optimistic creates over text quads) and
//! the programmatic `createFromSelection`.

use epdf_annotation_core::js::Record;
use epdf_annotation_core::{AnnotationRef, Msg, PageRef, TextEndAnchor, TextQuad};
use epdf_kernel::rt::Promise;
use epdf_plugin_selection::SELECTION;

use crate::controller::AnnotationCapability;
use crate::promise;
use crate::services::write_sink::{create_effects_of, group_effects_of};

fn core_page(p: epdf_kernel::geometry::PageRef) -> PageRef {
    PageRef::new(p.page_object_number)
}

impl AnnotationCapability {
    /// `createMarkup(subtype, page, quads, preset)` (host).
    pub fn create_markup(
        &self,
        subtype: &str,
        page: PageRef,
        quads: Vec<TextQuad>,
        preset: Option<&str>,
    ) {
        if !self.can_create() {
            return;
        }
        self.commit(Msg::CreateMarkup {
            subtype: subtype.to_string(),
            page,
            quads,
            preset: preset.map(str::to_string),
            flags: preset.and_then(|p| self.tool(p)).and_then(|t| t.flags),
        });
    }

    /// `createCaret(page, anchor)` (host).
    pub fn create_caret(&self, page: PageRef, anchor: TextEndAnchor) {
        if !self.can_create() {
            return;
        }
        self.commit(Msg::CreateCaret { page, anchor, flags: None });
    }

    /// `createReplaceText(page, quads, anchor, preset)` (host).
    pub fn create_replace_text(
        &self,
        page: PageRef,
        quads: Vec<TextQuad>,
        anchor: TextEndAnchor,
        preset: Option<&str>,
    ) {
        if !self.can_create() {
            return;
        }
        self.commit(Msg::CreateReplaceText {
            page,
            quads,
            anchor,
            preset: preset.map(str::to_string),
        });
    }

    /// `previewMarkup(subtype, quadsByPage, preset)` (host).
    pub fn preview_markup(
        &self,
        subtype: &str,
        quads_by_page: Record<Vec<TextQuad>>,
        preset: Option<&str>,
    ) {
        self.commit(Msg::SetMarkupPreview {
            subtype: subtype.to_string(),
            quads_by_page,
            preset: preset.map(str::to_string),
        });
    }

    /// `clearMarkupPreview()` (host).
    pub fn clear_markup_preview(&self) {
        self.commit(Msg::ClearMarkupPreview);
    }

    /// `createFromSelection(subtype, { preset, clear })`: markup, an insert
    /// caret, replace-text pairs or redaction marks from the current text
    /// selection, one per page; `[]` without a selection.
    pub fn create_from_selection(
        &self,
        subtype: &str,
        preset: Option<String>,
        clear: Option<bool>,
    ) -> Promise<Vec<AnnotationRef>> {
        let this = self.clone();
        let subtype = subtype.to_string();
        promise::spawn(async move {
            this.assert_create()?;
            let Some(selection) = this.cx().try_get(&SELECTION)? else { return Ok(Vec::new()) };
            if !selection.has_selection() {
                return Ok(Vec::new());
            }
            let snapshot = selection.get_snapshot();
            let mut pending: Vec<Promise<AnnotationRef>> = Vec::new();
            if subtype == "insert-text" {
                if let Some(end) = &snapshot.end {
                    let effects = this.commit(Msg::CreateCaret {
                        page: core_page(end.page),
                        anchor: TextEndAnchor {
                            glyph_quad: end.glyph_quad,
                            advance: f64::from(end.advance),
                        },
                        flags: None,
                    });
                    pending.push(this.await_create(create_effects_of(&effects).into_iter().next()));
                }
            } else {
                for entry in &snapshot.pages {
                    if entry.segments.is_empty() {
                        continue;
                    }
                    let quads: Vec<TextQuad> = entry.segments.iter().map(|s| s.quad).collect();
                    let page = core_page(entry.page);
                    if subtype == "replace-text" {
                        let last = entry.segments.last().expect("non-empty");
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
                        let effects = this.commit(Msg::CreateReplaceText {
                            page,
                            quads,
                            anchor,
                            preset: preset.clone(),
                        });
                        pending
                            .push(this.await_create(group_effects_of(&effects).into_iter().next()));
                    } else {
                        let effects = this.commit(Msg::CreateMarkup {
                            subtype: subtype.clone(),
                            page,
                            quads,
                            preset: preset.clone(),
                            flags: preset
                                .as_deref()
                                .and_then(|p| this.tool(p))
                                .and_then(|t| t.flags),
                        });
                        for id in create_effects_of(&effects) {
                            pending.push(this.await_create(Some(id)));
                        }
                    }
                }
            }
            if clear != Some(false) {
                selection.clear();
            }
            // `return Promise.all(pending)`: the async return adopts it.
            let all = promise::all(pending);
            epdf_kernel::rt::tick().await;
            all.await
        })
    }
}
