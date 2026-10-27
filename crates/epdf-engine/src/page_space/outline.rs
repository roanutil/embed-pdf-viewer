//! The outline between the file's coordinates and page space
//! (`core/src/pageSpace/outline.ts`): only the destinations an item goes to
//! change, each measured on the page it goes to.
//!
//! Items, drafts and patches stay the JSON they cross the wire as, so one
//! value holds either space, as the annotation and form ones do.

use serde_json::{Map, Value};

use super::VisibleBoxOf;
use super::destinations::{page_action_tree_of, page_destination_of, pdf_destination_value_of};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::actions::reader::ActionTree;

fn malformed(error: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::WireFormat,
        format!("an outline item is not the shape it takes: {error}"),
    )
}

/// The outline as a read hands it out, every destination in page space
/// (`pageOutlineOf`).
pub fn page_outline_of(
    outline: Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Value, EngineError> {
    let Value::Object(mut outline) = outline else {
        return Err(malformed("an outline is an object"));
    };
    if let Some(Value::Array(items)) = outline.remove("items") {
        let items = items
            .into_iter()
            .map(|item| page_outline_item_of(item, box_of))
            .collect::<Result<_, _>>()?;
        outline.insert("items".into(), Value::Array(items));
    }
    Ok(Value::Object(outline))
}

/// An item, its target's and its action's destinations in page space
/// (`pageOutlineItemOf`, `mapOutlineItem`).
pub fn page_outline_item_of(
    item: Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Value, EngineError> {
    let Value::Object(mut item) = item else {
        return Err(malformed("an item is an object"));
    };
    if let Some(Value::Object(target)) = item.get_mut("target")
        && target.get("kind").and_then(Value::as_str) == Some("goto")
        && let Some(destination) = target.remove("destination")
    {
        let destination: PdfDestination = serde_json::from_value(destination).map_err(malformed)?;
        let converted = page_destination_of(destination, box_of)?;
        target.insert("destination".into(), serde_json::to_value(converted).map_err(malformed)?);
    }
    if let Some(action) = item.get_mut("action").filter(|action| !action.is_null()) {
        let tree: ActionTree<PdfDestination> =
            serde_json::from_value(action.take()).map_err(malformed)?;
        *action = serde_json::to_value(page_action_tree_of(tree, box_of)?).map_err(malformed)?;
    }
    Ok(Value::Object(item))
}

/// A draft in the file's coordinates, as the engine writes it
/// (`pdfOutlineDraftOf`).
pub fn pdf_outline_draft_of(
    draft: Map<String, Value>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Map<String, Value>, EngineError> {
    pdf_outline_write_of(draft, box_of)
}

/// A patch, or what an op expects, in the file's coordinates
/// (`pdfOutlinePatchOf`).
pub fn pdf_outline_patch_of(
    patch: Map<String, Value>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Map<String, Value>, EngineError> {
    pdf_outline_write_of(patch, box_of)
}

/// `mapOutlineWrite`: a `goto` target's destination converted, an axis it
/// leaves out staying out; everything else as sent.
fn pdf_outline_write_of(
    mut write: Map<String, Value>,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Map<String, Value>, EngineError> {
    if let Some(Value::Object(target)) = write.get_mut("target")
        && target.get("kind").and_then(Value::as_str) == Some("goto")
        && let Some(destination) = target.remove("destination")
    {
        target.insert("destination".into(), pdf_destination_value_of(destination, box_of)?);
    }
    Ok(write)
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/src/pageSpace/outline.ts`'s conversions, on a
    //! page whose visible box starts at (0, 0) and one that doesn't.

    use serde_json::json;

    use super::*;
    use crate::geom::PdfRect;
    use crate::identity::PageObjectNumber;

    fn box_of(page: PageObjectNumber) -> Result<PdfRect, EngineError> {
        Ok(match page.get() {
            3 => PdfRect { left: 0.0, bottom: 0.0, right: 300.0, top: 300.0 },
            4 => PdfRect { left: 10.0, bottom: 20.0, right: 310.0, top: 320.0 },
            other => panic!("no page {other}"),
        })
    }

    fn page(n: u32) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": n })
    }

    #[test]
    fn an_item_converts_its_target_and_its_action_on_the_page_each_goes_to() {
        let goto = |page_number: u32, top: f64| json!({ "kind": "xyz", "page": page(page_number), "left": 20.0, "top": top, "zoom": null });
        let item = json!({
            "ref": { "kind": "objectNumber", "objectNumber": 6 },
            "parent": null,
            "title": "A",
            "target": { "kind": "goto", "destination": goto(4, 280.0) },
            "action": { "root": { "type": "goto", "subtype": "GoTo", "next": [],
                "destination": goto(3, 280.0) }, "incomplete": false, "warningFlags": 0,
                "warnings": [] },
            "open": false, "color": null, "bold": false, "italic": false,
        });
        let converted = page_outline_item_of(item, &mut box_of).unwrap();
        assert_eq!(
            converted["target"]["destination"],
            json!({ "kind": "xyz", "page": page(4), "x": 10.0, "y": 40.0, "zoom": null })
        );
        assert_eq!(converted["action"]["root"]["destination"]["y"], 20.0);
        assert_eq!(converted["title"], "A");
    }

    #[test]
    fn an_item_without_a_place_comes_back_as_it_was() {
        let item = json!({ "ref": { "kind": "objectNumber", "objectNumber": 7 }, "parent": null,
            "title": "B", "target": { "kind": "uri", "uri": "https://example.com/" },
            "action": null, "open": false, "color": "#ff0000", "bold": true, "italic": false });
        assert_eq!(page_outline_item_of(item.clone(), &mut box_of).unwrap(), item);
        let outline = json!({ "items": [item.clone()], "incomplete": true });
        assert_eq!(page_outline_of(outline.clone(), &mut box_of).unwrap(), outline);
    }

    #[test]
    fn a_write_converts_its_goto_and_keeps_an_axis_it_left_out_out() {
        let patch = json!({ "title": "x", "target": { "kind": "goto",
            "destination": { "kind": "xyz", "page": page(4), "x": 10.0 } } });
        let Value::Object(patch) = patch else { unreachable!() };
        let converted = pdf_outline_patch_of(patch, &mut box_of).unwrap();
        assert_eq!(
            converted["target"]["destination"],
            json!({ "kind": "xyz", "page": page(4), "left": 20.0 })
        );
        let draft = json!({ "title": "y", "target": { "kind": "goto",
            "destination": { "kind": "fitH", "page": page(3), "y": null } } });
        let Value::Object(draft) = draft else { unreachable!() };
        let converted = pdf_outline_draft_of(draft, &mut box_of).unwrap();
        assert_eq!(
            converted["target"]["destination"],
            json!({ "kind": "fitH", "page": page(3), "top": null })
        );
        let Value::Object(cleared) = json!({ "target": null, "color": null }) else {
            unreachable!()
        };
        assert_eq!(pdf_outline_patch_of(cleared.clone(), &mut box_of).unwrap(), cleared);
    }
}
