//! `placement.ts`: the click-to-place icon kinds (note, file attachment).

use epdf_annotation_core::{AnnotationFlagsPatch, AnnotationProps, Size};
use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::repository::Wire;
use crate::repository::seam::{css_to_color, num_json};

/// `ICON_PLACE_SIZE`: the engine forces an icon's `/Rect` to 20x20.
pub const ICON_PLACE_SIZE: Size = Size { width: 20.0, height: 20.0 };

/// `isIconPlaceKind(subtype)`.
pub fn is_icon_place_kind(subtype: &str) -> bool {
    subtype == "text" || subtype == "file-attachment"
}

/// `{ print: true, ...flags }` as wire JSON.
fn seeded_flags(flags: Option<&AnnotationFlagsPatch>) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("print".into(), json!(true));
    if let Some(Value::Object(f)) = flags.map(|f| serde_json::to_value(f).unwrap_or_default()) {
        for (k, v) in f {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

/// `iconPlacementDraft(subtype, geom, defaults, flags, file)`: the engine
/// create draft for a placed icon annotation. A file attachment without a
/// file throws.
pub fn icon_placement_draft(
    subtype: &str,
    geom: Wire,
    defaults: &AnnotationProps,
    flags: Option<&AnnotationFlagsPatch>,
    file: Option<Value>,
) -> Result<Wire, Error> {
    let mut shared = geom;
    shared.insert("color".into(), css_to_color(&defaults.color));
    shared.insert("opacity".into(), num_json(defaults.opacity));
    shared.insert("flags".into(), seeded_flags(flags));
    let mut out = Wire::new();
    if subtype == "text" {
        out.insert("subtype".into(), json!("text"));
        out.insert("icon".into(), json!(defaults.icon.clone().unwrap_or_else(|| "comment".into())));
        out.extend(shared);
        return Ok(out);
    }
    let Some(file) = file else {
        return Err(Error::new("[annotation] a file-attachment placement requires a file payload"));
    };
    out.insert("subtype".into(), json!("file-attachment"));
    out.insert("icon".into(), json!(defaults.icon.clone().unwrap_or_else(|| "paperclip".into())));
    out.insert("file".into(), file);
    out.extend(shared);
    Ok(out)
}
