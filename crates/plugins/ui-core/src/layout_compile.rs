//! Part of `layout-compile.ts`: what `validate_layout` reads a layout with.
//! The edge a place is on, the bar it rides along, the layout's modes, and
//! the commands the layout makes itself.
//!
//! The rest of that file, which turns a layout into the toolbars a viewer
//! draws (`compileBar`, `barsOnEdge`, `menuOf`, `omittedCommandsOf` and the
//! stacks), is not ported: no vector covers it and no Rust host draws a
//! layout yet (README, "Not ported").

use crate::layout::bars_of;
use crate::value::JsValue;

/// The command of the mode with no bar of its own: viewing.
pub const VIEW_MODE_COMMAND: &str = "mode:view";

/// The command that opens a mode, and its tab.
pub fn mode_command_of(mode: &str) -> String {
    format!("mode:{mode}")
}

/// The command that opens and closes a panel, and its button.
pub fn panel_command_of(panel: &str) -> String {
    format!("panel:{panel}")
}

pub(crate) const EDGES: [&str; 4] = ["top", "bottom", "start", "end"];

/// The layout's bars, by id, in order: the ones that are objects
/// (`validate_layout` names the rest).
fn object_bars(data: &JsValue) -> Vec<(String, JsValue)> {
    bars_of(data).entries().into_iter().filter(|(_, bar)| bar.is_object()).collect()
}

fn is_one_of(value: &JsValue, names: &[&str]) -> bool {
    value.as_str().is_some_and(|s| names.contains(&s))
}

/// The edge a place is on, or `None` for a place that isn't an edge.
pub fn edge_of(place: Option<&JsValue>) -> Option<String> {
    match place? {
        JsValue::String(edge) => EDGES.contains(&&**edge).then(|| edge.to_string()),
        place if place.is_object() && place.has("edge") => place
            .get("edge")
            .filter(|edge| is_one_of(edge, &EDGES))
            .map(|edge| edge.as_str().unwrap_or_default().to_string()),
        _ => None,
    }
}

/// The bar a place rides along, or `None`. It is the member as written, so a
/// `with` that isn't a string is that value.
pub fn host_of(place: Option<&JsValue>) -> Option<JsValue> {
    place.filter(|place| place.is_object() && place.has("with")).and_then(|place| place.get("with"))
}

/// Every mode of the layout's bars, once each, in bar order.
pub fn modes_of(data: &JsValue) -> Vec<String> {
    let mut modes: Vec<String> = Vec::new();
    for (_, bar) in object_bars(data) {
        if let Some(JsValue::String(mode)) = bar.get("mode")
            && !mode.is_empty()
            && !modes.iter().any(|seen| **seen == *mode)
        {
            modes.push(mode.to_string());
        }
    }
    modes
}
