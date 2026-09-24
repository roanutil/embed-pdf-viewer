//! G4. Page rendering.
//!
//! Replaces `services/src/features/render/{PageRenderReader,deviceRaster}.ts`
//! (294) and `core/src/dto/PageRender.ts`. Operation: `pages.render`, plus an
//! internal `render_page_bitmap` entry point that is not a wire kind and that
//! `features::server` (G15) consumes for `pages.renderEncoded`.
//!
//! Encoding stays a host-injected concern. Gate: exact interior BGRA on a
//! colored fixture, plus tile, rotation, alpha and buffer-lifetime tests.

pub mod geometry;
pub mod raster;
