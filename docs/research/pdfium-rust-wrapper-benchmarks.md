# Benchmarking a Rust wrapper around PDFium

The last benchmark on this branch closed with a sentence I have not stopped thinking about: a real
port should pick its first slice by asking how much compute sits behind each crossing. The three
toys in `ts-wasm-interop-poc/` had almost none, and Rust lost every frame cell to plain TypeScript,
1.6x to 3.1x, with the tables in
[`ts-wasm-interop-poc/bench/RESULTS.md`](../../ts-wasm-interop-poc/bench/RESULTS.md).

PDFium is the opposite case. So this is a plan to measure the same boundary with a real workload
behind it: what does it cost to put Rust between TypeScript and the PDFium fork in
`packages/engine/runtime`, first in wasm and then native?

## What this cannot tell us

Worth saying first, because there are two ways to over-read the result and I expect to be tempted by
both.

It says nothing about whether 16,281 lines of `engine-services` port well. A hand-written Rust
wrapper over roughly 25 PDFium functions is not evidence about that in either direction.

And it says nothing about iOS or Android, which is the actual argument for the port in
[the port plan](./rust-core-port.md). Every number here is about web and Node. If they all come out
against Rust, the case for the port is unchanged, because the case was never performance.

## The gate

Everything below is contingent on one thing that has never been tried here, so it goes first.

PDFium is built with Emscripten. Rust compiled to `wasm32-unknown-unknown` cannot link an Emscripten
object, which is wave 0 of the port plan and the thing that can invalidate the rest. But the link
looks friendlier than that plan assumed. `packages/engine/runtime/build/compile.esm.sh:35` links
`libembedpdf.a`, a plain static archive, with `em++`. A Rust `staticlib` built for
`wasm32-unknown-emscripten` can go on that same command line. No second module, no cross-module
calls, no shared-memory problem.

And `packages/engine/runtime/scripts/fetch-libpdfium.sh` downloads that archive prebuilt, pinned by
sha256 in `engine-runtime-build.json`. So the gate needs emsdk to relink. It does not need a PDFium
source build.

The spike is one function. `rs_probe_page_size` calls `EPDF_GetPageSizeByIndexNormalized`, gets built
with `cargo build --target wasm32-unknown-emscripten --release`, joins the `em++` line, and gets
loaded in Node and asserted equal to the number the current runtime returns. Timebox it at a day.

The risk is toolchain agreement, and I cannot predict it from here. Local Rust is 1.98.0. Emscripten
is pinned at 3.1.70 in `packages/engine/runtime/Dockerfile:2` and 3.1.72 in `scripts/dev.sh:7`, which
is its own small problem someone should look at. If the two LLVMs disagree on wasm object format,
try pinning emsdk to whatever Rust 1.98.0 was tested against, then try building the Rust as an
Emscripten `SIDE_MODULE` against `-sMAIN_MODULE=2`, which shares linear memory and so is not the
per-call-crossing disaster the port plan feared. If neither works, say the wasm arm is blocked and
publish that. It is a more valuable finding than any timing below.

## The arms

Four builds, all from one link script with identical flags, because otherwise you are comparing
builds and not wrappers.

Arm A is today: `libembedpdf.a` linked by `em++`, called from TypeScript through `cwrap` in
`packages/engine/runtime/src/wasm/wasm-runtime.ts`.

Arm B is the thin shim. One `extern "C"` Rust function per PDFium function the workloads touch,
added to `EXPORTED_FUNCTIONS`, reached through the same `cwrap` path, so
`fn.FPDFText_GetTextObject` becomes `fn.rs_FPDFText_GetTextObject`. Identical call count. One extra
wasm frame. This is the number you asked for: the tax.

Arm C is coarse. Four Rust functions, `rs_read_page_geometry`, `rs_read_page_text`,
`rs_render_page` and `rs_search_page`, each running its loop inside Rust and returning one packed
buffer. TypeScript does one `readBytes` and one typed-array decode. This is the counterweight,
because a cost with no benefit beside it can only ever argue one way.

Arm D is the control, and it is the addition I would most expect to get cut. It is arm C written in
C++ instead of Rust, as a standalone `.cc` on the same `em++` line. I want it because
`packages/engine/services/src/features/text/PageTextReader.ts:93` already reads a whole page of text
through a single `EPDFText_GetTextFull` call. The fork has been collapsing these loops in C++ for a
while. So if arm C beats arm A by 5x, arm D is what tells you whether that 5x belongs to Rust or
merely to moving the loop, and I think the honest answer is mostly the latter. It costs one more
source file and no PDFium rebuild.

(If arm D lands within noise of arm C on every workload, that is the single most decision-relevant
result in this document, and it argues for writing coarse `EPDF*` functions in the fork rather than
for a Rust layer at all.)

`pdfium-sys` is `bindgen` over the headers in `build/libpdfium/wasm32/include`, using the same
`/^(?:FPDF|EPDF|FORM|PDFiumExt_)/` filter as `build/generate-functions.mjs:15`.

## The fairness gate

Before any timing, every arm produces a canonical result for every workload and they get compared:
geometry runs as JSON rounded to 1e-9, extracted text as a string, bitmap bytes as a sha256, search
hits as rects. Any disagreement aborts the run.

The previous study's numbers were trustworthy because it did this, and the gate caught real
divergence. Skipping it means a wrong arm can look fast and nothing about the timings would reveal
it.

## Workloads

Five, ordered by how much PDFium work sits behind each crossing. Each cell reports median and p95 in
nanoseconds.

### 1. Null probe

`EPDF_GetPageSizeByIndexNormalized` in a tight loop, then a `mem.peek` loop, then a
`mem.readBytes(ptr, 64)` loop. Roughly a million crossings with no PDFium work behind any of them.
Arms A and B only, since C and D have nothing to collapse.

This is the pure wrapper tax and the only workload that measures it in isolation.

### 2. Glyph geometry

`PageGeometryReader.read` at
`packages/engine/services/src/features/geometry/PageGeometryReader.ts:86`, on page 4 of
`examples/snippet-react/public/report.pdf`, which carries 8,106 glyphs. The count goes in the
results header.

This is the chattiest path in the engine and the reason to bother. Per glyph the loop at `:109` makes
`FPDFText_GetTextObject`, `EPDFText_GetCharGeometry`, sometimes `FPDFText_GetFontSize`, and then 5 to
21 `mem.peek` calls to decode the struct in `readGlyphRaw` at `:135`. So 8,106 glyphs is somewhere
around 100,000 crossings for arithmetic that fits in registers.

(I first picked `examples/react/public/ebook.pdf`, whose densest page has 625 glyphs. That is thin
enough that the crossing count stops being the dominant term, which would have quietly flattered
every arm. Measuring the candidates first was worth the ten minutes.)

### 3. Page text

`PageTextReader` on the same page. Already bulk, so it is not a crossing-count workload at all. It is
here for a different axis: a large buffer coming back across the boundary. In wasm `readBytes` is a
typed-array slice and nearly free; natively it is an N-API `ArrayBuffer` copy. I expect this one to
be boring in wasm and interesting native.

### 4. Page render

`FPDF_RenderPageBitmap` at 1x and 4x, plus the bitmap copy out of the heap, reported separately
because the copy is the only part any arm can change. One crossing, milliseconds of PDFium work
behind it.

If the wrapper tax is visible here, something is wrong with the harness.

### 5. Search

An `FPDFText_FindNext` loop for a term with many hits. A middle case, roughly a thousand crossings
with small work behind each.

## Method

Reuse `ts-wasm-interop-poc/bench/harness.mjs` by relative path rather than writing new statistics.
Warm up 10,000 iterations or 200 ms, whichever comes first. Sample against a 500 ms budget per cell
and count iterations, rather than fixing the iteration count. Consume every result into an
accumulator the harness prints, because an unconsumed call is a call V8 may delete. Report median and
p95, never mean. Run the arms round-robin for k=7 rounds so the laptop's thermal drift is spread
across all four rather than handed to whoever went last.

Stamp Node, OS, CPU, emsdk version, rustc version and battery state into every results file. Absolute
numbers here are machine-specific and a results file without that header is a trap for whoever reads
it in six months.

Node only. The question is per-operation cost, and cold start is already covered in
`ts-wasm-interop-poc/bench/RESULTS.md`. But collect one static number while you are there: the byte
size of `embedpdf.wasm` for each arm. A Rust staticlib grows the module, and on web that is a real
cost native does not pay.

## Shape of it

```
pdfium-rust-spike/
  crates/
    pdfium-sys/          bindgen over the fetched headers
    shim/                arm B, 1:1 exports
    ops/                 arm C, four coarse operations
  cpp/ops.cc             arm D, the same four in C++
  build/link-wasm.sh     mirrors compile.esm.sh, plus the staticlib
  bench/
    arms/{a,b,c,d}/      four loadable runtimes
    workloads.mjs
    gate.mjs             canonical outputs, compared before timing
    run.mjs              round-robin driver
    results/
```

Top-level and self-contained, like `ts-wasm-interop-poc/`, so nothing published is touched and a dead
end costs one `rm -rf`. The cost is that `build/link-wasm.sh` is a copy of `compile.esm.sh` and will
drift from it. I think that is the right trade for a spike, but it is a trade.

## Native, after

Same crates targeting `aarch64-apple-darwin`, `napi-rs` in place of the Emscripten exports, linked
against `libembedpdf.dylib`. Arm A becomes the generated C++ addon from
`build/generate-napi-binding.mjs`. Same five workloads, same gate, same harness.

I expect native and wasm to disagree, and for a specific reason. In wasm, `mem.peek` is
`module.getValue` reading a typed array in JavaScript, and it never crosses anything. Natively,
`peek` at `packages/engine/runtime/src/native/native-runtime.ts:57` is a full N-API round trip. So the
21 peeks per glyph are nearly free in one place and expensive in the other, and any conclusion drawn
from the wasm numbers alone will be wrong about native.

## Predictions

Written before measuring, so a mismeasurement is obvious rather than persuasive.

| Workload | Prediction |
|---|---|
| Null probe, B vs A | within 15%; a Rust frame inside the module is an ordinary wasm call |
| Glyph geometry, C vs A, wasm | 3x to 10x, held down by peeks already being cheap |
| Glyph geometry, C vs A, native | larger, plausibly past 10x, because peeks are N-API calls |
| Page render, all arms | within noise; it is all PDFium |
| Page text, wasm | boring; native, C wins on the buffer copy |
| D vs C, everywhere | within noise, and that is the finding |
| `embedpdf.wasm` size | +100 to +400 kB for B and C |

If arm B is more than 25% slower than arm A anywhere, suspect the harness before believing it.

## What a good outcome looks like

One table with five workloads across four arms, a stated tax for the thin shim, and a sentence
somewhere in [the port plan](./rust-core-port.md) that says what a PDFium crossing actually costs
instead of guessing.

The result I am most interested in is arm D, and I would rather find out that the fork should keep
growing `EPDF*` functions in C++ than talk myself into a Rust layer that buys the same thing for more
work. I genuinely do not know which way it goes.
