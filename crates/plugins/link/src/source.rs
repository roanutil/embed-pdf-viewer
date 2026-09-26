//! `source.ts`: the stand-alone source (no annotation plugin). One
//! annotations read per page, folded to the clickable areas.

use epdf_annotation_core::{AnnotationRef, PageObjectNumber, PageRef, pdf_to_content_rect};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error};
use epdf_plugin_annotation::promise::{await_engine, spawn};
use epdf_plugin_annotation::value::{get, pdf_rect_of, str_of, truthy};
use serde_json::Value;

use crate::contract::{HoverFlags, Link};
use crate::model::{LinkAction, LinkState};

/// `String(value)` inside a template literal, for the members an id is
/// built from. Absence is `undefined`.
pub fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => {
            epdf_geometry::js::number_to_string(n.as_f64().unwrap_or(f64::NAN))
        }
        Some(Value::String(s)) => s.clone(),
        // `Array.prototype.join`: `null` elements print empty.
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

/// The `TypeError` reading `member` off a missing (`None`) or `null` value
/// throws.
fn cannot_read(value: Option<&Value>, member: &str) -> Error {
    let what = if value.is_some() { "null" } else { "undefined" };
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{member}')"))
}

/// `value` when it is neither missing nor `null`, else the `TypeError`
/// reading `member` off it throws.
fn object_or_throw<'a>(value: Option<&'a Value>, member: &str) -> Result<&'a Value, Error> {
    match value {
        Some(v) if !v.is_null() => Ok(v),
        other => Err(cannot_read(other, member)),
    }
}

/// The loop over `snap.annotations`. `Err` is where the TypeScript throws
/// (a list that is not iterable, a `null` entry, a link without `flags`,
/// `ref` or `rect`), which rejects `loadLinksPage` before it stores
/// anything.
pub fn project_links(
    snap: &Value,
    pon: PageObjectNumber,
    crop: &Value,
) -> Result<Vec<Link>, Error> {
    let snap = object_or_throw(Some(snap), "annotations")?;
    let Some(Value::Array(annotations)) = get(snap, "annotations") else {
        return Err(Error::named("TypeError", "snap.annotations is not iterable"));
    };
    let crop = pdf_rect_of(Some(crop));
    let mut items = Vec::new();
    for dto in annotations {
        let dto = object_or_throw(Some(dto), "subtype")?;
        let target = get(dto, "target");
        if str_of(dto, "subtype") != Some("link") || target.is_none_or(Value::is_null) {
            continue;
        }
        let flags = object_or_throw(get(dto, "flags"), "hidden")?;
        if truthy(get(flags, "hidden")) || truthy(get(flags, "noView")) {
            continue;
        }
        let r = object_or_throw(get(dto, "ref"), "kind")?;
        let id = if str_of(r, "kind") == Some("objectNumber") {
            format!("obj:{}", js_string(get(r, "annotObjectNumber")))
        } else {
            format!("idx:{pon}:{}", js_string(get(dto, "index")))
        };
        let rect = object_or_throw(get(dto, "rect"), "left")?;
        let actions = get(dto, "actions").filter(|a| !a.is_null());
        let member = |key: &str| actions.and_then(|a| get(a, key)).filter(|v| !v.is_null());
        let activate = member("activate").filter(|a| truthy(Some(a))).cloned();
        let enter = truthy(member("cursorEnter").and_then(|c| get(c, "root")));
        let exit = truthy(member("cursorExit").and_then(|c| get(c, "root")));
        items.push(Link {
            id,
            bounds: pdf_to_content_rect(pdf_rect_of(Some(rect)), crop),
            target: target.cloned().unwrap_or(Value::Null),
            attached: str_of(dto, "replyType") == Some("group")
                && get(dto, "inReplyTo").is_some_and(|v| !v.is_null()),
            activate,
            ref_: serde_json::from_value::<AnnotationRef>(r.clone()).ok(),
            hover_events: (enter || exit).then_some(HoverFlags { enter, exit }),
        });
    }
    Ok(items)
}

/// The page's crop box from the document's page list, when it has the page
/// and the page has one.
fn crop_of(cx: &Controller<LinkState, LinkAction>, page: &PageRef) -> Option<Value> {
    let document = cx.document()?;
    let layout = document.pages.as_array()?.iter().find(|p| {
        p.get("ref").and_then(|r| r.get("pageObjectNumber")).and_then(Value::as_u64)
            == Some(u64::from(page.page_object_number))
    })?;
    layout.get("boxes").and_then(|b| get(b, "crop")).filter(|c| truthy(Some(c))).cloned()
}

/// `loadLinksPage(io, page)`: list the page and store its links. Resolves
/// with the items it stored, empty (and storing nothing) without a handle
/// or a page of that number, and when the read fails.
pub fn load_links_page(
    cx: &Controller<LinkState, LinkAction>,
    page: PageRef,
) -> Promise<Vec<Link>> {
    let cx = cx.clone();
    spawn(async move {
        let pon = page.page_object_number;
        let (Ok(doc), Some(crop)) = (cx.doc(), crop_of(&cx, &page)) else {
            return Ok(Vec::new());
        };
        // `doc.page(page)` throws for a page the engine lacks, inside the
        // `try`.
        if doc.page_index(pon).is_err() {
            return Ok(Vec::new());
        }
        let Ok(snap) = await_engine(doc.call("page.annotations.list", Some(pon), Vec::new())).await
        else {
            return Ok(Vec::new());
        };
        let items = project_links(&snap, pon, &crop)?;
        cx.dispatch(LinkAction::SetPage { page, items: items.clone() });
        Ok(items)
    })
}
