//! `destination.ts`: a destination as a reveal. A destination (ISO 32000-1
//! §12.3.2.2) says where on a page to look and how close; the stage's
//! reveal takes the same three knobs:
//!
//! - `rect`: what to look at, a point for `xyz`, a rect for `fitR`, the page
//!   for `fit` and the `fitB*` kinds;
//! - `zoom`: `'keep'`, `{level}`, `'fit'`, `'fit-width'` or `'fit-height'`;
//! - `anchor`: where it lands; `'keep'` is the specification's `null`, keep
//!   the current value on that axis.
//!
//! Destinations are in page space, the stage's own space, so nothing
//! converts. The `fitB*` kinds fit the page's content bounding box; the
//! whole page stands in for it.

use epdf_geometry::{Rect, Size};

use crate::contract::*;

fn keep_or_start(value: Option<f64>) -> RevealAnchorValue {
    RevealAnchorValue::Word(if value.is_some() { RevealWord::Start } else { RevealWord::Keep })
}

/// `revealOfDestination(destination, size)`: the reveal that goes where the
/// destination says, on a page of `size` points.
pub fn reveal_of_destination(destination: &PageDestination, size: Size) -> RevealOptions {
    let Size { width, height } = size;
    match *destination {
        PageDestination::Xyz { x, y, zoom, .. } => RevealOptions {
            behavior: None,
            // A null axis keeps the current camera value; the number fed in
            // for it is never read (the 'keep' anchor).
            rect: Some(Rect { x: x.unwrap_or(0.0), y: y.unwrap_or(0.0), width: 0.0, height: 0.0 }),
            anchor: Some(RevealAnchor { x: Some(keep_or_start(x)), y: Some(keep_or_start(y)) }),
            // A zoom of 0 means the same as null: keep the current zoom.
            zoom: Some(match zoom {
                Some(level) if level != 0.0 => RevealZoom::Level { level },
                _ => RevealZoom::Word(RevealZoomWord::Keep),
            }),
        },
        PageDestination::Fit { .. } => RevealOptions {
            zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
            ..Default::default()
        },
        PageDestination::FitB { .. } => RevealOptions {
            rect: Some(Rect { x: 0.0, y: 0.0, width, height }),
            zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
            ..Default::default()
        },
        PageDestination::FitH { y, .. } | PageDestination::FitBh { y, .. } => RevealOptions {
            rect: Some(Rect { x: 0.0, y: y.unwrap_or(0.0), width, height: 0.0 }),
            zoom: Some(RevealZoom::Word(RevealZoomWord::FitWidth)),
            anchor: Some(RevealAnchor { x: None, y: Some(keep_or_start(y)) }),
            behavior: None,
        },
        PageDestination::FitV { x, .. } | PageDestination::FitBv { x, .. } => RevealOptions {
            rect: Some(Rect { x: x.unwrap_or(0.0), y: 0.0, width: 0.0, height }),
            zoom: Some(RevealZoom::Word(RevealZoomWord::FitHeight)),
            anchor: Some(RevealAnchor { x: Some(keep_or_start(x)), y: None }),
            behavior: None,
        },
        PageDestination::FitR { x, y, width: rect_width, height: rect_height, .. } => {
            RevealOptions {
                rect: Some(Rect { x, y, width: rect_width, height: rect_height }),
                zoom: Some(RevealZoom::Word(RevealZoomWord::Fit)),
                ..Default::default()
            }
        }
    }
}
