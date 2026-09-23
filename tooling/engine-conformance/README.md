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
fixtures/encrypt.mjs                  the Standard security handler, for the encrypted ones
fixtures/*.pdf                        their output, committed
src/corpus.ts                         which document, which pages, which operation
src/generate-vectors.ts               reads every case through the TypeScript engine
src/quantize.ts                       the fixed-point contract, with its Rust twin
src/scope-cases.ts                    the auth/scope corpus, G3's gate
vectors/<operation>/<case>.json       the committed vectors
vectors/scope/{parse,resolve}.json    what the TypeScript scope resolver answers
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

## Search dialect

`vectors/search/` is G5's entry gate. The port order gives search an entry gate
rather than an exit gate, because the shipping matcher is JavaScript `RegExp`
and the server is RE2, and a Rust engine that reproduces neither would be found
out halfway through 655 lines rather than before them.

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run search
pnpm --filter @embedpdf/tooling-engine-conformance run search:check
```

`regex-dialect.json` records, per case, whether `validateSearchRegex` accepted
the pattern, which `SearchRegexIssue` it reported when it did not, and where
`matchRegex` put every match. `fold.json` records `foldText`'s output and its
map. `crates/epdf-engine/tests/search_dialect.rs` replays both.

The gate settled two things Phase 0 owed. `regress` executes ECMAScript syntax
but is exponential on `(?:a+)+b` in both of its backends (295 ms at 24
characters backtracking, 1.13 s on the PikeVM, roughly 8x per four more), which
disqualifies it for patterns a search box takes from anyone. The `regex` crate
is linear but speaks a different dialect, so
`crates/epdf-engine/src/features/search/dialect.rs` translates, and every case
in the corpus agrees except two.

Those two are `^` and `$` under `m` across a `\r`, U+2028 or U+2029. JavaScript
breaks a line on all four terminators and the `regex` crate breaks on `\n`
only. They are listed in that test's `KNOWN_DIVERGENCES` with the reason; G5
owns the fix, and until then a `^`-anchored query can find different matches on
web and native for a document whose extracted text uses `\r`.

The Rust fold is G5 too, so `fold.json` currently has an invariant runner
rather than a parity runner: one map entry per folded code unit, monotonic, and
every entry landing on a character start. Those are the properties
`toOriginalRange` rests on.

## Scope

`vectors/scope/` is G3's gate, and it is the only one here guarding a security
boundary rather than an answer. `auth/scope` is a deliberate duplicate and not
a port: web keeps the TypeScript resolver in `engine/core` and its enforcement
callers in `engine/main`, native permission enforcement has no other owner, and
the port order says no deletion at Stage C is implied. Two copies of an
authorization rule drift, and drift here means a token grants more on one
platform than the other while both sides' unit tests stay green.

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run scope
pnpm --filter @embedpdf/tooling-engine-conformance run scope:check
```

`parse.json` records, per scope string, whether the grammar accepted it and the
shape it parsed to. `resolve.json` records the decoded bit table and then, per
scope array and permission word, the expanded capability set, every
`checkCapability` answer, and the `checkCollab` and `checkSetGroup` probes.
`crates/epdf-engine/tests/scope_conformance.rs` replays both, with no
known-divergence list: a search result that differs subtly is a worse search, a
permission result that differs subtly is a security bug, and there is nothing
about one worth writing down instead of fixing.

The cases lean on the four places a reimplementation plausibly differs rather
than on breadth. Narrowing, where `[doc.annotate.modify,
annotations:update:self]` has to mean "edit your own" and an implementation
that checked `modify` first turns it into "edit anyone". Bit 12 without bit 3,
which grants nothing. `group=X`, which checks the caller's membership as well
as the row's group. And the filter values carrying colons, because the parser
splits on the first two and only the first two.

Worth knowing if you extend it: the runner compares TypeScript's
`{kind, name}` objects against Rust's enum by hand rather than deriving
`Deserialize` on the enum. Deriving it would make the two shapes agree by
construction, which is the one thing this gate is for.

## Encrypted fixtures

`fixtures/encrypt.mjs` implements enough of the PDF Standard security handler
to generate documents at revisions 2, 3, 4 and 6. G3's gate asks for permission
traces, and the repository had exactly one encrypted document,
`encrypted-hello.pdf` with user password `hello`. One document cannot
distinguish a user unlock from an owner unlock, cannot cover the revisions in
use, and cannot produce the unsupported-handler path at all.

Every generated document uses user password `user` and owner password `owner`,
except `encrypted-r4-empty-user-password.pdf`, which opens with no password and
is still encrypted. That one is in the corpus because it is the case most
easily mistaken for an unencrypted document.

The file ID, the salts and the AES initialization vectors are fixed constants
rather than random, so the output is byte-identical on every run and CI can
diff it. Nothing in that file is a cryptography implementation anyone should
reuse: RC4 is broken, revision 2's 40-bit key is a rounding error, and fixed
IVs are exactly what you must not ship. It exists to produce documents PDFium
will open one way.

RC4 is written out by hand because OpenSSL 3 dropped it and
`createCipheriv('rc4', ...)` throws `ERR_OSSL_EVP_UNSUPPORTED`. Revisions 2
through 4 have no other option.

## Security state

`vectors/security/` is the derivation half of G3's gate: given what PDFium
reported, what shape does the engine produce? Pure, so it needs no document and
no fixture per combination of bits. The half that reads a real encrypted file
is a separate runner.

```bash
pnpm --filter @embedpdf/tooling-engine-conformance run security
pnpm --filter @embedpdf/tooling-engine-conformance run security:check
```

`state.json` records `buildSecurityInfo`, `hasAllStandardSecurityPermissions`,
`openedAsFromCode`, `advisoryFromPdfBits` and `securityStateFromProbe`.
`crates/epdf-engine/tests/security_state.rs` replays it.

Three things in here are easy to get wrong and invisible when you do. "All
permissions" for an encrypted document is `0xFFFFFFFC` and not `0xFFFFFFFF`,
because the Standard security handler reserves bits 1 and 2 as zero and PDFium
masks owner permissions through that convention; compare against `0xFFFFFFFF`
and every owner-unlocked document reads as restricted. The order of
`access.reasons` is wire shape rather than an implementation detail, since
`/access` carries the array as written. And an unencrypted document's security
handler revision is nulled rather than passed through.

The runner also pins something no comment can. `document-security-state.ts`
says the advisory "MUST mirror the bit combinations the scope resolver uses for
`pdf.permissions` expansion", and the two live in different packages in both
languages. `the_advisory_and_the_scope_resolver_tell_the_same_story` checks
each advisory field against the capability it claims to describe, so a badge
that offers high-quality printing where the resolver denies `doc.print.high`
fails rather than shipping.

The encrypted fixtures also pin a defect rather than a behaviour.
`crates/epdf-engine/src/features/security/reader.rs` reproduces
`SecurityReader.readLiveSecurityInfo`, which calls
`FPDF_GetDocUserPermissions` while describing it as the word in force after an
owner unlock. PDFium's header says that call "always returns user permissions,
even if the document was unlocked by the owner", so an owner-unlocked session
reports itself restricted; the call that matches the description is
`FPDF_GetDocPermissions`. `encrypted-r2-rc4-40.pdf` opened with the owner
password reports `0xFFFFFFFC` from one and `0xFFFFF7E8` from the other.

Parity won over correctness there, deliberately: native is the first caller
either implementation will ever have, and the function has none today.
`read_live_reports_declared_permissions_which_is_the_reproduced_defect` pins
the wrong answer so that fixing it is something somebody does on purpose, on
both sides at once.

One genuine type difference is recorded rather than smoothed over. JavaScript's
bitwise operators return a signed 32-bit int, so `0xFFFFFFFC & ~(1 << 3)` is
`-12` there, and the TypeScript launders every permission word through `u32()`
at each boundary. Rust takes `u32` and converts once at the FFI edge with
`normalize_u32`. Both spellings of each word are in the corpus.

## The linked WASM bridge

`src/verify-wasm-bridge.mts` drives the Stage A geometry export inside a module
where Rust and PDFium are linked together, and checks the result against the
frozen vectors. It is the only gate that can prove the thing Stage A rests on:
the page pointer PDFium hands out is the same pointer Rust can read, because
they share one linear memory. `crates/epdf-wasm/tests/borrowed_page.rs` covers
the C ABI and the ownership contract on the host, where the target makes no
difference.

Building the module is deliberately not automatic, since the shipping payload
is PDFium alone until the routing lands:

```bash
export EMSDK=packages/engine/runtime/runtime-src/third_party/emsdk
export PATH="$EMSDK/upstream/emscripten:$PATH"
cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release
cd packages/engine/runtime
node build/generate-functions.mjs wasm32-eh
node build/generate-runtime-methods.mjs build/generated
EMBEDPDF_RUST_STATICLIB=../../../target/wasm32-unknown-emscripten/release/libepdf_wasm.a \
  PDF_RUNTIME_TARGET=wasm32-eh PDF_RUNTIME_WASM_OUT_DIR=/tmp/eh-rust \
  bash build/compile.esm.sh
cd ../../..
pnpm --filter @embedpdf/tooling-engine-conformance run verify:wasm-bridge /tmp/eh-rust
```

Measured on `report.pdf` at the time of writing, and the reason the encoding is
still an open question rather than a settled one:

```
page 0: 238 runs, 5087 glyphs, 1132667 bytes, 9.6ms
page 4: 293 runs, 8106 glyphs, 1753365 bytes, 9.6ms
```

A megabyte and a half of JSON for one page is not a wire anybody should ship.
The plan says the packed alternative carries a decode cost the benchmark ratio
does not isolate, so which encoding wins is a question for the Phase 0
baseline, with this number as one of its inputs.
