# Benchmarking a Rust wrapper around PDFium

The last benchmark on this branch closed with a sentence I have not stopped thinking about: a real
port should pick its first slice by asking how much compute sits behind each crossing. The three
toys in `viewer-rust-port-benchmarks/` had almost none, and Rust lost every frame cell to plain TypeScript,
1.6x to 3.1x, with the tables in
[`viewer-rust-port-benchmarks/bench/RESULTS.md`](../../viewer-rust-port-benchmarks/bench/RESULTS.md).

PDFium is the opposite case. So this is a plan to measure the same boundary with a real workload
behind it: what does it cost to put Rust between TypeScript and the PDFium fork in
`packages/engine/runtime`, first in wasm and then native?

Read this as the measurement, not as the decision. It was written while the open question was still
whether to add Rust at all. That question is settled, and the live recommendation for the fork lives
in [the runtime decision](./pdfium-rust-wrapper-decision.md): bind to the C ABI from Rust, don't add
coarse `EPDF*` wrappers. Where the closing section below disagrees with that, the decision wins.

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

Reuse `viewer-rust-port-benchmarks/bench/harness.mjs` by relative path rather than writing new statistics.
Warm up 10,000 iterations or 200 ms, whichever comes first. Sample against a 500 ms budget per cell
and count iterations, rather than fixing the iteration count. Consume every result into an
accumulator the harness prints, because an unconsumed call is a call V8 may delete. Report median and
p95, never mean. Run the arms round-robin for k=7 rounds so the laptop's thermal drift is spread
across all four rather than handed to whoever went last.

Stamp Node, OS, CPU, emsdk version, rustc version and battery state into every results file. Absolute
numbers here are machine-specific and a results file without that header is a trap for whoever reads
it in six months.

Node only. The question is per-operation cost, and cold start is already covered in
`viewer-rust-port-benchmarks/bench/RESULTS.md`. But collect one static number while you are there: the byte
size of `embedpdf.wasm` for each arm. A Rust staticlib grows the module, and on web that is a real
cost native does not pay.

## Shape of it

```
pdfium-binding-benchmarks/
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

Top-level and self-contained, like `viewer-rust-port-benchmarks/`, so nothing published is touched and a dead
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

## What actually happened

Built, gated, and run over seven interleaved rounds in wasm and seven more natively. The full
tables, the environment stamps and every caveat are in
[`pdfium-binding-benchmarks/RESULTS.md`](../../pdfium-binding-benchmarks/RESULTS.md), which is the source of record;
what follows is what I got wrong and what I did not expect. The numbers below are from a re-run: the
first version of `RESULTS.md` had wasm tables with no raw data behind them, because `bench/run.mjs`
used to clear its whole results directory at the start of a run and a native run wiped the wasm
results that preceded it. That bug is fixed, and both platforms were re-run together in one sitting
so every number quoted here has a committed JSON file behind it.

The module size prediction was wrong by roughly two orders of magnitude. I said +100 to +400 kB for
the Rust arms. The real numbers, against a 4,676,714-byte baseline, are +3,802 bytes for arm B,
+4,774 for arm C and +8,163 for arm D. The biggest grower is the C++ one. A shim of one-line
`extern "C"` functions, and four coarse loops next to it, do not pull a runtime in behind them, and
the linker throws away everything they never touch. So the sentence in the method section about a Rust staticlib
being a real cost on web that native does not pay is, at this scale, just wrong.

Then the null probe, which is the one cell I built specifically to price the wrapper, and it could
not do it. Arm B reads 21 to 28 nanoseconds faster than arm A in every one of the seven rounds, both
before and after the order rotation that was added to test for exactly this. A wrapper cannot be
faster than the call it wraps. At roughly 340 ns a crossing, that gap is below what a 500 ms
sampling budget resolves on this machine, so the cell says nothing in either direction: not that the
tax is negative, and not that it is zero.

The tax showed up anyway, on the workloads with real PDFium work behind them. Arm B costs 1.03x arm
A on `geometry`, 1.02x on `text`, 1.02x and 1.04x on the two renders, 1.02x on `search`. One to four
percent, inside the pre-registered 15 percent band and nowhere near the 25 percent line past which I
said to suspect the harness. The prediction held. The instrument designed to test it did not, which
is worth remembering the next time I plan to measure something by subtracting two large numbers.

(Two of the wasm arms link a Rust archive and two do not, and on `render-4x` the two Rust-linked arms
run 3 to 4 percent high in six of seven rounds, while on `null-call` the Rust-linked arm is the
faster one. Those two observations do not tell one story, and `RESULTS.md` records the correlation
without naming a mechanism. I think that is the right call, and it is also why nothing under about 5
percent in that file should be read as a finding.)

Arm D landed within noise of arm C on both platforms, and that is the finding this whole document
was written to obtain. In wasm the `geometry` medians are 1.070 ms and 1.057 ms, with the per-round
ratio of D over C running 0.962x to 1.048x, tighter than the worst same-arm round-to-round spread
measured anywhere in the run. Natively it is 577.37 µs against 581.16 µs, ratio 0.993x to 1.034x.
The one near-exception was wasm `search`, where arm C beat arm D in all seven rounds by 0.1 to
7.4 percent, and that gap did not reproduce natively at all. I do not know whether it was a real difference between `rs_search_page` and
`cc_search_page` on this fixture's 120 hits or something Binaryen did to one static library
and not the other.

So the coarse win belongs to moving the loop, not to the language that holds it. That much is the
measurement, and it stands.

What I drew from it at the time does not. I read it as an argument that the fork should keep growing
coarse `EPDF*` functions in C++, the way
`packages/engine/services/src/features/text/PageTextReader.ts:93` already pulls a whole page of text
through one `EPDFText_GetTextFull` call, and that no Rust layer was needed to collect the win. That
recommendation is superseded by [the runtime decision](./pdfium-rust-wrapper-decision.md). It
assumed the live question was whether to add Rust at all. Under the port that actually got decided,
a new coarse `EPDF*` function buys a win the port already deletes the crossings for, and charges the
same price the old recommendation charged: another coarse operation in C++ written against PDFium's
headers by hand, with no borrow checker and no `Result`, in the part of the stack where a mistake is
a memory-safety bug rather than an exception. Same price, no remaining reason to pay it.

The coarsening win itself is large and real, and it is much larger natively, for the reason the
prediction gave. In wasm, collapsing the per-glyph loop into one crossing takes `geometry` to 0.38x
for both coarse arms, about 2.6x faster than arm A, which is just under the 3x-to-10x band I
predicted. Natively, held within one language and framework, it is 7.16x (arm B against arm C, both
`napi-rs`) and 14.87x (arm A against arm D, both `node-addon-api`). The mechanism is visible in the
null probe, but it is not the same peek for both pairings. Arm A's `mem.peek32` costs 4.0 ns in wasm
and 39.2 ns natively, roughly 9.8x, and that gap is behind the 14.87x figure. Arm B's costs 3.7 ns in
wasm and 15.0 ns natively, about 4x, and that gap is behind 7.16x. Ten times is the wrong multiple
for the pairing that isolates the language.

Arm A's peek is expensive for reasons that have nothing to do with coarsening. It goes through
`raw.peek(P(p), 'i32')` at `bench/arm.mjs:243`, converting the pointer to a BigInt and dispatching on
a JavaScript string type tag on every call, the same shape as the shipping addon's `peek(ptr, kind)`
at `packages/engine/runtime/src/native/native-runtime.ts:57`. `readGlyphRaw` makes up to 21 of those
per glyph, so 8,106 glyphs is on the order of 170,000 calls through that path in one `geometry`
iteration. So 14.87x is pricing string-tag dispatch, BigInt marshalling, and node-addon-api's own
per-call cost together, not "coarsening held within C++" on its own, and arm D's coarse call never
pays any of those three. That is three distinct effects riding on one number, not one effect scaled
up. 7.16x is the figure I trust more: arm B's peek is a direct `f64` call into `napi-rs` with no
string dispatch and no BigInt conversion, so it avoids all three.

`search` went the other way, and it is the only place the direction of a prediction failed. Native
coarsening wins only 1.62x and 2.06x on that workload, smaller than wasm's 3.2x to 3.3x, not larger.
The likely reason is that `searchThin` is call-bound rather than peek-bound, and a native call costs
200.3 ns against wasm's 351.2 ns, so the thing being collapsed away was already the cheaper of the
two. That is inferred from the shape of the loop and the null-probe cells, not measured directly.
The `text` prediction failed more quietly: I said arm C would win natively on the buffer copy, and
all four arms land within about 2 percent of each other. That is not a coarsening result at all.
`textThin` and `textCoarse` both make exactly one crossing and then the identical `mem.readU16`, so
the workload never had a thin side for coarsening to beat; the prediction was never testable on this
shape of call, and the near-flat spread says that rather than saying anything about C versus native
`napi-rs`. Render came out within noise on both platforms, as predicted, which at this point is the
least interesting sentence in the document.

Three of this study's own measurement bugs each reversed or manufactured a conclusion, and all three
were invisible to the fairness gate, because a wrong measurement and a right answer coexist happily.
The coarse decoder allocated a throwaway array and a closure per rect and per quad, roughly 26,000
extra allocations an iteration, paid only by the coarse arms: `geometry` read 1.57x slower for arm C
before that fix and 0.38x after. `build/link-wasm.sh` passed no `-O` flag at all, so `em++` defaulted
to `-O0` and arm D's C++ was built unoptimized against arm C's `-O3` Rust, which manufactured a 25 to
33 percent Rust-over-C++ win in all seven rounds that vanished the moment the flags were equalized.
And arm order was never rotated within a round while background load decayed inside every round, so
arm A systematically ran under the heaviest machine. Every one of the three lived in code this plan
specified. I think that is worth more to a future reader than any single number above: the gate
proves the arms agree, and it will never tell you they are agreeing for different amounts of work.

One number in `RESULTS.md` looks like a native wrapper tax and is not. Native arm A is
`node-addon-api` C++ with BigInt pointers and native arm B is `napi-rs` Rust with `f64` pointers, so
the 8.642 ms against 4.134 ms on `geometry`, and the 39.2 ns against 15.0 ns on `null-peek`, mix
language, binding framework and pointer representation in one gap. It cannot be reported as a
wrapper tax and I am not reporting it as one. But it is a 2x difference on the read that
`PageGeometryReader` performs up to 21 times per glyph, and separating the two conventions is cheap:
add an f64 entry point to the existing C++ addon and time both through one framework. If the BigInt
convention is most of it, that is a change to argument marshalling with no port attached to it.

All of this is one machine, one fixture page, one PDF. It says nothing about whether the 16,281
lines of `engine-services` port well, and nothing about iOS or Android, which is the actual argument
for the port. The case was never performance. It still isn't.
