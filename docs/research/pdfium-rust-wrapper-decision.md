# PDFium in a Rust core: what to do with `engine/runtime`

Keep the fork as a C ABI and bind to it from Rust. Don't add coarse `EPDF*` wrappers to collapse
crossings, because the port deletes those crossings for free. Put the one boundary that still costs
anything at JS to Rust, per operation, which is where `engine-core`'s wire protocol already sits.

This document assumes the Rust core is happening, with platform-native wrappers for web and mobile.
That premise changes what the benchmark means. An earlier version of this file read the same numbers
as an argument against adding Rust at all; that reading is moot now, and the numbers say something
more useful under the premise that actually holds.

Full tables and caveats are in [`pdfium-binding-benchmarks/RESULTS.md`](../../pdfium-binding-benchmarks/RESULTS.md).
One machine (Apple M4 Max, Node 24.20.0, emsdk 3.1.72, rustc 1.98.0), one fixture page
(`examples/snippet-react/public/report.pdf` page 4, 8,106 glyphs), seven interleaved rounds with arm
order rotated per round, behind a fairness gate proving all four arms compute identical answers. The
numbers below are from a re-run of that benchmark: a bug in `bench/run.mjs` had cleared the wasm
results out from under the first published wasm tables, so this document, `RESULTS.md`, and
`pdfium-rust-wrapper-benchmarks.md` were all updated together from one sitting with committed raw
data behind every figure.

## What the numbers mean now

The benchmark compared four arms: PDFium alone (a), a 1:1 Rust shim (b), four coarse operations in
Rust (c), and the same four in C++ (d). Read under the port, three results matter.

**The expensive boundary is the one the port removes.** Arms c and d ran the same roughly 25,000
PDFium calls per page as arm a. The difference is that they made them inside the module instead of
across it, and that alone bought 2.6x in wasm and 7.16x natively on glyph geometry. Once
`PageGeometryReader` is Rust, its per-glyph loop is Rust calling C in-process. Those 25,000 crossings
stop existing. We don't have to engineer that win, we inherit it.

**A Rust frame in front of PDFium is nearly free.** Arm b costs 1 to 4 percent in wasm on workloads
doing real work, and grows the module by 3,802 bytes against arm a's 4,676,714. That's the cost of a
thin Rust binding over the C API, measured from the worst side of it: arm b was reached from
JavaScript through `cwrap`, where the Rust core will be calling straight through. So a `pdfium-sys`
style binding is not a thing to optimize. Build it the boring way.

**Which language owns the loop doesn't matter.** Arms c and d are indistinguishable: `d/c` between
0.962 and 1.048 on geometry in wasm, and inseparable on every native workload. This used to be an
argument for keeping the loop in C++. Under a Rust core it's the opposite, and better: the loop can
live wherever it's cheapest to maintain, and that's Rust, next to the rest of `engine-services`, at
no measured performance cost.

## Recommendations

1. **Bind, don't extend.** Keep the fork's C ABI as the interface and generate Rust bindings with
   `bindgen` over the same headers `packages/engine/runtime/build/generate-functions.mjs` already
   walks, using the same `FPDF|EPDF|FORM|PDFiumExt_` filter. The spike hand-wrote 21 declarations in
   `pdfium-binding-benchmarks/crates/pdfium-sys/src/lib.rs` to dodge a sysroot configuration problem; that
   shortcut isn't worth carrying into the real thing.

2. **Stop adding `EPDF*` functions whose only job is collapsing crossings.** That was the right
   instinct when the caller was JavaScript, and `packages/engine/services/src/features/text/PageTextReader.ts:93`
   is a good example of it paying off. It stops paying the moment the caller is Rust. Keep adding
   `EPDF*` where it saves work inside PDFium rather than at the boundary:
   `EPDFText_GetCharGeometry` earns its place because it gathers boxes, quads, matrix and flags from
   one traversal instead of recomputing per field, and that value survives the port intact.

3. **Make the JS-to-Rust boundary coarse, per operation.** This is the boundary that still costs
   something, and the numbers put a price on getting it wrong: a per-glyph JS boundary costs 2.6x to
   7.16x more than a per-page one. `packages/engine/core/src/wire/worker-protocol.ts` is already
   shaped this way, one message per operation, so the existing contract is the right one to keep.
   Resist any convenience API that reintroduces a per-item JS call.

4. **On Node, use `napi-rs` with `f64` pointers, not BigInt.** The shipping addon takes 8.642 ms on
   glyph geometry where the `napi-rs` addon takes 4.134 ms for the same PDFium calls in the same
   order, and 39.2 ns against 15.0 ns on a single `peek`. We can't attribute that cleanly, because
   the comparison changed the binding framework, the pointer representation, and the string type-tag
   dispatch at `packages/engine/runtime/src/native/native-runtime.ts:57` all at once. A coarse
   boundary makes it mostly moot, which is another reason to prefer one, but the default should be
   the cheap convention.

5. **Link Rust into the existing Emscripten module.** A Rust staticlib built for
   `wasm32-unknown-emscripten` links into the same `em++` command as `libembedpdf.a`, proven in
   `pdfium-binding-benchmarks/build/link-wasm.sh`. This answers wave 0 of [the port plan](./rust-core-port.md),
   which called the question genuinely open and made everything after it contingent. No second
   module, no `wasm32-wasip1` rebuild, no cross-module import cost. One caveat for whoever does it:
   `-O3` makes `em++` run `wasm-opt`, and emsdk 3.1.72's bundled v119 rejects two wasm features in
   rustc 1.98.0's precompiled std, so a newer `wasm-opt` is needed.

6. **Publish iOS and Android artifacts through the fork's release matrix.** As of
   the [2026-09-08 audit](./platform-gap-audit.md), the local fork builds the mobile
   targets, but release CI and the latest published release still cover nine targets.
   Local artifacts supported successful iOS simulator and Android emulator renders.
   Complete release automation, publish through an authorized maintainer, and verify
   device packaging before shipping. This benchmark does not establish mobile performance.

## The one real tradeoff

If wave 2 of the port is far out, there's a case for adding a coarse `EPDF_GetPageGeometryFull` to
the fork now as a bridge. `pdfium-binding-benchmarks/cpp/ops.cc`'s `cc_read_page_geometry` is a working
reference that already passes the fairness gate against the current reader, so it's close to free to
land, and it buys 2.6x on web and 7.16x on Node against today's TypeScript reader.

The cost is that it's throwaway. Once the Rust core owns `PageGeometryReader`, that function has no
caller, and it's a public C API in a fork we'd then need a reason to remove. Worth it if the port is
quarters away and the geometry path is hurting users now. Not worth it otherwise, and we don't think
it is otherwise.

## What this doesn't settle

Nothing here says whether 16,281 lines of `engine-services` port well, which is wave 2 and the
actual risk. Nothing here touches iOS or Android performance, or the Swift and Kotlin binding
surface. And every number is one machine, one PDF, one page.

One caution on trusting any of it. The study found three of its own measurement bugs in development,
and each reversed or manufactured a conclusion: a decoder allocating roughly 26,000 objects per
iteration that only the coarse arms paid, arm d compiled at `-O0` against arm c's `-O3`, and arm
order never rotated within a round. All three were arms doing different amounts of work for reasons
unrelated to the thing under test, and the fairness gate caught none of them, because the answers
stayed identical throughout. A fourth of the same kind is the most likely way these numbers are
still wrong.
