//! `geometry.ts`: the selection plugin's view of a page's text.
//! Segmentation, hit-testing and word and line expansion live in the
//! canonical text layout ([`crate::text_layout::TextLayout`]), in page space,
//! so pointer points go in as they are and segments come out ready to draw.

use crate::text_layout::PageTextSegment;

/// One merged visual line of a selection, as the layout gives it: `quad`
/// carries the corners, named in the line's own frame; `advance` is the
/// reading direction along the baseline (1 toward the right end); `rect`
/// is its bounding box. All in page space.
pub type SelectionSegment = PageTextSegment;
