# epdf-fonts

`packages/core/fonts` in Rust: the 14 standard PDF fonts as PDFium draws them. That's each face's
ascent, descent and per-character advances, its OpenType program, the CSS family the program mounts
under, and the CSS stack for before it loads. It also carries `engine-core`'s `STANDARD_FONTS` and
`STANDARD_FACES` as `StandardFont` and `StandardFont::face`. All 101 cases replay identically
(`cargo test -p epdf-fonts`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/fonts.json` | 101 | `standardFontMetrics` with every code point of every face plus misses, `standardFontProgram`, the bytes `load()` resolves with, `standardFontCss`, `fontProgramFamily`, `decodeBase64` padding, unknown names |

The generator is `tooling/plugin-conformance/src/suites/fonts.ts`. Three of its entries are
wrappers: `standardFontMetrics` asks `advance` for each code point the case passes,
`standardFontProgram` records the program's data members, and `standardFontProgramLoad` returns the
full program bytes, which the replay compares byte for byte. `tests/behavior.rs` ports
`packages/core/fonts/test/standard-fonts.test.ts`: every program is an OpenType font whose table
checksums hold, whose `hhea` ascent and descent are the metrics', and whose cmap reaches glyphs with
the metrics' advances.

## Where the data comes from

There's one copy of it, the TypeScript package's generated output. `build.rs` reads
`packages/core/fonts/src/standard/faces.generated.ts` and the 14 `programs/<font>.generated.ts`
files and writes `$OUT_DIR/faces.rs`. It follows the generator's exact line layout and panics on
anything else: an unexpected line, a face list or order other than `STANDARD_FONTS`, an advance
count that differs from the code points, a program that isn't padded base64. A regenerated face set
flows into Rust on the next build or stops it.

The cost is that the crate builds only inside this repository, which every host build already
requires. It also compiles all 14 programs in, about 420 KB of base64, where the TypeScript splits
each into a module it imports on demand (`packages/core/fonts/src/standard/programs.ts:10-25`).
Nothing links the crate into `epdf-web` yet. When the form painter does, embedding
build-time-decoded bytes would be about 25% smaller and skip the runtime decode.

## Where the port differs

`FontProgram::load` is synchronous and can't fail. The TypeScript's `load()` returns a promise
because it imports the program's module on demand, and drops a failed import so the next call
retries (`packages/core/fonts/src/standard/programs.ts:54-63`). Here the program is in the binary:
the first call decodes it and every call returns the same `&'static [u8]`.

The API takes `StandardFont`, so an unknown name can't reach it. The TypeScript's type forbids one
too, but at runtime it throws a `TypeError` reading off the missing face: `Cannot read properties of
undefined (reading 'codePoints')` from `standardFontMetrics`
(`packages/core/fonts/src/standard/metrics.ts:32`), and `(reading 'programHash')` from everything
that builds a program (`packages/core/fonts/src/standard/programs.ts:47`). `src/harness.rs` parses
the name and answers a miss with that exact error, and 20 cases pin it.

Names that are `Object.prototype` keys aren't pinned. `FACES['toString']` finds a function, so the
TypeScript gets further and throws reading `forEach` (metrics) or `slice`
(`fontProgramFamily(undefined)`). Rust treats `toString`, `__proto__` and `constructor` like any
other unknown name.

`font_program_family` takes the first 16 UTF-16 code units, as `String#slice` does. When the 16th
unit is the high half of a surrogate pair, the TypeScript returns a lone surrogate, which a Rust
`String` can't hold, so Rust stops before that character. Hashes are hex, so it can't happen in
practice. `tests/behavior.rs` pins the Rust choice.

`decode_base64` assumes the text is a whole number of 4-character groups. The TypeScript throws
`RangeError` from `new Uint8Array` for any other length (`packages/core/fonts/src/base64.ts:8`).
`build.rs` checks every program, and nothing else calls it.

`StandardFontMetrics::advance` takes a `u32` where the TypeScript takes any number. Its `Map` only
matches the integer keys, so the harness turns `NaN`, fractions and negatives into a miss.
