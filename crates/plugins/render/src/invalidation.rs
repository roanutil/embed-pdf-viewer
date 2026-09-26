//! `invalidation.ts`: the built-in event to pages map, which pages'
//! annotated raster a confirmed mutation repaints. Everything the engine
//! emits today is appearance-scoped; content-mutation events
//! (`redaction.applied`) are the controller's, with scope `content`. The map
//! is origin-agnostic: a baked raster is stale whoever moved the highlight.

use serde_json::Value;

fn pon_of(page: Option<&Value>) -> Option<u32> {
    page?.get("pageObjectNumber").and_then(Value::as_u64).map(|p| p as u32)
}

/// A widget list's placed pages; an unplaced widget (`page: null`) has no
/// pixels to repaint.
fn placed_pages(widgets: Option<&Value>) -> Vec<u32> {
    widgets
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| pon_of(w.get("page").filter(|p| !p.is_null())))
        .collect()
}

/// `annotatedPons(event, allPons)`.
pub fn annotated_pons(event: &Value, all_pons: impl FnOnce() -> Vec<u32>) -> Vec<u32> {
    match event.get("type").and_then(Value::as_str).unwrap_or_default() {
        // `annotation.moved` is a z-order move: baked stacking can change.
        "annotation.created" | "annotation.updated" | "annotation.deleted" | "annotation.moved" => {
            pon_of(event.get("page")).into_iter().collect()
        }
        "form.valueChanged" | "form.effectsApplied" => placed_pages(event.get("changedWidgets")),
        "form.fieldDeleted" => placed_pages(event.get("removedWidgets")),
        "form.fieldCreated"
        | "form.fieldUpdated"
        | "form.widgetAttached"
        | "form.widgetDetached" => placed_pages(event.get("field").and_then(|f| f.get("widgets"))),
        // Coarse results (counts only): repaint every page.
        "form.imported" | "form.repaired" => all_pons(),
        // pages.* replace the registry (the kernel's revision re-keys every
        // layout), and metadata never touches pixels.
        _ => Vec::new(),
    }
}
