# Cross-language engine conformance corpus

Frozen vectors that pin the Rust engine core to the behaviour of the shipping
TypeScript engine, one operation at a time. `multiplatform-plan.md` Section 9
calls for this under "Cross-language corpus"; `multiplatform-port-order.md` G2
calls for the geometry half of it under "a glyph-level differential harness
against `PageGeometryReader`, built in Phase 0 because none exists".

TypeScript is the oracle. A Rust reader that disagrees is wrong until somebody
regenerates the corpus on purpose and says why in the commit.

## Layout

```
fixtures/generate.mjs                 documents generated for the corpus
fixtures/*.pdf                        their output, committed
src/corpus.ts                         which document, which pages, which operation
src/generate-vectors.ts               reads every case through the TypeScript engine
src/quantize.ts                       the fixed-point contract, with its Rust twin
vectors/<operation>/<case>.json       the committed vectors
```

The Rust side reads them through `test_support::corpus`; the geometry runner is
`crates/epdf-engine/tests/geometry_corpus.rs`.

## Running it

Regenerate after a deliberate behaviour change, and commit the diff:

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run vectors
```

Check without writing, which is what CI does:

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run vectors:check
```

Replay them against Rust:

```bash
bash packages/engine/runtime/scripts/fetch-libpdfium.sh darwin-arm64  # once
cargo test --workspace
```

## Two decisions worth knowing before you add a case

**Numbers are frozen at 1e-4 of a PDF point.** The oracle reads PDFium compiled
to WASM and the Rust runner reads the same fork compiled natively. Both sides
quantize to integer units and compare those, so nothing in the gate rests on
float equality. Divergence below a ten-thousandth of a point is deliberately
invisible here; it belongs to the pixel and performance gates. `src/quantize.ts`
and `crates/test-support/src/corpus.rs` have to agree exactly, including on how
they break ties, and each says so.

**A metric case has to embed its font.** A document that names a standard-14
font and embeds no font program makes PDFium substitute, and the substitution
depends on the host: the WASM build has no system fonts and uses its built-in
mapper, a native macOS build reaches CoreText. On `hello_world.pdf` that is
`looseBox.bottom` 47.012 through WASM against 46.328 through the macOS dylib,
which is four orders of magnitude past the quantum and not a rounding question.
So `rotated_text.pdf`, `sheared_text.pdf`, `mirrored_text.pdf` and
`vertical_text.pdf` are not in this corpus despite covering exactly the right
behaviour, and `fixtures/embedded_orientations.pdf` covers it with an embedded
Roboto instead.

That substitution gap is a product question, not only a testing one: a native
reader and a browser reader will lay out the same standard-14 document
differently until something pins the fallback environment across hosts. It
belongs to G4's `FontRegistry`. Until then, keep metric cases embedded.

## Adding an operation

1. Add the kind to `CorpusOperation` and handle it in `readOperation`. The
   switch is exhaustive, so the compiler will point at the second place.
2. Add or extend the cases in `CORPUS`.
3. Regenerate, and read the diff rather than trusting it.
4. Add a Rust runner beside `geometry_corpus.rs` that loads the same operation.

## Operation matrix

`src/operations.ts` carries one row per `WorkerRequest` kind: the port group
that owns it, the ownership stage it moves under, the phase that moves it, and
the cancellation case it has to document. `OPERATIONS.md` is its rendering, and
is generated.

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run operations        # rewrite OPERATIONS.md
pnpm --filter @embedpdf/tooling-engine-conformance run operations:check  # what CI runs
```

The kinds come from the `WorkerRequest` type through the TypeScript compiler,
not from a regular expression over the file. Grepping for `kind: '` also finds
the four `LayerOpenSource` variants nested inside the open requests and the
four response and lifecycle kinds; asking the compiler gives 65 and no
argument. The check fails in both directions: a kind with no row, and a row for
a kind that no longer exists.

## Schema parity

`vectors/schema/` is the one set of vectors here that is not read out of a
running engine. `src/schema-cases.ts` holds hand-written JSON documents,
`src/generate-schema-vectors.ts` records what the shipping zod schema in
`packages/engine/core/src/wire/schemas.ts` makes of each one, and
`crates/epdf-engine/tests/schema_parity.rs` asserts that serde accepts exactly
the same documents and reads them to mean the same thing.

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run schema
pnpm --filter @embedpdf/tooling-engine-conformance run schema:check
```

The plan keeps those schemas handwritten while making the Rust types canonical,
which only works if something holds the two together. It found its first
divergence immediately: `created` and `modified` are `z.string().datetime()` on
the TypeScript side, so the schema refuses `2026-09-22T08:00:00+02:00`,
`2025-02-30T00:00:00Z` and `not-a-date`, while a `Patch<String>` in Rust took
all three. Hence `IsoDateTime` and `clock::is_iso_8601_utc`.

Explicit `undefined` is in the plan's list of cases and is not here, because it
is not JSON-expressible. It belongs in a TypeScript-only test.
