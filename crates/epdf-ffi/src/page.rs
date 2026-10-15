//! A page as `pages.list` and every page mutation's `layout` describe it.
//! Here rather than in `epdf-viewer-host` because the page-edit and stage
//! plugins return it too.

use epdf_kernel::Error;
use serde_json::Value;

use crate::PageRef;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageInfo {
    pub page: PageRef,
    pub index: u32,
    pub width: f64,
    pub height: f64,
    /// Degrees clockwise: 0, 90, 180 or 270.
    pub rotation: u32,
}

fn malformed(what: String) -> Error {
    Error::named("InvalidState", format!("[epdf-ffi] {what}")).with("code", "invalid-state")
}

fn whole(value: &Value, field: &str, what: &str) -> Result<u32, Error> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| malformed(format!("{what} without a whole \"{field}\": {value}")))
}

fn number(value: &Value, field: &str, what: &str) -> Result<f64, Error> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| malformed(format!("{what} without \"{field}\": {value}")))
}

/// A `PageLayout`: `ref`, `index`, `size` and `rotation`.
pub fn decode_page(page: &Value) -> Result<PageInfo, Error> {
    let size =
        page.get("size").ok_or_else(|| malformed(format!("a page without \"size\": {page}")))?;
    let reference =
        page.get("ref").ok_or_else(|| malformed(format!("a page without \"ref\": {page}")))?;
    Ok(PageInfo {
        page: PageRef { page_object_number: whole(reference, "pageObjectNumber", "a page ref")? },
        index: whole(page, "index", "a page")?,
        width: number(size, "width", "a page size")?,
        height: number(size, "height", "a page size")?,
        rotation: whole(page, "rotation", "a page")?,
    })
}

pub fn decode_pages(pages: &[Value]) -> Result<Vec<PageInfo>, Error> {
    pages.iter().map(decode_page).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_layout_page_decodes_to_page_info() {
        let page = json!({
            "ref": { "kind": "objectNumber", "pageObjectNumber": 7 }, "index": 2, "rotation": 90,
            "size": { "width": 612.0, "height": 792.0 }
        });
        assert_eq!(
            decode_page(&page).unwrap(),
            PageInfo {
                page: PageRef { page_object_number: 7 },
                index: 2,
                width: 612.0,
                height: 792.0,
                rotation: 90,
            }
        );
    }

    #[test]
    fn a_page_without_its_size_is_invalid_state() {
        let page = json!({
            "ref": { "kind": "objectNumber", "pageObjectNumber": 7 }, "index": 2, "rotation": 0
        });
        assert_eq!(decode_page(&page).unwrap_err().code(), Some("invalid-state"));
    }
}
