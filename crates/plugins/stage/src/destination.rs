//! `destination.ts`: PDF destination to reveal, the pure translator
//! between the PDF protocol's navigation vocabulary (ISO 32000-1
//! §12.3.2.2) and the stage's arrival primitive. Outline clicks, link
//! annotations and `/OpenAction` all resolve engine-side to an explicit
//! [`PdfDestination`]; this maps it onto one `reveal(page_index, options)`.
//!
//! This is the `/destination` entry: link and search use it without the
//! capability.

use epdf_geometry::{
    CropBox, NonInvertible, PageGeometryInput, PageRotation, Point, Rect, Size, apply_point, js,
    page_geometry,
};
use serde::{Deserialize, Serialize};

use crate::types::*;

/// `PageBoxes`: the PDF page boundary boxes in user space (y-up edges).
/// `bleed`, `trim` and `art` are not read here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageBoxes {
    pub media: CropBox,
    pub crop: CropBox,
}

/// `PageLayout`: one page as the engine reports it (`pages.list`). Page
/// actions and the optional boxes are not read by the stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayout {
    /// Display order at read time.
    pub index: usize,
    /// Durable identity.
    pub page_object_number: PageObjectNumber,
    #[serde(default)]
    pub label: Option<String>,
    /// Un-rotated crop dimensions in PDF points.
    pub size: Size,
    #[serde(default)]
    pub rotation: PageRotation,
    /// `/UserUnit`. The TypeScript reads an absent one as 1 where it
    /// defaults it (`?? 1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_unit: Option<f64>,
    #[serde(default)]
    pub boxes: PageBoxes,
}

/// `PdfDestination`: an explicit destination. Coordinates are PDF user
/// space (y-up, absolute); `None` means "retain the current value", and an
/// `/XYZ` zoom of 0 means the same.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PdfDestination {
    #[serde(rename = "xyz", rename_all = "camelCase")]
    Xyz {
        page_object_number: PageObjectNumber,
        #[serde(default)]
        left: Option<f64>,
        #[serde(default)]
        top: Option<f64>,
        #[serde(default)]
        zoom: Option<f64>,
    },
    #[serde(rename = "fit", rename_all = "camelCase")]
    Fit { page_object_number: PageObjectNumber },
    #[serde(rename = "fitH", rename_all = "camelCase")]
    FitH {
        page_object_number: PageObjectNumber,
        #[serde(default)]
        top: Option<f64>,
    },
    #[serde(rename = "fitV", rename_all = "camelCase")]
    FitV {
        page_object_number: PageObjectNumber,
        #[serde(default)]
        left: Option<f64>,
    },
    #[serde(rename = "fitR", rename_all = "camelCase")]
    FitR { page_object_number: PageObjectNumber, left: f64, bottom: f64, right: f64, top: f64 },
    #[serde(rename = "fitB", rename_all = "camelCase")]
    FitB { page_object_number: PageObjectNumber },
    #[serde(rename = "fitBH", rename_all = "camelCase")]
    FitBh {
        page_object_number: PageObjectNumber,
        #[serde(default)]
        top: Option<f64>,
    },
    #[serde(rename = "fitBV", rename_all = "camelCase")]
    FitBv {
        page_object_number: PageObjectNumber,
        #[serde(default)]
        left: Option<f64>,
    },
}

impl PdfDestination {
    pub fn page_object_number(&self) -> PageObjectNumber {
        match *self {
            PdfDestination::Xyz { page_object_number, .. }
            | PdfDestination::Fit { page_object_number }
            | PdfDestination::FitH { page_object_number, .. }
            | PdfDestination::FitV { page_object_number, .. }
            | PdfDestination::FitR { page_object_number, .. }
            | PdfDestination::FitB { page_object_number }
            | PdfDestination::FitBh { page_object_number, .. }
            | PdfDestination::FitBv { page_object_number, .. } => page_object_number,
        }
    }
}

/// Drop the members a JSON value carries as `undefined` (the kernel's
/// `{"$undefined": true}`), as a JavaScript object read would see them:
/// absent. Engine DTOs reach the stage as JSON and may carry the marker.
pub fn without_undefined(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !epdf_kernel::is_undefined(v))
                .map(|(k, v)| (k.clone(), without_undefined(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_undefined).collect()),
        other => other.clone(),
    }
}

/// A destination translated for `reveal`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationReveal {
    /// Display index, from the layout row.
    pub page_index: usize,
    pub options: RevealOptions,
}

fn anchor_word(present: bool, word: RevealWord) -> RevealAnchorValue {
    RevealAnchorValue::Word(if present { word } else { RevealWord::Keep })
}

/// Translate one explicit destination against its target page's layout.
/// `bbox` is the page's content bounding box in content space, for the
/// `/FitB*` kinds; the page is the fallback until the engine exposes it.
///
/// Fails where the TypeScript throws: `pageGeometry` inverts the page's
/// PDF-to-view matrix, which a `userUnit` of 0 makes singular.
pub fn destination_to_reveal(
    dest: &PdfDestination,
    layout: &PageLayout,
    bbox: Option<Rect>,
) -> Result<DestinationReveal, NonInvertible> {
    let crop = layout.boxes.crop;
    // An absent `userUnit` multiplies to NaN, as `zoom * undefined` does.
    let geo = page_geometry(
        PageGeometryInput {
            crop,
            rotation: layout.rotation,
            user_unit: layout.user_unit.unwrap_or(f64::NAN),
        },
        1.0,
    )?;
    let pdf_to_content = geo.pdf_to_content;
    let to_content = |x: f64, y: f64| apply_point(pdf_to_content, Point { x, y });
    let page = Rect { x: 0.0, y: 0.0, width: layout.size.width, height: layout.size.height };
    let bx = bbox.unwrap_or(page);
    let point = |p: Point| Rect { x: p.x, y: p.y, width: 0.0, height: 0.0 };

    let options = match *dest {
        PdfDestination::Xyz { left, top, zoom, .. } => {
            let p = to_content(left.unwrap_or(crop.left), top.unwrap_or(crop.top));
            RevealOptions {
                rect: Some(point(p)),
                anchor: Some(RevealAnchor {
                    x: Some(anchor_word(left.is_some(), RevealWord::Start)),
                    y: Some(anchor_word(top.is_some(), RevealWord::Start)),
                }),
                zoom: Some(match zoom {
                    Some(level) if level != 0.0 => RevealZoom::Level { level },
                    _ => RevealZoom::Word(RevealZoomWord::Keep),
                }),
                behavior: None,
            }
        }
        PdfDestination::Fit { .. } => RevealOptions {
            zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
            ..Default::default()
        },
        PdfDestination::FitH { top, .. } => {
            let y = top.map_or(0.0, |top| to_content(crop.left, top).y);
            RevealOptions {
                rect: Some(Rect { x: 0.0, y, width: page.width, height: 0.0 }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::FitWidth)),
                anchor: Some(RevealAnchor {
                    x: None,
                    y: Some(anchor_word(top.is_some(), RevealWord::Start)),
                }),
                behavior: None,
            }
        }
        PdfDestination::FitV { left, .. } => {
            let x = left.map_or(0.0, |left| to_content(left, crop.top).x);
            RevealOptions {
                rect: Some(Rect { x, y: 0.0, width: 0.0, height: page.height }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::FitHeight)),
                anchor: Some(RevealAnchor {
                    x: Some(anchor_word(left.is_some(), RevealWord::Start)),
                    y: None,
                }),
                behavior: None,
            }
        }
        PdfDestination::FitR { left, bottom, right, top, .. } => {
            let tl = to_content(left, top);
            let br = to_content(right, bottom);
            RevealOptions {
                rect: Some(Rect {
                    x: js::min(tl.x, br.x),
                    y: js::min(tl.y, br.y),
                    width: (br.x - tl.x).abs(),
                    height: (br.y - tl.y).abs(),
                }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
                ..Default::default()
            }
        }
        PdfDestination::FitB { .. } => RevealOptions {
            rect: Some(bx),
            zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
            ..Default::default()
        },
        PdfDestination::FitBh { top, .. } => {
            let y = top.map_or(bx.y, |top| to_content(crop.left, top).y);
            RevealOptions {
                rect: Some(Rect { x: bx.x, y, width: bx.width, height: 0.0 }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::FitWidth)),
                anchor: Some(RevealAnchor {
                    x: None,
                    y: Some(anchor_word(top.is_some(), RevealWord::Start)),
                }),
                behavior: None,
            }
        }
        PdfDestination::FitBv { left, .. } => {
            let x = left.map_or(bx.x, |left| to_content(left, crop.top).x);
            RevealOptions {
                rect: Some(Rect { x, y: bx.y, width: 0.0, height: bx.height }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::FitHeight)),
                anchor: Some(RevealAnchor {
                    x: Some(anchor_word(left.is_some(), RevealWord::Start)),
                    y: None,
                }),
                behavior: None,
            }
        }
    };
    Ok(DestinationReveal { page_index: layout.index, options })
}
