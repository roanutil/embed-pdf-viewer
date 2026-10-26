//! `write/marks.ts`: every mark is a `redact` annotation created through
//! the annotation plugin, in the style of the redact tool's defaults, which
//! follow the `overlay` settings (`sync/overlay.rs`). Removing a mark and
//! changing its label are annotation deletes and updates, checked per mark.
//!
//! Every verb is an `async` function in the TypeScript, so each refusal
//! rejects the returned promise.

use std::collections::HashSet;

use epdf_annotation_core::record::annotation_key;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, is_undefined};
use epdf_plugin_search::{SearchOptions, SearchQuery};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::controller::RedactionCapability;
use crate::services::store::CAPABILITY;

/// The redact tool's defaults: the fields a mark draft takes from them.
const STYLE_FIELDS: [&str; 7] =
    ["color", "interiorColor", "opacity", "fontFamily", "fontSize", "fontColor", "textAlign"];

/// `throwIfCancelled(signal)`: stop before the next step once the caller
/// cancelled.
fn throw_if_cancelled(signal: Option<&AbortSignal>) -> Result<(), Error> {
    match signal.filter(|s| s.aborted()) {
        Some(signal) => Err(plugin_error(
            code::OPERATION_CANCELLED,
            CAPABILITY,
            "operation cancelled",
            signal.reason_value(),
            None,
        )),
        None => Ok(()),
    }
}

fn unsupported(message: &str) -> Error {
    plugin_error(code::UNSUPPORTED, CAPABILITY, message, None, None)
}

/// `new PluginError(code, 'redaction', message, { permission })`.
fn denied(message: &str, permission: &str) -> Error {
    plugin_error(code::PERMISSION_DENIED, CAPABILITY, message, None, None)
        .with("permission", permission)
}

impl RedactionCapability {
    /// `markStyle()`: a new mark's style, as the engine takes it: the redact
    /// tool's current defaults, which carry the `overlay` settings, so a
    /// mark from code looks like one drawn with the tool. A field the
    /// defaults lack is left out, as JSON leaves out `undefined`.
    fn mark_style(&self) -> Map<String, Value> {
        let tool = self.core().siblings.annotation.tools_get_defaults("redact");
        let mut style = Map::new();
        for field in STYLE_FIELDS {
            if let Some(value) = tool.get(field).filter(|v| !is_undefined(v)) {
                style.insert(field.into(), value.clone());
            }
        }
        style
    }

    /// `markOfRecord(record)`: the mark a created or changed record is.
    fn mark_of_record(&self, record: &Value) -> Result<RedactionMark, Error> {
        self.mark_for(record).ok_or_else(|| {
            plugin_error(code::OPERATION_FAILED, CAPABILITY, "the record is not a mark", None, None)
        })
    }

    /// `isPending(ref)`: whether a ref is a mark waiting to be applied.
    fn is_pending(&self, reference: &Value) -> bool {
        let Some(key) = annotation_key(reference) else { return false };
        self.list_pending(None)
            .iter()
            .any(|mark| annotation_key(&mark.ref_).as_deref() == Some(key.as_str()))
    }

    /// Whether this mark may be removed before it's applied: it is a mark,
    /// and may be deleted (`annotations:delete`).
    pub fn can_unmark(&self, reference: &Value) -> bool {
        self.is_pending(reference) && self.core().siblings.annotation.can_delete(reference)
    }

    /// Whether this mark's label may change: it is a mark, and may be
    /// changed (`annotations:update`).
    pub fn can_update_label(&self, reference: &Value) -> bool {
        self.is_pending(reference) && self.core().siblings.annotation.can_update(reference)
    }

    /// Mark the current text selection (one mark per page) and clear it.
    /// Resolves `{ marks }`, empty without a selection; rejects
    /// `unsupported` without the selection plugin.
    pub fn mark_selection(&self, options: OperationOptions) -> Promise<MarksResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed("annotations:create", "redaction.markSelection")?;
            let Some(plane) = core.siblings.selection()? else {
                return Err(unsupported("no selection plugin"));
            };
            if !plane.has_selection() {
                return Ok(MarksResult { marks: Vec::new() });
            }
            // The selected text frame rides all the way into `/QuadPoints`:
            // native apply uses these exact cells, so marked == previewed ==
            // applied.
            let created = core
                .siblings
                .annotation
                .create_from_selection("redact", true, options.signal.clone())
                .await?;
            let records = created.get("annotations").and_then(Value::as_array);
            let marks = records
                .into_iter()
                .flatten()
                .map(|record| this.mark_of_record(record))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(MarksResult { marks })
        })
    }

    /// `createMark(page, region, signal)`.
    async fn create_mark(
        &self,
        page: PageRef,
        region: Map<String, Value>,
        signal: Option<AbortSignal>,
    ) -> Result<RedactionMark, Error> {
        let mut draft = Map::new();
        draft.insert("subtype".into(), json!("redact"));
        draft.extend(region);
        draft.extend(self.mark_style());
        let created = self
            .core()
            .siblings
            .annotation
            .create(
                &page.to_json(),
                &draft,
                None,
                epdf_plugin_annotation::write::crud::CreateOptions {
                    signal,
                    tool: None,
                    select: false,
                },
            )
            .await?;
        self.mark_of_record(created.get("annotation").unwrap_or(&Value::Null))
    }

    /// Mark a page-space rectangle. Resolves `{ mark }`; rejects `not-found`
    /// for a page that isn't there.
    pub fn mark_area(
        &self,
        page: PageTarget,
        bounds: epdf_geometry::Rect,
        options: OperationOptions,
    ) -> Promise<MarkResult> {
        self.mark_area_json(page, json!(bounds), options)
    }

    /// [`mark_area`](Self::mark_area) with the bounds as the caller's JSON,
    /// passed to the engine as they are.
    pub fn mark_area_json(
        &self,
        page: PageTarget,
        bounds: Value,
        options: OperationOptions,
    ) -> Promise<MarkResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed("annotations:create", "redaction.markArea")?;
            let target = core.store.page_ref_of(page)?;
            let mut region = Map::new();
            region.insert("rect".into(), bounds);
            let mark = this.create_mark(target, region, options.signal).await?;
            Ok(MarkResult { mark })
        })
    }

    /// Mark a whole page. Resolves `{ mark }`; rejects `not-found` for a
    /// page that isn't there.
    pub fn mark_page(&self, page: PageTarget, options: OperationOptions) -> Promise<MarkResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed("annotations:create", "redaction.markPage")?;
            let layout = core.store.page_of(page)?;
            let target = core.store.ref_of_layout(&layout)?;
            let size = layout.get("size");
            let member = |key: &str| {
                size.and_then(|s| s.get(key)).cloned().unwrap_or_else(epdf_kernel::undefined)
            };
            let mut region = Map::new();
            region.insert(
                "rect".into(),
                json!({ "x": 0, "y": 0, "width": member("width"), "height": member("height") }),
            );
            let mark = this.create_mark(target, region, options.signal).await?;
            Ok(MarkResult { mark })
        })
    }

    /// Mark every match of a search, or the ones on some pages. Runs the
    /// query through the search plugin, so it replaces the user's current
    /// search. Resolves `{ marks }`, one per match; rejects `unsupported`
    /// without the search plugin.
    pub fn mark_matches(
        &self,
        query: SearchQuery,
        pages: Option<Vec<PageTarget>>,
        options: OperationOptions,
    ) -> Promise<MarksResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed("annotations:create", "redaction.markMatches")?;
            let signal = options.signal;
            let Some(finder) = core.siblings.search()? else {
                return Err(unsupported("no search plugin"));
            };
            let wanted: Option<HashSet<u32>> = match pages {
                Some(pages) => Some(
                    pages
                        .into_iter()
                        .map(|page| core.store.page_ref_of(page).map(|p| p.page_object_number))
                        .collect::<Result<_, _>>()?,
                ),
                None => None,
            };
            finder.search(query, SearchOptions { from: None, signal: signal.clone() }).await?;
            let mut marks = Vec::new();
            for hit in finder.list_hits(None).iter() {
                if wanted.as_ref().is_some_and(|w| !w.contains(&hit.page.page_object_number)) {
                    continue;
                }
                if hit.segments.is_empty() {
                    continue;
                }
                throw_if_cancelled(signal.as_ref())?;
                // The engine's rect covers the quads.
                let quads: Vec<Value> = hit.segments.iter().map(|s| json!(s.quad)).collect();
                let mut region = Map::new();
                region.insert("quadPoints".into(), Value::Array(quads));
                marks.push(this.create_mark(hit.page, region, signal.clone()).await?);
            }
            Ok(MarksResult { marks })
        })
    }

    /// Remove marks without applying them. Refs that are not pending marks
    /// are skipped; a mark this session may not delete fails with
    /// `permission-denied` (see [`can_unmark`](Self::can_unmark)). A cancel
    /// ends the batch: the marks removed so far stay removed.
    pub fn unmark(&self, refs: Vec<Value>, options: OperationOptions) -> Promise<BatchResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let signal = options.signal;
            throw_if_cancelled(signal.as_ref())?;
            let annotation = &this.core().siblings.annotation;
            let mut result = BatchResult::default();
            for reference in refs {
                if !this.is_pending(&reference) {
                    result.skipped.push(BatchSkip {
                        ref_: reference,
                        reason: "not a pending redaction mark".into(),
                    });
                    continue;
                }
                if !annotation.can_delete(&reference) {
                    let refused = denied(
                        "redaction.unmark requires annotations:delete",
                        "annotations:delete",
                    );
                    result
                        .failed
                        .push(BatchFailure { ref_: reference, error: refused.plugin_error_info() });
                    continue;
                }
                match annotation.delete(&reference, signal.clone()).await {
                    Ok(()) => result.applied.push(reference),
                    Err(error) => {
                        let refused = to_plugin_error(CAPABILITY, &error);
                        if refused.plugin_code() == Some(code::OPERATION_CANCELLED) {
                            return Err(refused);
                        }
                        result.failed.push(BatchFailure {
                            ref_: reference,
                            error: refused.plugin_error_info(),
                        });
                    }
                }
            }
            Ok(result)
        })
    }

    /// Remove every mark, as [`unmark`](Self::unmark) does.
    pub fn clear_pending(&self, options: OperationOptions) -> Promise<BatchResult> {
        let refs = self.list_pending(None).iter().map(|mark| mark.ref_.clone()).collect();
        self.unmark(refs, options)
    }

    /// Set the label a mark shows once it's applied; `overlay_text:
    /// Some(None)` clears it. Keeps the mark's font and colors. Resolves
    /// `{ mark }`. Rejects `not-found` for a ref that isn't a mark,
    /// `permission-denied` when [`can_update_label`](Self::can_update_label)
    /// is false.
    pub fn update_label(
        &self,
        reference: Value,
        patch: RedactionLabelPatch,
        options: OperationOptions,
    ) -> Promise<MarkResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let annotation = &this.core().siblings.annotation;
            let current = annotation
                .get(&reference)
                .filter(|r| r.get("subtype").and_then(Value::as_str) == Some("redact"));
            let Some(current) = current else {
                return Err(plugin_error(
                    code::NOT_FOUND,
                    CAPABILITY,
                    "the target is not a redaction mark",
                    None,
                    None,
                ));
            };
            if !annotation.can_update(&reference) {
                return Err(denied(
                    "redaction.updateLabel requires annotations:update",
                    "annotations:update",
                ));
            }
            // Always carry the current /DA styling: the engine rewrites /DA
            // whenever a label field rides a patch, so a text-only edit must
            // not let the styling fall back to defaults.
            let mut body = Map::new();
            body.insert("subtype".into(), json!("redact"));
            if let Some(text) = &patch.overlay_text {
                body.insert("overlayText".into(), json!(text));
            }
            if let Some(repeat) = patch.repeat {
                body.insert("repeat".into(), json!(repeat));
            }
            for field in ["fontFamily", "fontSize", "fontColor", "textAlign"] {
                if let Some(value) = current.get(field).filter(|v| !is_undefined(v)) {
                    body.insert(field.into(), value.clone());
                }
            }
            let updated = annotation
                .update(&reference, &body, None, options.signal)
                .await
                .map_err(|error| to_plugin_error(CAPABILITY, &error))?;
            let mark = this
                .mark_of_record(updated.get("annotation").unwrap_or(&Value::Null))
                .map_err(|error| to_plugin_error(CAPABILITY, &error))?;
            Ok(MarkResult { mark })
        })
    }
}
