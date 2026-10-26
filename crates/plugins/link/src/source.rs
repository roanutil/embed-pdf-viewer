//! `source.ts`: the stand-alone source, used when no annotation plugin is
//! installed: the clickable areas among one page's annotations, in page
//! space.

use epdf_annotation_core::json::truthy;
use epdf_annotation_core::record::annotation_key;
use epdf_kernel::{Error, undefined};
use serde_json::{Map, Value, json};

use crate::contract::Link;

/// `dto.member`, `undefined` when absent.
fn member<'a>(dto: &'a Value, key: &str) -> Option<&'a Value> {
    dto.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

/// `value?.member`.
fn optional<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.filter(|v| !v.is_null()).and_then(|v| member(v, key))
}

/// The `TypeError` reading `key` off a `null` or `undefined` element.
fn cannot_read(value: &Value, key: &str) -> Error {
    let what = if value.is_null() { "null" } else { "undefined" };
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{key}')"))
}

/// `linksOf(annotations)`: the visible link annotations of a page that have
/// a target, in the member order `source.ts` writes. `Err` is where the
/// TypeScript throws: a list that is not iterable, or a missing element.
pub fn links_of(annotations: &Value) -> Result<Vec<Link>, Error> {
    let Value::Array(annotations) = annotations else {
        return Err(Error::named("TypeError", "annotations is not iterable"));
    };
    let mut links = Vec::new();
    for dto in annotations {
        if dto.is_null() || epdf_kernel::is_undefined(dto) {
            return Err(cannot_read(dto, "subtype"));
        }
        let target = member(dto, "target");
        if member(dto, "subtype").and_then(Value::as_str) != Some("link")
            || target.is_none_or(Value::is_null)
        {
            continue;
        }
        if truthy(member(dto, "hidden")) || truthy(member(dto, "noView")) {
            continue;
        }
        let reference = member(dto, "ref");
        // `annotationKey(dto.ref)` reads `ref.kind` first.
        let id = match reference {
            Some(r) if !r.is_null() => annotation_key(r).map_or_else(undefined, Value::String),
            other => return Err(cannot_read(other.unwrap_or(&undefined()), "kind")),
        };
        let actions = member(dto, "actions");
        let activate = optional(actions, "activate").filter(|a| truthy(Some(a)));
        let enter = truthy(optional(optional(actions, "cursorEnter"), "root"));
        let exit = truthy(optional(optional(actions, "cursorExit"), "root"));
        let mut link = Map::new();
        link.insert("id".into(), id);
        link.insert("bounds".into(), member(dto, "rect").cloned().unwrap_or_else(undefined));
        link.insert("target".into(), target.cloned().unwrap_or(Value::Null));
        if let Some(activate) = activate {
            link.insert("activate".into(), activate.clone());
        }
        if enter || exit {
            link.insert("hoverEvents".into(), json!({ "enter": enter, "exit": exit }));
        }
        link.insert("ref".into(), reference.cloned().unwrap_or(Value::Null));
        let grouped =
            optional(member(dto, "reply"), "type").and_then(Value::as_str) == Some("group");
        link.insert("attached".into(), json!(grouped));
        links.push(Value::Object(link));
    }
    Ok(links)
}
