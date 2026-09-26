//! `write/marks.ts`: every mark is a `redact` annotation created through
//! the annotation plugin.
//!
//! The TypeScript's `markArea`, `markPage` and `clearPending` are plain
//! functions that return a promise, so their refusals throw where they are
//! called; they return `Result<Promise<_>, Error>` here. The `async` verbs
//! (`markSelection`, `markMatches`, `unmark`, `updateLabel`) reject instead.

use std::collections::HashSet;

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{AnnotationRef, PageRef};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, is_undefined};
use epdf_plugin_annotation::promise::spawn;
use epdf_plugin_annotation::{CreateAnnotationInput, annotation_key};
use epdf_plugin_search::{SearchOptions, SearchQuery};
use serde_json::{Map, Value, json};

use crate::contract::RedactionLabelPatch;
use crate::controller::RedactionCapability;
use crate::write::adopt;

fn is_redact(dto: &Value) -> bool {
    dto.get("subtype").and_then(Value::as_str) == Some("redact")
}

/// A JavaScript truthiness test for an optional string.
fn truthy(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.is_empty())
}

impl RedactionCapability {
    fn assert_can_mark(&self) -> Result<(), Error> {
        if !self.annotation().can_create() {
            return Err(plugin_error(
                code::PERMISSION_DENIED,
                "redaction",
                "marking needs annotation create authority",
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `overlayProps()`: the configured overlay look, as annotation props.
    fn overlay_props(&self) -> Value {
        let mut props = Map::new();
        let Some(overlay) = &self.0.config.overlay else { return Value::Object(props) };
        if truthy(&overlay.fill) {
            props.insert("interiorColor".into(), json!(overlay.fill));
        }
        let text = overlay.text.as_ref();
        if let Some(color) = text.map(|t| &t.color).filter(|c| truthy(c)) {
            props.insert("fontColor".into(), json!(color));
        }
        if let Some(family) = text.map(|t| &t.font_family).filter(|f| truthy(f)) {
            props.insert("fontFamily".into(), json!(family));
        }
        if let Some(size) = text.and_then(|t| t.font_size) {
            props.insert("fontSize".into(), json!(size));
        }
        Value::Object(props)
    }

    /// `annotation.create(input)` over an object literal.
    fn create_mark(&self, input: Value) -> Promise<AnnotationRef> {
        match serde_json::from_value::<CreateAnnotationInput>(input) {
            Ok(input) => self.annotation().create(input),
            Err(error) => Promise::rejected(Error::new(error.to_string())),
        }
    }

    /// Mark the current text selection (one mark per page) and clear it.
    /// Empty without a selection.
    pub fn mark_selection(&self) -> Promise<Vec<AnnotationRef>> {
        let this = self.clone();
        spawn(async move {
            this.assert_can_mark()?;
            let Some(plane) = this.selection()? else {
                return Err(plugin_error(
                    code::UNSUPPORTED,
                    "redaction",
                    "no selection plugin",
                    None,
                    None,
                ));
            };
            if !plane.has_selection() {
                return Ok(Vec::new());
            }
            // The selected text frame rides all the way into `/QuadPoints`:
            // native apply uses these exact cells, so marked == previewed ==
            // applied.
            adopt(this.annotation().create_from_selection(
                "redact",
                Some("redact".into()),
                Some(true),
            ))
            .await
        })
    }

    /// Mark a page-space rectangle. `bounds` is the caller's `Rect`, as is.
    pub fn mark_area(
        &self,
        page: &PageRef,
        bounds: Value,
    ) -> Result<Promise<AnnotationRef>, Error> {
        self.assert_can_mark()?;
        Ok(self.create_mark(json!({
            "page": page,
            "subtype": "redact",
            "bounds": bounds,
            "tool": "redact",
            "props": self.overlay_props(),
        })))
    }

    /// Mark a whole page.
    pub fn mark_page(&self, page: &PageRef) -> Result<Promise<AnnotationRef>, Error> {
        let Some(layout) = self.layout_of(page) else {
            return Err(plugin_error(code::NOT_FOUND, "redaction", "no such page", None, None));
        };
        let size = layout.get("size").cloned().unwrap_or(Value::Null);
        let bounds = json!({
            "x": 0,
            "y": 0,
            "width": size.get("width").cloned().unwrap_or(Value::Null),
            "height": size.get("height").cloned().unwrap_or(Value::Null),
        });
        self.mark_area(page, bounds)
    }

    /// Mark every match of a search (needs the search plugin); `pages`
    /// keeps only the hits on those pages.
    pub fn mark_matches(
        &self,
        query: SearchQuery,
        pages: Option<Vec<PageRef>>,
    ) -> Promise<Vec<AnnotationRef>> {
        let this = self.clone();
        spawn(async move {
            this.assert_can_mark()?;
            let Some(finder) = this.search()? else {
                return Err(plugin_error(
                    code::UNSUPPORTED,
                    "redaction",
                    "no search plugin",
                    None,
                    None,
                ));
            };
            finder.search(query, SearchOptions::default()).await?;
            let wanted: Option<HashSet<u32>> =
                pages.map(|pages| pages.iter().map(|p| p.page_object_number).collect());
            let mut refs = Vec::new();
            for hit in finder.list_hits(None).iter() {
                if wanted.as_ref().is_some_and(|w| !w.contains(&hit.page.page_object_number)) {
                    continue;
                }
                if hit.segments.is_empty() {
                    continue;
                }
                let quads: Vec<Value> = hit
                    .segments
                    .iter()
                    .map(|s| serde_json::to_value(s.quad).unwrap_or_default())
                    .collect();
                let created = this.create_mark(json!({
                    "page": PageRef::new(hit.page.page_object_number),
                    "subtype": "redact",
                    "quads": quads,
                    "tool": "redact",
                    "props": this.overlay_props(),
                }));
                refs.push(created.await?);
            }
            Ok(refs)
        })
    }

    /// Remove marks. Refs that are not pending marks are skipped. Resolves
    /// with the annotation plane's `BatchResult` as JSON.
    pub fn unmark(&self, refs: Vec<AnnotationRef>) -> Promise<Value> {
        let this = self.clone();
        spawn(async move {
            let pending: HashSet<String> =
                this.list_pending(None).iter().map(|mark| annotation_key(&mark.ref_)).collect();
            let (marks, others): (Vec<AnnotationRef>, Vec<AnnotationRef>) =
                refs.into_iter().partition(|r| pending.contains(&annotation_key(r)));
            let skipped: Vec<Value> = others
                .iter()
                .map(|r| json!({ "ref": r, "reason": "not a pending redaction mark" }))
                .collect();
            if marks.is_empty() {
                return Ok(json!({ "applied": [], "skipped": skipped, "failed": [] }));
            }
            let mut result = this.annotation().delete_many(marks).await?.to_json();
            if let Some(Value::Array(items)) = result.get_mut("skipped") {
                items.extend(skipped);
            }
            Ok(result)
        })
    }

    /// Remove every mark.
    pub fn clear_pending(&self) -> Promise<Value> {
        self.unmark(self.list_pending(None).into_iter().map(|mark| mark.ref_).collect())
    }

    /// Edit a mark's overlay label, always carrying its current `/DA`
    /// styling: the engine rewrites `/DA` whenever a label field rides a
    /// patch, so a text-only edit must not let the styling fall back to
    /// defaults.
    pub fn update_label(&self, r: AnnotationRef, patch: RedactionLabelPatch) -> Promise<()> {
        let this = self.clone();
        spawn(async move {
            let Some(current) = this.annotation().get_raw(&r).filter(is_redact) else {
                return Err(plugin_error(
                    code::NOT_FOUND,
                    "redaction",
                    "the target is not a redaction mark",
                    None,
                    None,
                ));
            };
            let mut body = Map::new();
            body.insert("subtype".into(), json!("redact"));
            match &patch.overlay_text {
                Nullable::Undefined => {}
                Nullable::Null => {
                    body.insert("overlayText".into(), Value::Null);
                }
                Nullable::Value(text) => {
                    body.insert("overlayText".into(), json!(text));
                }
            }
            if let Some(repeat) = patch.repeat {
                body.insert("repeat".into(), json!(repeat));
            }
            for key in ["fontFamily", "fontSize", "fontColor", "textAlign"] {
                if let Some(value) = current.get(key).filter(|v| !is_undefined(v)) {
                    body.insert(key.into(), value.clone());
                }
            }
            this.annotation()
                .update_raw(r, Value::Object(body))
                .await
                .map_err(|error| to_plugin_error("redaction", &error))
        })
    }
}
