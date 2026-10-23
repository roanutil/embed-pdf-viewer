# epdf-form-core

`packages/core/form` in Rust: the form widget painter. Given a widget row and its field, it says
what the engine's appearance generator would draw (`widget_drawing`), lays out text the way PDFium's
variable text does (`layout_variable_text`), places a drawing on the page (`widget_scene`), and
tells an HTML editor where typed text will land (`text_editor_layout`). It's pure: no PDFium, no
DOM, and it builds for `wasm32-unknown-unknown` with `--no-default-features`. All 438 cases replay
identically (`cargo test -p epdf-form-core`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/form-painter.json` | 438 | `layoutFontOf` (21), `layoutVariableText` (54), `widgetDrawing` (239), `widgetScene` (26), `textEditorLayout` (19), `paints` (8), the line-break rule `needsBreak` (71) |

This is not `vectors/plugins/form-core.json`. That file records the form plugin
(`crates/plugins/form`); this one records the painter it draws with.

The generator is `tooling/plugin-conformance/src/suites/form-painter.ts`. Its widget matrix follows
the engine's parity gate (`packages/engine/main/test/widget-painter-parity.test.ts`), which checks
the same TypeScript against PDFium's real appearance streams: every standard face at four sizes,
alignments, the five border styles, multi-line wrapping, combs, passwords, `/MaxLen`, combo and
list boxes, push buttons, and each check style on and off. It adds what the gate can't build through
the engine: a font that isn't one of the 14, missing glyphs, absent colours, empty and negative boxes,
negative and out-of-range `/TI`. Five entries are wrappers because the shipping values hold
closures; the suite's header names them. `tests/behavior.rs` ports
`packages/core/form/test/painter.test.ts` and `editor.test.ts`.

The arithmetic matches bit for bit, not within a tolerance. The TypeScript rounds to single
precision wherever PDFium computes in `float` (`Math.fround`), and the port rounds in the same
places, so a line that only just fits breaks in the same spot.

## Where the port differs

Run text is a `String`. The TypeScript builds it with `String.fromCharCode` from UTF-16 code units
(`packages/core/form/src/layout/edit.ts:25,33`), so a comb that puts the two halves of a surrogate
pair in two cells writes each half alone. A Rust `String` can't hold a lone surrogate, so each half
becomes U+FFFD. `LaidWord::code` still carries both units. No vector pins this, because a lone
surrogate can't round-trip through the JSON the replay reads; `tests/behavior.rs` pins the Rust
choice.

The inputs are typed. `WidgetRow` and `FieldFacts` deserialize from the engine's widget and field
DTOs and keep only what the painter reads. Colours are parsed as `parseInt(pair, 16)` would parse
each pair (`drawing/border.ts:36-38`); the schema allows only `#rrggbb`, so other text isn't pinned.

`FontProgram` serializes its data members only. The TypeScript object also carries `load`, which the
canonical form prints as `{"$function": true}`; `src/harness.rs` appends that member to every
`program` it returns.

`SceneNode` and `Paint` are local. `core-annotation` owns those types in TypeScript, but its Rust port
(`crates/plugins/annotation-core`) has no `glyphs` node or `clip` yet, so this crate defines the
`path` and `glyphs` nodes `widget_scene` makes, with the TypeScript's member order. Task 13.7 owns
the fix: it ports the `glyphs` node and `clip` into annotation-core's `SceneNode`, keeping that
crate's `--no-default-features` build pure, and this crate then re-exports annotation-core's types
in place of its own.

## What it costs a binary

`epdf-fonts` keeps all 14 OpenType programs in one static table, about 420 KB of base64, and this
crate reads that table for metrics. The linker keeps the whole table: a release build of
`tests/behavior.rs` with `--no-default-features`, which never calls `FontProgram::load`, still holds
all 14 programs (435,050 bytes of base64). Nothing links `epdf-form-core` into `epdf-web` yet. When
the form plugin does, splitting the programs out of the metrics table in `epdf-fonts` would keep
them out of a build that only lays text out.
