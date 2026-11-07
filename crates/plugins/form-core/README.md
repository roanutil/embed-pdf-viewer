# epdf-form-core

`packages/core/form` in Rust: the form widget painter. Given a widget row and its field, it says
what the engine's appearance generator would draw (`widget_drawing`), places a drawing on the page
(`widget_scene`), and tells an HTML editor where typed text will land (`text_editor_layout`). It
draws text in the layout of `epdf-text-core` (`crates/plugins/text-core`), the port of
`packages/core/text`, as upstream e40c9c236 does. It's pure: no PDFium, no DOM, and it builds for `wasm32-unknown-unknown` with
`--no-default-features`. All 514 cases replay identically (`cargo test -p epdf-form-core`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/form-painter.json` | 514 | `defaultPaintFonts` (21), `programPaintFont` (5), `layoutField` (100), `widgetDrawing` (248), `widgetScene` (26), `textEditorLayout` (24), `widgetText` (11), `paints` (8), the line-break rule `needsBreak` (71) |

This is not `vectors/plugins/form-core.json`. That file records the form plugin
(`crates/plugins/form`); this one records the painter it draws with.

The generator is `tooling/plugin-conformance/src/suites/form-painter.ts`. Its widget matrix follows
the engine's parity gate (`packages/engine/main/test/widget-painter-parity.test.ts`), which checks
the same TypeScript against PDFium's real appearance streams: every standard face at four sizes,
alignments, the five border styles, multi-line wrapping, combs, passwords, `/MaxLen`, combo and
list boxes, push buttons, and each check style on and off. It adds what the gate can't build through
the engine: a font that isn't one of the 14, missing glyphs, absent colours, empty and negative boxes,
negative and out-of-range `/TI`. Fonts arrive in a case as data (a standard name, the last resort,
or a program font's ranges and advance), because the shipping values hold closures; the suite's
header names every wrapper. `tests/behavior.rs` ports
`packages/core/form/test/painter.test.ts` and `editor.test.ts`.

The arithmetic matches bit for bit, not within a tolerance. The TypeScript rounds to single
precision wherever PDFium computes in `float` (`Math.fround`), and the port rounds in the same
places, so a line that only just fits breaks in the same spot.

## Where the port differs

The inputs are typed. `WidgetRow` and `FieldFacts` deserialize from the engine's widget and field
DTOs and keep only what the painter reads. Colours are parsed as `parseInt(pair, 16)` would parse
each pair (`drawing/border.ts:36-38`); the schema allows only `#rrggbb`, so other text isn't pinned.

`PaintFont` and `MountableFontProgram` serialize their data members only. The TypeScript objects
also carry `covers`, `width` and `load`, which the canonical form prints as `{"$function": true}`;
`src/harness.rs` adds those members where the TypeScript has them.

`SceneNode` and `Paint` are `core-annotation`'s, re-exported from `epdf-annotation-core`, as the
TypeScript imports them. That crate builds without its harness for `wasm32-unknown-unknown`, so
depending on it adds no PDFium. A `textRun` node holds its font program as a `FontProgramRef`, the
run's `MountableFontProgram` behind it.

## What it costs a binary

`epdf-fonts` keeps the 14 OpenType programs in a static table apart from the metrics this crate
reads (Task 13.18, Ruling13.5(b)), so a build that never calls `FontProgram::load` leaves them out.
Before the split the linker kept them all: a release build of `tests/behavior.rs` with
`--no-default-features` held all 14 programs (435,050 bytes of base64). The form plugin links this
crate into `epdf-web`; the size a wasm32 build pays is in `crates/plugins/fonts/README.md`.
