//! Text helpers over a page's geometry and text snapshots.
//!
//! The layout and charmap functions clamp or tolerate what PDFium never
//! produces: runs that leave gaps, indices past the page, maps that break the
//! charmap invariants. Here those inputs are refused, so a malformed snapshot
//! cannot come back as plausible geometry or text. Inverted ranges are not
//! malformed: they keep the empty answer the shared vectors pin.

use epdf_engine::features::geometry::{PageGeometryRun, PageGeometrySnapshot};
use epdf_engine::features::text::TextRange;
use epdf_engine::features::text::charmap::{
    PageTextSnapshot, boundary_text_offset, char_map_violation, char_range_for_text_offsets,
    slice_text_by_chars,
};
use epdf_engine::geom::{PagePoint, PageQuad, PageRect};
use epdf_engine::host::HostResponse;
use epdf_engine::page_space::PageCoordinates;
use epdf_engine::page_space::text::{TextLayout, create_text_layout};
use serde::Deserialize;

use super::{answer, invalid, parse};
use crate::abi::Failure;

/// A character range as `start` and `count`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountedRange {
    start: u32,
    count: u32,
}

/// A half-open `[start, end)` range.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HalfOpenRange {
    start: u32,
    end: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentsArgs {
    geometry: PageGeometrySnapshot<PageCoordinates>,
    range: CountedRange,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuadArgs {
    geometry: PageGeometrySnapshot<PageCoordinates>,
    index: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeArgs {
    text: PageTextSnapshot,
    range: HalfOpenRange,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryArgs {
    text: PageTextSnapshot,
    boundary: u32,
}

// ── geometry ──

fn finite_rect(r: &PageRect) -> bool {
    [r.x, r.y, r.width, r.height, r.x + r.width, r.y + r.height].iter().all(|n| n.is_finite())
}

fn finite_point(p: &PagePoint) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn finite_quad(q: &PageQuad) -> bool {
    [q.upper_left, q.upper_right, q.lower_left, q.lower_right].iter().all(finite_point)
}

fn finite_run(run: &PageGeometryRun<PageCoordinates>) -> bool {
    let font_size = run.font_size().is_none_or(f64::is_finite);
    let glyphs = match run {
        PageGeometryRun::Upright { glyphs, .. } => {
            glyphs.iter().all(|g| finite_rect(&g.loose) && g.tight.as_ref().is_none_or(finite_rect))
        }
        PageGeometryRun::Rotated { glyphs, rotation, .. } => {
            rotation.is_finite()
                && glyphs
                    .iter()
                    .all(|g| finite_quad(&g.loose) && g.tight.as_ref().is_none_or(finite_quad))
        }
    };
    font_size && glyphs && finite_rect(&run.rect())
}

/// The layout of a snapshot whose runs tile its characters in order, from
/// zero, with finite geometry, and its character count.
fn checked_layout(
    geometry: PageGeometrySnapshot<PageCoordinates>,
) -> Result<(TextLayout, u32), Failure> {
    let mut next = 0u32;
    for (index, run) in geometry.runs.iter().enumerate() {
        let count = run.glyph_count();
        if count == 0 {
            return Err(invalid(format!(
                "geometry run {index} has no characters, so the runs do not tile them"
            )));
        }
        if run.start() != next {
            return Err(invalid(format!(
                "geometry run {index} starts at {} where {next} tiles the characters",
                run.start()
            )));
        }
        next = u32::try_from(count)
            .ok()
            .and_then(|count| next.checked_add(count))
            .ok_or_else(|| invalid("geometry runs tile more than u32 characters"))?;
        if !finite_run(run) {
            return Err(invalid(format!("geometry run {index} has a non-finite coordinate")));
        }
    }
    Ok((create_text_layout(geometry), next))
}

pub(super) fn segments(args: &[u8]) -> Result<HostResponse, Failure> {
    let SegmentsArgs { geometry, range } = parse(args)?;
    let (layout, char_count) = checked_layout(geometry)?;
    if range.start.checked_add(range.count).is_none_or(|end| end > char_count) {
        return Err(invalid(format!(
            "character range [{}, +{}) is outside the page's {char_count} characters",
            range.start, range.count
        )));
    }
    let segments = layout.segments(TextRange { start: range.start, count: range.count });
    if !segments.iter().all(|s| finite_quad(&s.quad) && finite_rect(&s.rect)) {
        return Err(invalid("a segment's geometry is not finite"));
    }
    answer(&segments)
}

pub(super) fn character_quad(args: &[u8]) -> Result<HostResponse, Failure> {
    let QuadArgs { geometry, index } = parse(args)?;
    let (layout, char_count) = checked_layout(geometry)?;
    if index >= char_count {
        return Err(invalid(format!(
            "character {index} is outside the page's {char_count} characters"
        )));
    }
    let quad = layout.char_quad(index);
    if quad.as_ref().is_some_and(|q| !finite_quad(q)) {
        return Err(invalid(format!("character {index}'s quad is not finite")));
    }
    answer(&quad)
}

// ── charmap ──

fn checked_text(text: &PageTextSnapshot) -> Result<u32, Failure> {
    let length = text.text_len_utf16();
    match char_map_violation(text.char_count, length, &text.char_map) {
        Some(violation) => Err(invalid(format!("char map invariant violated: {violation}"))),
        None => Ok(length),
    }
}

fn within(range: &HalfOpenRange, limit: u32, space: &str) -> Result<(), Failure> {
    if range.start > limit || range.end > limit {
        return Err(invalid(format!(
            "{space} range [{}, {}) is outside [0, {limit}]",
            range.start, range.end
        )));
    }
    Ok(())
}

pub(super) fn slice_text(args: &[u8]) -> Result<HostResponse, Failure> {
    let RangeArgs { text, range } = parse(args)?;
    checked_text(&text)?;
    within(&range, text.char_count, "character")?;
    answer(&slice_text_by_chars(&text, range.start, range.end))
}

pub(super) fn text_offsets_to_chars(args: &[u8]) -> Result<HostResponse, Failure> {
    let RangeArgs { text, range } = parse(args)?;
    let length = checked_text(&text)?;
    within(&range, length, "UTF-16")?;
    answer(&char_range_for_text_offsets(&text, range.start, range.end))
}

pub(super) fn character_boundary_to_text(args: &[u8]) -> Result<HostResponse, Failure> {
    let BoundaryArgs { text, boundary } = parse(args)?;
    checked_text(&text)?;
    if boundary > text.char_count {
        return Err(invalid(format!(
            "character boundary {boundary} is outside [0, {}]",
            text.char_count
        )));
    }
    answer(&boundary_text_offset(&text, boundary))
}
