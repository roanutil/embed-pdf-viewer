Run the gate inside the approved development container:

```sh
node tooling/dotnet-parity/run.mjs --manifest tooling/dotnet-parity/manifest.json
```

The gate compares the **real packed React viewer**, opened at `e2e.html`, with
`platforms/dotnet/EmbedPDF.Parity`. It reuses the existing viewer Playwright
configuration and its isolated preview on port 5241. Only headless Chromium
and explicitly disabled Rust routing are accepted. The bridge observes the
same local engine document the viewer opened, then reads public text/layout
and raw render services. Worker initialization and geometry replies are
recorded independently; the existing requested-backend label is never proof.

1. Install with the lockfile; build the viewer and its dependencies using the
   repository build tooling. Fetch only the checksum-verified runtime pinned
   by `packages/engine/runtime/engine-runtime-build.json`.
2. Run `node tooling/dotnet-parity/fixtures/prepare-packed.mjs` after those
   builds. This installs local packed tarballs into
   `/tmp/embedpdf-dotnet-parity/vite-app` and builds the real e2e entry.
3. Run `node tooling/dotnet-parity/run.mjs --pin-packed /tmp/embedpdf-dotnet-parity/vite-app`.
   Review the resulting package integrity, production-file SHA256s and pins.
   Keep generated tarballs/builds local. A missing or changed artifact fails.
4. Build `epdf-capi` with `CARGO_BUILD_JOBS=2 cargo build -p epdf-capi`.
   Export absolute `EPDF_NATIVE_DIR` and the matching PDFium loader path
   (`LD_LIBRARY_PATH` on Linux). Install Chromium through the existing
   Playwright tooling. Run the default command above.
5. Review `fixtures/records/latest/summary.json` and every per-case
   `*.record.json`. Each uses the Slice1 manual validation-record schema,
   with actual input/artifact/log hashes, exact inner commands, image/SDK/libc
   identity and failures. Commit evidence and review it before a compatibility
   claim. The host heavy-build lock is orchestration, not recorded as a test.

The managed comparator reads actual serialized JSON number tokens as decimal,
verifies parsing preserved every digit, and rounds **once** to four places
with `MidpointRounding.AwayFromZero`. It compares text, map entries, array
order, property presence and strings exactly. Duplicate keys, decimal overflow
and precision loss are refusals. There is no binary-double epsilon. The
`self-test` subcommand includes intentionally different positive/negative
`.00005` and crop/rotation boundaries, plus a repeated-rounding trap.

The corpus includes supplementary/dropped/combining/RTL text maps from the
committed canonical vectors; live embedded-orientation and reverse/gapped
PDFs; arbitrary character selections; empty ranges; repeated quotes and
spaces; shifted CropBox beyond MediaBox; all quarter turns; UserUnit-aware
transform probes and transparent region renders. `fixtures/generate.mjs`
authors the small PDF independently and pins the corpus. Regeneration resets
local packed pins, so review them again.

Page indices are zero-based. Geometry uses PDF points measured from the
top-left of the unrotated visible crop box, with y down. The manifest pins
the tested font bytes **and order**. The initial controlled DejaVu font comes
from the approved image, under its installed licence notice. The controlled bundle is eligible for the corpus gate. The recorded limitation is:
consumer deployment font bundle: compatibility not yet verified; re-run the gate with the supplied bundle before relying on it.

Raster descriptors, alpha convention and every pixel's SHA256 must match
exactly. This runner rejects all tolerance settings. Propose any raster
tolerance separately with mismatch evidence and obtain human approval before
implementing it. Runtime/font/viewer pin bumps require a new manual run.

A failed prerequisite or native crash is a failed case, never a skip or a
replacement snapshot. The existing PDFium1461 held-layer crash is an upstream
limitation until the real font ABI/runtime work lands; switching runtime
libraries without pinning and rerunning the gate does not establish parity.

WASM ABI checks derive their input from the exact pinned
`pdfFunctionSignatures` source through `fixtures/write-abi-input.mjs` and then
run the existing authoritative verifier. Header regeneration is a separate
build input: its output must not silently replace the binding source the
packed runtime actually uses. The records distinguish diagnostic artifacts
from the subsequent restored-source rebuild.
