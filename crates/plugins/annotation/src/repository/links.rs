//! `repository/links.ts`: the desired hit rects of a parent's attached link
//! children, derived from its committed geometry.

use epdf_annotation_core::{Annot, Geom, Rect, selection_quad, union_rect};
use epdf_geometry::text_quad_bounds;

/// `linkChildRects(a)`: one rect per markup quad, else one over the
/// visual (rotated) footprint.
pub fn link_child_rects(a: &Annot) -> Vec<Rect> {
    if let Geom::Quads { quads } = &a.geom {
        return quads.iter().map(|q| text_quad_bounds(*q)).collect();
    }
    vec![union_rect(&selection_quad(&a.geom, a.style.stroke_width, Some(&a.style.border)))]
}
