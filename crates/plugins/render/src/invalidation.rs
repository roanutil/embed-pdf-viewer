//! `invalidation.ts`: the built-in event to pixels map, which pages'
//! pictures a confirmed fact repaints, and at which scope. `annotations`
//! repaints the pictures that draw annotations, `fields` the ones that draw
//! form fields, and `content` every picture (a redaction or a flatten
//! rewrites the content stream). Over-invalidation is acceptable;
//! under-invalidation is the bug.
//!
//! The map ignores origin: a baked raster is stale whether this session
//! moved the highlight or a collaborator did. Events fire only for confirmed
//! mutations, so optimistic previews never reach it.
//!
//! An event missing a member the map reads (an annotation fact with no
//! `page`, a result with no page) names no page for it, where the TypeScript
//! throws a `TypeError` reading `objectNumber` of `undefined`. Engine facts
//! always carry these members, and no vector sends one without them.

use indexmap::IndexSet;
use serde::Serialize;
use serde_json::Value;

use crate::contract::InvalidateScope;

/// `PixelChange`: the pages a fact repaints and the scope they repaint in.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PixelChange {
    pub pages: Vec<u32>,
    pub scope: InvalidateScope,
}

fn pon_of(page: Option<&Value>) -> Option<u32> {
    let pon = page?.get("objectNumber")?.as_f64()?;
    (pon >= 0.0 && pon.fract() == 0.0 && pon <= f64::from(u32::MAX)).then_some(pon as u32)
}

/// A widget list's placed pages; an unplaced widget (`page: null`) has no
/// pixels to repaint.
fn placed_pages(widgets: Option<&Value>) -> Vec<u32> {
    widgets
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|widget| pon_of(widget.get("page").filter(|p| !p.is_null())))
        .collect()
}

/// `changeOf(pages, scope)`: the pages once each, in first-seen order, or no
/// change for none.
fn change_of(pages: Vec<u32>, scope: InvalidateScope) -> Option<PixelChange> {
    if pages.is_empty() {
        return None;
    }
    let unique: IndexSet<u32> = pages.into_iter().collect();
    Some(PixelChange { pages: unique.into_iter().collect(), scope })
}

/// The pages a form fact repaints: the widgets `meta.changedWidgets` names,
/// the widget rows the event carries, and a deleted widget's page.
fn form_pages_of(event: &Value) -> Vec<u32> {
    let mut pages = placed_pages(event.get("meta").and_then(|m| m.get("changedWidgets")));
    pages.extend(placed_pages(event.get("widgets")));
    pages.extend(pon_of(event.get("page").filter(|p| !p.is_null())));
    pages
}

/// The pages whose results applied.
fn applied_pages(event: &Value) -> Vec<u32> {
    event
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|result| result.get("status").and_then(Value::as_str) == Some("applied"))
        .filter_map(|result| pon_of(result.get("page")))
        .collect()
}

/// `pixelChangeOf(event, allPageObjectNumbers)`.
pub fn pixel_change_of(event: &Value, all_pons: impl FnOnce() -> Vec<u32>) -> Option<PixelChange> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        // `annotations.reordered` is a z-order move: baked stacking can change.
        "annotations.created"
        | "annotations.updated"
        | "annotations.deleted"
        | "annotations.restored"
        | "annotations.reordered" => {
            change_of(pon_of(event.get("page")).into_iter().collect(), InvalidateScope::Annotations)
        }
        "forms.valueSet"
        | "forms.created"
        | "forms.updated"
        | "forms.deleted"
        | "forms.restored"
        | "forms.widgetAdded"
        | "forms.widgetRemoved"
        | "forms.widgetDeleted"
        | "forms.widgetRestored"
        | "forms.widgetUpdated"
        | "forms.widgetsReordered" => change_of(form_pages_of(event), InvalidateScope::Fields),
        // A coarse result (counts only, no per-widget detail): every page.
        "forms.repaired" => change_of(all_pons(), InvalidateScope::Fields),
        // Sealing bakes the signature into its widget's appearance.
        "signatures.completed" => {
            let widget = event.get("signature").and_then(|s| s.get("widget"));
            let pages = match widget.filter(|w| !w.is_null() && *w != &Value::Bool(false)) {
                Some(widget) => placed_pages(Some(&Value::Array(vec![widget.clone()]))),
                None => Vec::new(),
            };
            change_of(pages, InvalidateScope::Fields)
        }
        // These rewrite the page content itself.
        "redaction.applied" | "pages.flattened" => {
            change_of(applied_pages(event), InvalidateScope::Content)
        }
        "annotations.flattened" => {
            let applied = event.get("results").and_then(Value::as_array).is_some_and(|results| {
                results.iter().any(|r| r.get("status").and_then(Value::as_str) == Some("applied"))
            });
            let pages =
                if applied { pon_of(event.get("page")).into_iter().collect() } else { Vec::new() };
            change_of(pages, InvalidateScope::Content)
        }
        // The stream lost events that will never arrive: any page may be stale.
        "stream.desynced" => change_of(all_pons(), InvalidateScope::Content),
        // `pages.*` replace the page registry (the kernel's revision re-keys
        // every layout), and metadata never touches pixels. A new saved
        // version changes no pixels beyond the signature it seals.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn widget(pon: u32) -> Value {
        json!({ "page": { "kind": "objectNumber", "objectNumber": pon } })
    }

    #[test]
    fn a_signing_repaints_its_widget_page_and_nothing_without_one() {
        let event =
            json!({ "type": "signatures.completed", "signature": { "widget": widget(22) } });
        assert_eq!(
            pixel_change_of(&event, Vec::new),
            Some(PixelChange { pages: vec![22], scope: InvalidateScope::Fields })
        );
        let event = json!({ "type": "signatures.completed", "signature": { "widget": null } });
        assert_eq!(pixel_change_of(&event, Vec::new), None);
    }

    #[test]
    fn flattens_are_content_facts_for_what_applied() {
        let result = |pon: u32, status: &str| json!({ "page": { "kind": "objectNumber", "objectNumber": pon }, "status": status });
        let event = json!({
            "type": "pages.flattened",
            "results": [result(11, "applied"), result(22, "failed"), result(11, "applied")],
        });
        assert_eq!(
            pixel_change_of(&event, Vec::new),
            Some(PixelChange { pages: vec![11], scope: InvalidateScope::Content })
        );
        let event = json!({
            "type": "annotations.flattened",
            "page": { "kind": "objectNumber", "objectNumber": 33 },
            "results": [{ "status": "skipped" }, { "status": "applied" }],
        });
        assert_eq!(
            pixel_change_of(&event, Vec::new),
            Some(PixelChange { pages: vec![33], scope: InvalidateScope::Content })
        );
        let event = json!({
            "type": "annotations.flattened",
            "page": { "kind": "objectNumber", "objectNumber": 33 },
            "results": [{ "status": "skipped" }],
        });
        assert_eq!(pixel_change_of(&event, Vec::new), None);
    }

    #[test]
    fn a_form_fact_counts_its_rows_and_a_deleted_widget_page() {
        let event = json!({
            "type": "forms.widgetDeleted",
            "meta": { "changedWidgets": [widget(11)] },
            "widgets": [widget(33), { "page": null }],
            "page": { "kind": "objectNumber", "objectNumber": 22 },
        });
        assert_eq!(
            pixel_change_of(&event, Vec::new),
            Some(PixelChange { pages: vec![11, 33, 22], scope: InvalidateScope::Fields })
        );
    }
}
