//! `frame.ts`: where a render item draws: its frame (the box a painter
//! draws into, turned like the annotation) and the engine's raster inside it.

use crate::object::Nullable;
use crate::rect::{normalize_deg, rect_center, rotate_point};
use crate::types::{Frame, Raster, Rect, RenderItem, Shape};

fn frame_rotation_of(geometry: &Shape, rot: Option<f64>) -> f64 {
    let turned_box = match geometry {
        Shape::Box(_) | Shape::Caret(_) => true,
        Shape::TextBox(s) => s.callout_line.is_null(),
        _ => false,
    };
    if turned_box { normalize_deg(rot.unwrap_or(0.0)) } else { 0.0 }
}

/// `rasterInFrame(frame, raster, rotation)`.
pub fn raster_in_frame(
    frame_box: Rect,
    frame_rotation: f64,
    raster: Rect,
    rotation: f64,
) -> Raster {
    let middle = rotate_point(rect_center(raster), rect_center(frame_box), -frame_rotation);
    Raster {
        box_: Rect {
            x: middle.x - raster.width / 2.0 - frame_box.x,
            y: middle.y - raster.height / 2.0 - frame_box.y,
            width: raster.width,
            height: raster.height,
        },
        rotation: normalize_deg(rotation - frame_rotation),
        order: Default::default(),
    }
}

/// `placed(item, scale)`: the item with its frame and raster.
pub fn placed(mut item: RenderItem, scale: f64) -> RenderItem {
    let rotation = frame_rotation_of(&item.geometry, item.rot);
    item.raster = Some(match item.ap_box {
        Some(ap) => {
            Nullable::Value(raster_in_frame(item.box_, rotation, ap, item.ap_rot.unwrap_or(0.0)))
        }
        None => Nullable::Null,
    });
    item.frame = Some(Frame { box_: item.box_, rotation, scale, order: Default::default() });
    item
}
