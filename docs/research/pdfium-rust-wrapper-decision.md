# PDFium Rust wrapper: what to do about it

Don't add a Rust layer to `packages/engine/runtime` for performance. Grow coarse `EPDF*` functions
in the C++ fork instead. That's where the entire measured win lives, and none of it belongs to Rust.

Full tables and caveats are in [`pdfium-rust-spike/RESULTS.md`](../../pdfium-rust-spike/RESULTS.md).
Everything below comes from one machine (Apple M4 Max, Node 24.20.0, emsdk 3.1.72, rustc 1.98.0),
one fixture page (`examples/snippet-react/public/report.pdf` page 4, 8,106 glyphs), seven interleaved
rounds with arm order rotated per round, behind a fairness gate that proves all four arms compute
identical answers on two fixtures before anything is timed.

## What we measured

Four arms of the same runtime, in wasm and again natively. Arm a is PDFium as it ships. Arm b is a
1:1 Rust shim, same call count, one extra frame. Arm c runs four coarse operations in Rust so a page
of glyph geometry costs one boundary crossing instead of roughly 25,000. Arm d is those same four
operations in C++.

**Coarsening the crossing is worth a lot.** In wasm, geometry runs 2.6x faster and search 3.3x.
Natively it's 7.10x on geometry, comparing arm b against arm c so the language and binding framework
are held fixed. The native win is larger for the reason we predicted: `mem.peek` is a JavaScript
typed-array read in wasm and a real N-API round trip natively, and `PageGeometryReader` makes up to
21 of them per glyph.

**Arms c and d are indistinguishable, on both platforms.** In wasm, `d/c` sits between 0.975 and
1.004 on geometry. Natively they're inseparable on every workload. Whatever coarsening buys, it buys
the same whether the loop is written in Rust or in C++.

**The thin Rust wrapper is nearly free, and so is its bundle cost.** Arm b costs 1 to 4 percent in
wasm on workloads that do real work. The module grows by 3,802 bytes for the shim and 4,774 for the
coarse Rust, against arm a's 4,676,714. We predicted 100 to 400 kB and were wrong by two orders of
magnitude.

**Render and text show nothing.** All four arms land within 1 percent natively and within 5 percent
in wasm. Render is dominated by PDFium's own bitmap work, and `PageTextReader.ts:93` was already
bulk before any of this.

## Recommended next steps

1. **Add a coarse glyph-geometry function to the fork, in C++.** `pdfium-rust-spike/cpp/ops.cc`'s
   `cc_read_page_geometry` is a working reference implementation that already passes the fairness
   gate against the current reader. Promoting it to a real `EPDF*` entry point collects the whole
   2.6x-to-7.1x win with no port, no new toolchain, and no second language in the build. This is the
   one recommendation we'd act on first.

2. **Chase the native per-call gap separately.** On `geometry` the shipping addon takes 8.374 ms
   where the `napi-rs` addon takes 4.000 ms for the same PDFium calls in the same order, and on a
   single `peek` it's 38.0 ns against 14.7 ns. We can't attribute that from these numbers, because
   the benchmark changed the binding framework, the pointer representation, and the type-tag
   dispatch at `packages/engine/runtime/src/native/native-runtime.ts:57` all at once. The experiment
   that separates them is cheap and involves no Rust: add an `f64`-pointer entry point to the
   existing C++ addon and time both conventions through one framework. If the BigInt convention is
   most of it, that's a change to argument marshalling and nothing more.

3. **Treat sub-5-percent cross-arm differences as unresolved.** Each arm is a separately linked
   binary, and code layout alone moves PDFium-dominated timings by a few percent. The render
   correlation with Rust-linked arms is at that scale and its discriminating experiment (link
   `libshim.a` into a module that never calls it) hasn't been run.

## The best path, considering only these results

Take the coarsening win in C++ in the fork, and leave the runtime otherwise alone. Nothing here
justifies a Rust layer on performance grounds, on web or on Node.

That's a narrower conclusion than it sounds. This study says nothing about whether 16,281 lines of
`engine-services` port well, and nothing about iOS or Android, which is the actual argument in
[the port plan](./rust-core-port.md). If the Rust core happens, it should happen for code sharing
across platforms, and these numbers neither help nor hurt that case.

Two findings do help it, and they're worth carrying forward. The linkage question that plan called
genuinely open is answered: a Rust staticlib built for `wasm32-unknown-emscripten` links into the
same `em++` command as `libembedpdf.a`, proven and in the repo. And the wrapper tax is small enough
that the boundary shape isn't an obstacle. So if the port proceeds on mobile grounds, the web build
won't pay much for it. We just shouldn't start it expecting to get faster.

One caution on reading any of the above. The study found three of its own measurement bugs during
development, and each one reversed or manufactured a conclusion: a decoder allocating roughly 26,000
objects per iteration that only the coarse arms paid, arm d compiled at `-O0` against arm c's `-O3`,
and arm order never rotated within a round. All three were arms doing different amounts of work for
reasons unrelated to the wrapper, and the fairness gate caught none of them, because the answers
stayed identical throughout. A fourth of the same kind is the most likely way these numbers are
still wrong.
