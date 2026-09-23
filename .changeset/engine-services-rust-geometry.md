---
'@embedpdf/engine-services': minor
---

Add `RustPageGeometryReader`, an alternative `pages.geometry` reader that runs the glyph loop in the Rust engine core instead of crossing the WASM boundary once per glyph. `WorkerHost` selects it with the new `rustReads` option, and ignores that option on a runtime without the core, which is every published runtime today. Both readers produce identical `PageGeometrySnapshot` values against the same document.
