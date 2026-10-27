# Start here: the Rust port

> Current build hosted at: https://embed-pdf-rust-kernel-eval.roan-util-cloudfare.workers.dev

This branch, `research/multi-platform-rust-core-impl-plugins`, runs EmbedPDF's kernel and plugins
in Rust, compiled to WASM under the web viewer. Separately, `platforms/apple` and
`platforms/android` package the Rust engine core for Swift and Kotlin. The viewer chrome stays
TypeScript. The engine is TypeScript over PDFium, with a Rust core linked into the
PDFium payload that today serves page geometry only. Nothing here is published, and it's a
research branch, not a release candidate.

Read these five, in order. Everything else at the root is a detail they link to.

1. [`multiplatform-plan.md`](multiplatform-plan.md): the plan. The goal and the architecture, one
   Rust core consumed by the web, Swift and Kotlin.
2. [`multiplatform-plugins-plan.md`](multiplatform-plugins-plan.md): why the kernel and plugins
   moved too, and the method. The shipping TypeScript is the oracle, and recorded vectors in
   `tooling/plugin-conformance/vectors/` hold the Rust to it.
3. [`examples/rust-kernel-eval/README.md`](examples/rust-kernel-eval/README.md): building and
   running the web viewer on the Rust kernel. Read "Building it" below first.
4. [`docs/research/rust-plugins-web-eval.md`](docs/research/rust-plugins-web-eval.md): a manual
   pass over the viewer, feature by feature, against the TypeScript viewer on the same PDFs.
5. [`perf-compare/FINDINGS.md`](perf-compare/FINDINGS.md): performance against `main`. Read the
   numbers below before its opening section, which is dated 2026-09-29 and out of date.

## Building it

`pnpm build` and `pnpm preview` in `examples/rust-kernel-eval` give you a static build of the
viewer on the Rust kernel. Alongside the eval README's prerequisites you'll need `wasm-opt`
(Binaryen), which `crates/epdf-web/scripts/build.sh` runs on the kernel and the README doesn't
list.

PDFium is one command. `pnpm --filter @embedpdf/engine-runtime fetch:payload` downloads the
wasm32 payload, with the Rust core linked in, from a `runtime-payloads-<hash>` release on
[`roanutil/embed-pdf-runtime`](https://github.com/roanutil/embed-pdf-runtime/releases). The hash
covers the Rust crates `epdf-wasm` links as well as the runtime's own build inputs
(`ef318e5cb82aac78` at the time of writing), so a checkout only fetches the payload its sources
produce. The cost is that every change reaching the payload needs a publish before anyone can
fetch it. Until then `fetch:payload` fails and names the missing release. To publish, run
`node scripts/publish-engine-payload.mjs` with `PERF_WORK_DIR` pointing at a `perf-compare/.work`
that has the PDFium fork built. Without that it starts a fork build that takes hours.

## Performance against `main`

On the desktop profile (10 rounds, Apple M1 `macmini2020`), the plugins tip at `faa2c6189` is
1.097x `main` on the median row's latency and 1.571x on its main-thread time. Under a 4x CPU
throttle, measured one fix earlier at `c0fcd2746`, it's 1.319x and 1.986x. Both come from
"Page geometry in one pass" and "Store notifications, at most once a frame" in `FINDINGS.md`.

So the Rust viewer is now close to `main` on wall time and still does about half again as much
main-thread work. `open(large)` is no longer behind. `scroll` is the gap that's left: 354 ms of
main-thread JavaScript against 208 on an M4 Max, and about 80 ms of that is each call's arguments
and result crossing the WASM boundary as JSON ("Where the plugins tip goes now" in
`FINDINGS.md`). `45c920b26`, merged here from `research/rust-kernel-plain-args`, takes about
17 ms of that. In its own run against `main`, the median row's main-thread time went from 1.55x
to 1.503x, and its latency read 1.063x to 1.073x, inside the noise between arms.

The engine's Rust geometry reader doesn't move these numbers either way. Geometry is about 2% of
the worker's time in the `select` flow (5.8 ms in TypeScript, 5.4 in Rust), and search doesn't use
it. The cost it does carry is size: linking the core grows the engine WASM from 7.19 to 10.57 MiB,
mostly Stage B's 65-op `Host`, and that growth hasn't been timed on its own.

## Where it stands

- Port coverage, with the tests that pin it:
  [`multiplatform-port-status.md`](multiplatform-port-status.md) for the engine and
  [`multiplatform-plugins-status.md`](multiplatform-plugins-status.md) for the plugins. The engine
  page is from late September with its counts refreshed at `172f5369d`; the plugins page now
  carries a "Catch-up to upstream" section (Task 13 and Task 14.1 to 14.7, `63f1219c7` to
  `172f5369d`) with what is left for you: the XCFramework and iOS run, the 10 `core-wasm` `test:wasm`
  failures that predate Task 14, and the generator-owned carries.
- Review findings on the plugins branch (0 critical, 4 high, 34 medium, 89 low):
  [`docs/research/rust-plugins-branch-review.md`](docs/research/rust-plugins-branch-review.md).
- Native packaging, built locally and published nowhere:
  [`platforms/apple/README.md`](platforms/apple/README.md) and
  [`platforms/android/README.md`](platforms/android/README.md).
- How the benchmark works: [`perf-compare/README.md`](perf-compare/README.md).
  `perf-compare/RESULTS.md` is about the engine-only branch,
  `research/multi-platform-rust-core-impl`, not this one.

You can skip the working plans under `docs/superpowers/plans/` and in
`viewer-perf-compare-plan.md`.
