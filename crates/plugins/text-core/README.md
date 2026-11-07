# epdf-text-core

`packages/core/text` in Rust: text laid out as the engine's shared text engine lays it out. Fonts
with fallbacks and a last resort, bidi order, graphemes, joins, and a form field's lines, size and
runs (`layout_field`). It's pure and builds for `wasm32-unknown-unknown`.

It has no vectors of its own. The form painter's suite (`vectors/core/form-painter.json`, replayed
by `cargo test -p epdf-form-core`) calls `layoutField`, `defaultPaintFonts`, `programPaintFont` and
`needsBreak` directly, and the annotation scene suite (`vectors/core/annotation-scene.json`)
reaches it through redaction labels.

`build.rs` reads `packages/core/text/src/joining.generated.ts`, so the joining table has one copy.
`MountableFontProgram` and the last-resort program are `packages/core/fonts`, so they live in
`epdf-fonts`.

## Where the port differs

Text is UTF-16 inside the layout, as the TypeScript reads a string, and becomes a `String` only in
a run. A run is whole graphemes, so it never splits a surrogate pair.

Bidi levels come from `unicode-bidi`, not `bidi-js`. Both pass Unicode's conformance tests, which
leave open the levels of characters rule X9 removes; `src/bidi.rs` assigns those, and applies
rule L1, the way `bidi-js` does. Scripts, marks and mirrors come from `unicode-script`,
`unicode-properties` and `unicode-bidi-mirroring` (Unicode 16 to 18, where Node 22 has 17), so a
character assigned in the newest versions can classify differently. No vector reaches one.

