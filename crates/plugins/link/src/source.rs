//! `source.ts`: the engine-backed link source, for viewer-only deployments
//! (no annotation plugin). A page's annotation list keeps just the
//! navigable links, standalone and attached children alike. The
//! capability's lazy `ensure_page` and the effects' event-driven refetch
//! share it; both hand it the plugin's typed context, the TypeScript's
//! structural `io` slice.

use epdf_annotation_core::{AnnotationRef, pdf_to_content_rect};
use epdf_kernel::{Ctx, Error};
use epdf_plugin_annotation::promise::then;
use epdf_plugin_annotation::value::{get, pdf_rect_of, str_of, truthy};
use serde_json::Value;

use crate::types::{HoverFlags, LinkAction, LinkNavItem, LinkState, PageObjectNumber};

/// `String(value)` inside a template literal, for the members an id is
/// built from. Absence is `undefined`; callers read members with `get`,
/// which already turns the `undefined` marker into absence.
pub fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => {
            epdf_geometry::js::number_to_string(n.as_f64().unwrap_or(f64::NAN))
        }
        Some(Value::String(s)) => s.clone(),
        // `Array.prototype.join`: `null` and `undefined` elements print empty.
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| if item.is_null() { String::new() } else { js_string(Some(item)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(o)) => match o.get("$number").and_then(Value::as_str) {
            Some(spelled) => spelled.to_string(),
            None => "[object Object]".into(),
        },
    }
}

/// The page's crop box from the document's page list, when it has the page.
fn crop_of(ctx: &Ctx<LinkState, LinkAction>, pon: PageObjectNumber) -> Option<Value> {
    let document = ctx.document()?;
    let pages = document.pages.as_array()?;
    let page = pages
        .iter()
        .find(|p| p.get("pageObjectNumber").and_then(Value::as_f64) == Some(f64::from(pon)))?;
    Some(page.get("boxes").and_then(|b| get(b, "crop")).cloned().unwrap_or(Value::Null))
}

/// The fulfilled handler's loop. `None` where the TypeScript would throw
/// (a missing `annotations` list, a `null` entry, a link without `flags`,
/// `ref` or `rect`): the throw rejects the derived promise, which nothing
/// observes, so nothing is dispatched.
pub fn project_links(
    snap: &Value,
    pon: PageObjectNumber,
    crop: &Value,
) -> Option<Vec<LinkNavItem>> {
    let crop = pdf_rect_of(Some(crop));
    let mut items = Vec::new();
    for dto in get(snap, "annotations")?.as_array()? {
        if !dto.is_object() {
            return None;
        }
        let target = get(dto, "target");
        if str_of(dto, "subtype") != Some("link") || target.is_none_or(Value::is_null) {
            continue;
        }
        let flags = get(dto, "flags").filter(|f| !f.is_null())?;
        if truthy(get(flags, "hidden")) || truthy(get(flags, "noView")) {
            continue;
        }
        let r = get(dto, "ref").filter(|r| !r.is_null())?;
        let id = if str_of(r, "kind") == Some("objectNumber") {
            format!("obj:{}", js_string(get(r, "annotObjectNumber")))
        } else {
            format!("idx:{pon}:{}", js_string(get(dto, "index")))
        };
        let rect = get(dto, "rect").filter(|r| !r.is_null())?;
        let actions = get(dto, "actions").filter(|a| !a.is_null());
        let member = |key: &str| actions.and_then(|a| get(a, key)).filter(|v| !v.is_null());
        let activate = member("activate").filter(|a| truthy(Some(a))).cloned();
        let enter = truthy(member("cursorEnter").and_then(|c| get(c, "root")));
        let exit = truthy(member("cursorExit").and_then(|c| get(c, "root")));
        items.push(LinkNavItem {
            id,
            rect: pdf_to_content_rect(pdf_rect_of(Some(rect)), crop),
            target: target.cloned().unwrap_or(Value::Null),
            attached: str_of(dto, "replyType") == Some("group")
                && get(dto, "inReplyTo").is_some_and(|v| !v.is_null()),
            activate,
            ref_: serde_json::from_value::<AnnotationRef>(r.clone()).ok(),
            hover_events: (enter || exit).then_some(HoverFlags { enter, exit }),
        });
    }
    Some(items)
}

/// `loadLinksPage(io, pon)`: list the page and cache its links. Nothing
/// happens without a document handle or a page of that number; a refused
/// list is swallowed. `Err` is `doc.page(pon)` throwing for a page the
/// engine does not have.
pub fn load_links_page(
    ctx: &Ctx<LinkState, LinkAction>,
    pon: PageObjectNumber,
) -> Result<(), Error> {
    let doc = ctx.doc();
    let crop = crop_of(ctx, pon);
    let (Some(doc), Some(crop)) = (doc, crop) else { return Ok(()) };
    doc.page_index(pon)?;
    let call = doc.call("page.annotations.list", Some(pon), Vec::new());
    let ctx = ctx.clone();
    then(call, move |result| {
        if let Ok(snap) = result
            && let Some(items) = project_links(&snap, pon, &crop)
        {
            ctx.dispatch(LinkAction::SetPage { pon, items });
        }
    });
    Ok(())
}
