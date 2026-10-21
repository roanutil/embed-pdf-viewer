//! The pages a row names (`core/src/transfer/pageRefs.ts`).
//!
//! A page ref is found by where it sits, under a `page` key (an
//! annotation's, a ref's, a destination's): a field ref has the same shape,
//! `{ kind: 'objectNumber', objectNumber }`, and is never under `page`.

use serde_json::{Map, Value};

use crate::identity::{PageObjectNumber, PageRef};

/// `isPageRef`: exactly `kind` and `objectNumber`, the kind `objectNumber`.
fn page_ref_of(value: &Map<String, Value>) -> Option<PageRef> {
    if value.len() != 2 || value.get("kind").and_then(Value::as_str) != Some("objectNumber") {
        return None;
    }
    let number = value.get("objectNumber")?.as_u64()?;
    PageObjectNumber::new(u32::try_from(number).ok()?).ok().map(PageRef::new)
}

/// Every page `value` names: each page ref anywhere in it, in the order met
/// (`pageRefsIn`).
pub fn page_refs_in(value: &Value) -> Vec<PageRef> {
    fn visit(node: &Value, key: Option<&str>, found: &mut Vec<PageRef>) {
        match node {
            Value::Array(items) => items.iter().for_each(|item| visit(item, None, found)),
            Value::Object(fields) => {
                if key == Some("page")
                    && let Some(page) = page_ref_of(fields)
                {
                    found.push(page);
                    return;
                }
                for (name, child) in fields {
                    visit(child, Some(name), found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    visit(value, None, &mut found);
    found
}

/// A copy of `value` with each page ref anywhere in it replaced by
/// `map(page)` (`mapPageRefs`).
pub fn map_page_refs(value: Value, map: &dyn Fn(PageRef) -> PageRef) -> Value {
    fn visit(node: Value, key: Option<&str>, map: &dyn Fn(PageRef) -> PageRef) -> Value {
        match node {
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|item| visit(item, None, map)).collect())
            }
            Value::Object(fields) => {
                if key == Some("page")
                    && let Some(page) = page_ref_of(&fields)
                {
                    return serde_json::to_value(map(page)).unwrap_or(Value::Object(fields));
                }
                Value::Object(
                    fields
                        .into_iter()
                        .map(|(name, child)| {
                            let child = visit(child, Some(&name), map);
                            (name, child)
                        })
                        .collect(),
                )
            }
            other => other,
        }
    }
    visit(value, None, map)
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/pageRefs.test.ts`.

    use super::*;
    use serde_json::json;

    fn page(number: u32) -> PageRef {
        PageRef::new(PageObjectNumber::new(number).unwrap())
    }

    fn link() -> Value {
        json!({
            "ref": { "kind": "objectNumber", "page": page(3), "objectNumber": 10 },
            "target": { "kind": "goto", "destination": { "kind": "fit", "page": page(7) } },
            "rect": { "left": 0, "bottom": 0, "right": 1, "top": 1 },
        })
    }

    #[test]
    fn finds_every_page_ref_an_item_names_and_nothing_else() {
        assert_eq!(page_refs_in(&link()), vec![page(3), page(7)]);
    }

    #[test]
    fn replaces_every_page_ref_and_copies_the_rest() {
        let mapped = map_page_refs(link(), &|p| page(p.object_number().get() + 100));
        let mut expected = link();
        expected["ref"]["page"] = json!(page(103));
        expected["target"]["destination"]["page"] = json!(page(107));
        assert_eq!(mapped, expected);
    }
}
