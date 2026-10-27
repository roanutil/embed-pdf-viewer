//! `write/next-in-line.ts`: where a field's new widget goes when
//! `addWidget()` gets no place for it: next in line after the field's
//! widgets on the page. A column grows downward and a row sideways, each
//! step the same as the one between the last two; one button gets the next
//! below it, half its height away. Where the next step leaves the page, the
//! line starts again beside the group (a column) or under it (a row). Page
//! space: top-left origin, y down.

use epdf_geometry::js::{max, min};

use crate::model::Box;

fn fits(bx: &Box, width: f64, height: f64) -> bool {
    bx.x >= 0.0 && bx.y >= 0.0 && bx.x + bx.width <= width && bx.y + bx.height <= height
}

/// `nextInLine(boxes, page)`: the box after `boxes`, the field's widgets on
/// one page in the field's order.
pub fn next_in_line(boxes: &[Box], page_width: f64, page_height: f64) -> Box {
    let Some(last) = boxes.last() else { return Box::default() };
    let (width, height) = (last.width, last.height);
    let previous = if boxes.len() >= 2 { boxes.get(boxes.len() - 2) } else { None };
    // The step between the last two, or half a button's height below the only one.
    let (step_x, step_y) = match previous {
        Some(p) if p.x != last.x || p.y != last.y => (last.x - p.x, last.y - p.y),
        _ => (0.0, height * 1.5),
    };
    let next = Box { x: last.x + step_x, y: last.y + step_y, width, height };
    if fits(&next, page_width, page_height) {
        return next;
    }
    let column = step_y.abs() >= step_x.abs();
    let fold = |init: f64, f: fn(f64, f64) -> f64, of: &dyn Fn(&Box) -> f64| {
        boxes.iter().fold(init, |acc, b| f(acc, of(b)))
    };
    let left = fold(f64::INFINITY, min, &|b| b.x);
    let top = fold(f64::INFINITY, min, &|b| b.y);
    let right = fold(f64::NEG_INFINITY, max, &|b| b.x + b.width);
    let bottom = fold(f64::NEG_INFINITY, max, &|b| b.y + b.height);
    // The gap between two buttons along the line: the new line keeps it.
    let gap = max(if column { step_y.abs() - height } else { step_x.abs() - width }, 0.0);
    let or = |gap: f64, fallback: f64| if gap == 0.0 || gap.is_nan() { fallback } else { gap };
    let restart = if column {
        Box { x: right + or(gap, width / 2.0), y: top, width, height }
    } else {
        Box { x: left, y: bottom + or(gap, height / 2.0), width, height }
    };
    if fits(&restart, page_width, page_height) {
        return restart;
    }
    // No room beside the group either: on the page, at the last button's
    // place shifted in.
    Box {
        x: min(max(next.x, 0.0), max(page_width - width, 0.0)),
        y: min(max(next.y, 0.0), max(page_height - height, 0.0)),
        width,
        height,
    }
}
