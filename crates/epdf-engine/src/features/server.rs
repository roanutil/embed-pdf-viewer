//! G15. Server and file surfaces.
//!
//! Operations: `pages.renderEncoded` (sole owner of the wire kind; the raster
//! step comes from `features::render`'s internal entry point),
//! `annotations.renderAppearancesEncoded`, `document.renderPageFile`,
//! `document.renderPageFileEncoded`.
//!
//! No new engine behavior beyond G4 raster and G10 save. Browser and local
//! workers keep returning `NotImplemented` for the encoded kinds
//! (`worker-protocol.ts:446`).
