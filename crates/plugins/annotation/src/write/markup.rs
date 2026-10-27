//! `write/markup.ts`: what a text tool makes of the text selection: markup
//! over the selected text, a caret at its end (insert text), or a caret and
//! strikeout pair (replace text), created over text and shown at once.

use std::rc::Rc;

use epdf_annotation_core::types::UpdateResult;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_selection::{SELECTION, SelectionSnapshot};
use serde_json::{Value, json};

use crate::controller::{CAPABILITY, Core};
use crate::services::store::{Adjust, Commit};
use crate::tools::definitions::ResolvedTool;
use crate::write::outcomes::created_ref_of;

/// What a text tool makes of a text selection.
enum SelectionMarkup {
    Markup(String),
    Insert,
    Replace,
}

/// `selectionMarkupOf(tool)`.
fn selection_markup_of(tool: &ResolvedTool) -> Option<SelectionMarkup> {
    let authoring = tool.selection.as_ref()?;
    match authoring.get("kind").and_then(Value::as_str) {
        Some("markup") => Some(SelectionMarkup::Markup(tool.subtype.clone())),
        _ => match authoring.get("operation").and_then(Value::as_str) {
            Some("insert") => Some(SelectionMarkup::Insert),
            Some("replace") => Some(SelectionMarkup::Replace),
            _ => None,
        },
    }
}

impl Core {
    fn commit_markup(
        &self,
        subtype: &str,
        page: &Value,
        quads: &Value,
        preset: Option<&str>,
        adjust: Option<Adjust>,
    ) -> Result<Commit, Error> {
        let flags = preset.and_then(|p| self.tool(p)).and_then(|t| t.flags.clone());
        let mut message =
            json!({ "type": "createMarkup", "subtype": subtype, "page": page, "quads": quads });
        if let Some(p) = preset {
            message["preset"] = json!(p);
        }
        if let Some(f) = flags {
            message["flags"] = Value::Object(f);
        }
        self.commit(message, adjust, None)
    }

    fn commit_caret(
        &self,
        page: &Value,
        anchor: &Value,
        adjust: Option<Adjust>,
    ) -> Result<Commit, Error> {
        self.commit(json!({ "type": "createCaret", "page": page, "anchor": anchor }), adjust, None)
    }

    fn commit_replace_text(
        &self,
        page: &Value,
        quads: &Value,
        anchor: &Value,
        preset: Option<&str>,
        adjust: Option<Adjust>,
    ) -> Result<Commit, Error> {
        let mut message =
            json!({ "type": "createReplaceText", "page": page, "quads": quads, "anchor": anchor });
        if let Some(p) = preset {
            message["preset"] = json!(p);
        }
        self.commit(message, adjust, None)
    }

    /// `markupFromSelection(markup, snapshot, preset, adjust)`: one annotation per page.
    fn markup_from_selection(
        &self,
        markup: &SelectionMarkup,
        snapshot: &SelectionSnapshot,
        preset: Option<&str>,
        adjust: Adjust,
    ) -> Result<Vec<Commit>, Error> {
        let end = snapshot.end.as_ref();
        let anchor_of = |quad: &epdf_geometry::Quad, advance: i32| json!({ "glyphQuad": quad, "advance": advance });
        if let SelectionMarkup::Insert = markup {
            return match end {
                Some(end) => Ok(vec![self.commit_caret(
                    &end.page.to_json(),
                    &anchor_of(&end.glyph_quad, end.advance),
                    Some(adjust),
                )?]),
                None => Ok(Vec::new()),
            };
        }
        let mut commits = Vec::new();
        for entry in &snapshot.pages {
            let Some(last) = entry.segments.last() else { continue };
            let quads = json!(entry.segments.iter().map(|s| s.quad).collect::<Vec<_>>());
            let page = entry.page.to_json();
            match markup {
                SelectionMarkup::Markup(subtype) => {
                    commits.push(self.commit_markup(
                        subtype,
                        &page,
                        &quads,
                        preset,
                        Some(adjust.clone()),
                    )?);
                }
                _ => {
                    let anchor = match end {
                        Some(end)
                            if end.page.page_object_number == entry.page.page_object_number =>
                        {
                            anchor_of(&end.glyph_quad, end.advance)
                        }
                        _ => anchor_of(&last.quad, last.advance),
                    };
                    commits.push(self.commit_replace_text(
                        &page,
                        &quads,
                        &anchor,
                        preset,
                        Some(adjust.clone()),
                    )?);
                }
            }
        }
        Ok(commits)
    }

    /// `createMarkup(subtype, page, quads, preset)`.
    pub fn create_markup(
        &self,
        subtype: &str,
        page: &Value,
        quads: &Value,
        preset: Option<&str>,
    ) -> Result<(), Error> {
        if !self.can_create() {
            return Ok(());
        }
        self.commit_markup(subtype, page, quads, preset, None).map(|_| ())
    }

    /// `createCaret(page, anchor)`.
    pub fn create_caret(&self, page: &Value, anchor: &Value) -> Result<(), Error> {
        if !self.can_create() {
            return Ok(());
        }
        self.commit_caret(page, anchor, None).map(|_| ())
    }

    /// `createReplaceText(page, quads, anchor, preset)`.
    pub fn create_replace_text(
        &self,
        page: &Value,
        quads: &Value,
        anchor: &Value,
        preset: Option<&str>,
    ) -> Result<(), Error> {
        if !self.can_create() {
            return Ok(());
        }
        self.commit_replace_text(page, quads, anchor, preset, None).map(|_| ())
    }

    /// `applyToolToSelection(toolId)`: the text selection as what the tool makes of it.
    pub fn apply_tool_to_selection(&self, tool_id: &str) -> Result<bool, Error> {
        let Some(tool) = self.tool(tool_id) else { return Ok(false) };
        let Some(markup) = selection_markup_of(&tool) else { return Ok(false) };
        if !self.can_create() {
            return Ok(false);
        }
        let Some(selection) = self.cx.try_get(&SELECTION).ok().flatten() else { return Ok(false) };
        if !selection.has_selection() {
            return Ok(false);
        }
        let commits = self.markup_from_selection(
            &markup,
            &selection.get_snapshot(),
            Some(&tool.preset),
            self.after_create_shape(Some(tool.id.clone())),
        )?;
        selection.clear();
        for commit in &commits {
            self.after_create_done(Some(&tool.id), &commit.effects);
        }
        Ok(true)
    }

    /// `previewMarkup(subtype, quadsByPage, preset)`.
    pub fn preview_markup(
        &self,
        subtype: &str,
        quads_by_page: &Value,
        preset: Option<&str>,
    ) -> Result<(), Error> {
        let mut message =
            json!({ "type": "setMarkupPreview", "subtype": subtype, "quadsByPage": quads_by_page });
        if let Some(p) = preset {
            message["preset"] = json!(p);
        }
        self.commit_json(message).map(|_| ())
    }

    /// `clearMarkupPreview()`.
    pub fn clear_markup_preview(&self) {
        self.commit_or_log(json!({ "type": "clearMarkupPreview" }));
    }

    /// `createFromSelection(toolId, options)`: code never changes the annotation selection.
    pub fn create_from_selection(
        &self,
        tool_id: &str,
        clear: bool,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.createFromSelection")
        {
            return Promise::rejected(e);
        }
        let tool = self.tool(tool_id);
        let markup = tool.as_ref().and_then(|t| selection_markup_of(t));
        let (Some(tool), Some(markup)) = (tool, markup) else {
            return Promise::rejected(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("no tool '{tool_id}' that makes something of selected text"),
                None,
                None,
            ));
        };
        let Some(selection) = self.cx.try_get(&SELECTION).ok().flatten() else {
            return Promise::resolved(json!({ "annotations": [] }));
        };
        if !selection.has_selection() {
            return Promise::resolved(json!({ "annotations": [] }));
        }
        let before = self.model();
        let keep: Adjust = Rc::new(move |result: UpdateResult| {
            let mut session = result.session.clone();
            session.selected = before.selected.clone();
            session.editing = before.editing.clone();
            UpdateResult { session, ..result }
        });
        let commits = match self.markup_from_selection(
            &markup,
            &selection.get_snapshot(),
            Some(&tool.preset),
            keep,
        ) {
            Ok(c) => c,
            Err(e) => return Promise::rejected(e),
        };
        if clear {
            selection.clear();
        }
        let refs = crate::promise::all(
            commits.iter().filter(|c| !c.effects.is_empty()).map(created_ref_of).collect(),
        );
        let refs = self.cancellable(signal, refs);
        let me = self.weak();
        Promise::spawn(async move {
            let refs = refs.await?;
            let Some(core) = me.upgrade() else { return Ok(json!({ "annotations": [] })) };
            let annotations: Vec<Value> = refs.iter().filter_map(|r| core.get(r)).collect();
            Ok(json!({ "annotations": annotations }))
        })
    }
}
