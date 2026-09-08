# PDFium Rust Wrapper Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure what it costs to put Rust between TypeScript and the PDFium fork in `packages/engine/runtime`, in wasm first and then native, across four arms and five workloads.

**Architecture:** A self-contained `pdfium-binding-benchmarks/` directory at the repo root, sibling to `viewer-rust-port-benchmarks/`. It fetches the same prebuilt `libembedpdf.a` the shipping build uses, relinks it four ways with `em++` (baseline, plus a Rust thin shim, plus a Rust coarse layer, plus a C++ coarse layer), and drives all four through one zero-dependency benchmark harness borrowed from `viewer-rust-port-benchmarks/bench/harness.mjs`. A fairness gate compares canonical outputs before any timing runs. The native phase reuses every workload and the whole harness, swapping Emscripten exports for `napi-rs` and `cmake-js`.

**Tech Stack:** Rust 1.98.0 (`wasm32-unknown-emscripten`, `aarch64-apple-darwin`), Emscripten 3.1.72, `napi-rs` 2.x, `cmake-js` 7.3.1, Node 24.20.0, plain ESM with no test framework beyond `node:test`.

**Spec:** `docs/research/pdfium-rust-wrapper-benchmarks.md`

## Global Constraints

- Node 24.20.0 and pnpm 10.34.0, pinned in `mise.toml`. Run `mise install` if either is missing.
- Emscripten 3.1.72, matching `scripts/dev.sh:7`. Do not use 3.1.70 from `packages/engine/runtime/Dockerfile:2`; the two disagree and 3.1.72 is the one the local dev path installs.
- The wasm artifact is built **without** `WASM_BIGINT`. Emscripten legalizes every `i64` parameter into an `(i32 low, i32 high)` pair at the JS boundary. Every function this spike exports must therefore use `i32`/`f32`/`f64` parameters only. No `i64`, no `u64`, no pointers wider than 32 bits.
- `pdfium-binding-benchmarks/` must not be added to `pnpm-workspace.yaml` and must not be referenced by `turbo.json`. It is a spike; nothing published depends on it.
- Never modify `packages/engine/runtime/build/compile.esm.sh`. `pdfium-binding-benchmarks/build/link-wasm.sh` is a deliberate copy.
- Every benchmark function passed to `measure()` must return a number. The harness accumulates it into a sink; an unconsumed call is one V8 may delete.
- Report median and p95. Never mean.
- Timing runs are round-robin across arms, `k = 7` rounds.

## Deviations from the spec

Two, both deliberate, both cheaper than what the spec described.

1. **No `bindgen`.** The spec says `bindgen` over the fetched headers. Targeting `wasm32-unknown-emscripten` with `bindgen` needs clang pointed at the Emscripten sysroot, which is a build-configuration problem with no bearing on the measurement. The workloads need 21 PDFium functions. This plan hand-writes those 21 `extern "C"` declarations in `crates/pdfium-sys/src/lib.rs` instead. If the spike graduates into a real port, `bindgen` comes back then.

2. **The workloads transcribe the engine's call pattern rather than importing it.** `PageGeometryReader` at `packages/engine/services/src/features/geometry/PageGeometryReader.ts:86` depends on `DocumentSession`, `EngineError` and a built `@embedpdf/engine-runtime` dist. Importing that chain would drag the whole workspace build into a spike. Task 3 transcribes the glyph loop, call for call, into `bench/workloads.mjs`. This measures the shape of the real reader, not the reader itself, and the plan says so where the results get written.

---

## File Structure

```
pdfium-binding-benchmarks/
  .gitignore              build outputs, fetched archives, results
  README.md               what this is, how to run it, and that it is a spike
  Cargo.toml              workspace: pdfium-sys, shim, ops, napi-arms
  rust-toolchain.toml     pins 1.98.0 + the two targets
  crates/
    pdfium-sys/src/lib.rs   21 extern "C" declarations + FS_* structs
    shim/src/lib.rs         arm B: one rs_* per PDFium function
    ops/src/lib.rs          arm C: four coarse operations
    ops/src/record.rs       the 112-byte glyph record writer, shared by C
    napi-arms/src/lib.rs    native arms B and C behind napi-rs
  cpp/
    ops.cc                  arm D: the same four coarse operations in C++
    record.h                the 112-byte glyph record layout, shared by D
  build/
    fetch.sh                pulls libembedpdf.a via the shipping fetch script
    emsdk.sh                installs and activates emsdk 3.1.72
    link-wasm.sh            builds all four wasm arms
    build-native.sh         builds all four native arms
    CMakeLists.txt          arm D native, mirrors the shipping one
  bench/
    arm.mjs                 loads one arm, returns { fn, mem, close }
    fixture.mjs             opens the PDF, picks the page, caches the choice
    workloads.mjs           the five workloads, parameterized by arm
    gate.mjs                canonical outputs + cross-arm comparison
    node.mjs                one arm, one round, writes a results JSON
    run.mjs                 round-robin driver
    compare.mjs             reads results/, prints tables and markdown
    results/                gitignored
  RESULTS.md               the write-up
```

Responsibilities are split so that `arm.mjs` is the only file that knows how an arm is loaded, `workloads.mjs` is the only file that knows what is measured, and `gate.mjs` is the only file that knows what "the same answer" means. Adding a fifth arm should touch only `arm.mjs`.

---

### Task 1: The linkage gate

The whole spike is contingent on this. One Rust function, linked into an `em++` command line alongside `libembedpdf.a`, called from Node. **Timebox: one day.** If it does not link, stop and write up why; that is a more valuable result than any timing.

**Files:**
- Create: `pdfium-binding-benchmarks/.gitignore`
- Create: `pdfium-binding-benchmarks/Cargo.toml`
- Create: `pdfium-binding-benchmarks/rust-toolchain.toml`
- Create: `pdfium-binding-benchmarks/crates/pdfium-sys/Cargo.toml`
- Create: `pdfium-binding-benchmarks/crates/pdfium-sys/src/lib.rs`
- Create: `pdfium-binding-benchmarks/crates/shim/Cargo.toml`
- Create: `pdfium-binding-benchmarks/crates/shim/src/lib.rs`
- Create: `pdfium-binding-benchmarks/build/fetch.sh`
- Create: `pdfium-binding-benchmarks/build/emsdk.sh`
- Create: `pdfium-binding-benchmarks/build/link-probe.sh`
- Test: `pdfium-binding-benchmarks/bench/probe.test.mjs`

**Interfaces:**
- Consumes: nothing.
- Produces: `crates/pdfium-sys` exporting `FS_SIZEF`, `FPDF_InitLibrary()`, `FPDF_LoadMemDocument(*const u8, i32, *const i8) -> u32`, `EPDF_GetPageSizeByIndexNormalized(u32, i32, *mut FS_SIZEF) -> i32`. Handles are `u32` throughout, never pointers, because the spike must stay `i64`-free. `crates/shim` exporting `rs_probe_page_size(doc: u32, index: i32, out: *mut f32) -> i32`.

- [ ] **Step 1: Create the directory skeleton and Rust workspace**

`pdfium-binding-benchmarks/.gitignore`:

```gitignore
target/
build/libpdfium/
build/cache/
build/out/
build/emsdk/
bench/results/
node_modules/
```

`pdfium-binding-benchmarks/rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.98.0"
targets = ["wasm32-unknown-emscripten", "aarch64-apple-darwin"]
```

`pdfium-binding-benchmarks/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/pdfium-sys", "crates/shim"]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"
```

`pdfium-binding-benchmarks/crates/pdfium-sys/Cargo.toml`:

```toml
[package]
name = "pdfium-sys"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["rlib"]
```

`pdfium-binding-benchmarks/crates/shim/Cargo.toml`:

```toml
[package]
name = "shim"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["staticlib", "rlib"]

[dependencies]
pdfium-sys = { path = "../pdfium-sys" }
```

- [ ] **Step 2: Declare the four PDFium functions the probe needs**

`pdfium-binding-benchmarks/crates/pdfium-sys/src/lib.rs`:

```rust
//! Hand-written declarations for the slice of the PDFium C API this spike
//! touches. Handles are `u32`, not pointers: the wasm artifact is linked
//! without WASM_BIGINT, so anything 64 bits wide gets legalized at the JS
//! boundary and the spike stays clear of that entirely.
#![allow(non_camel_case_types, non_snake_case)]

pub type FPDF_DOCUMENT = u32;
pub type FPDF_BOOL = i32;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

extern "C" {
    pub fn FPDF_InitLibrary();
    pub fn FPDF_DestroyLibrary();
    pub fn FPDF_LoadMemDocument(
        data_buf: *const u8,
        size: i32,
        password: *const i8,
    ) -> FPDF_DOCUMENT;
    pub fn FPDF_CloseDocument(document: FPDF_DOCUMENT);
    pub fn EPDF_GetPageSizeByIndexNormalized(
        document: FPDF_DOCUMENT,
        page_index: i32,
        size: *mut FS_SIZEF,
    ) -> FPDF_BOOL;
}
```

- [ ] **Step 3: Write the probe function**

`pdfium-binding-benchmarks/crates/shim/src/lib.rs`:

```rust
//! Arm B. Every export is `rs_`-prefixed, takes and returns only i32/f32/f64,
//! and does exactly what the corresponding PDFium function does. The point is
//! that the call count is identical to arm A and the only difference is one
//! extra wasm frame.
#![allow(non_snake_case)]

use pdfium_sys::*;

/// Writes width into out[0] and height into out[1]. Returns 1 on success.
#[no_mangle]
pub extern "C" fn rs_probe_page_size(doc: u32, index: i32, out: *mut f32) -> i32 {
    let mut size = FS_SIZEF::default();
    let ok = unsafe { EPDF_GetPageSizeByIndexNormalized(doc, index, &mut size) };
    if ok == 0 {
        return 0;
    }
    unsafe {
        *out = size.width;
        *out.add(1) = size.height;
    }
    1
}
```

- [ ] **Step 4: Write the fetch and emsdk scripts**

`pdfium-binding-benchmarks/build/fetch.sh`:

```bash
#!/usr/bin/env bash
# Pull the same prebuilt libembedpdf.a the shipping build uses, pinned by
# sha256 in packages/engine/runtime/engine-runtime-build.json. No PDFium
# source build is needed anywhere in this spike.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME="$ROOT/../packages/engine/runtime"
TARGET="${1:-wasm32}"

PDF_RUNTIME_LIB_DIR="$ROOT/build/libpdfium" \
  bash "$RUNTIME/scripts/fetch-libpdfium.sh" "$TARGET"

echo "libembedpdf for $TARGET in $ROOT/build/libpdfium/$TARGET"
```

`pdfium-binding-benchmarks/build/emsdk.sh`:

```bash
#!/usr/bin/env bash
# Install and activate emsdk 3.1.72, matching scripts/dev.sh:7. Source this
# script rather than running it: it exports PATH entries em++ needs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EMSDK_VERSION="3.1.72"
EMSDK_DIR="$ROOT/build/emsdk"

if [[ ! -d "$EMSDK_DIR/.git" ]]; then
  git clone https://github.com/emscripten-core/emsdk.git "$EMSDK_DIR"
fi
"$EMSDK_DIR/emsdk" install "$EMSDK_VERSION"
"$EMSDK_DIR/emsdk" activate "$EMSDK_VERSION"
# shellcheck disable=SC1091
source "$EMSDK_DIR/emsdk_env.sh"
```

- [ ] **Step 5: Write the probe link script**

`pdfium-binding-benchmarks/build/link-probe.sh`:

```bash
#!/usr/bin/env bash
# The gate. Builds the Rust staticlib for wasm32-unknown-emscripten and puts
# it on the SAME em++ line as libembedpdf.a, exactly the way
# packages/engine/runtime/build/compile.esm.sh:35 links the shipping artifact.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out/probe"

# shellcheck disable=SC1091
source "$ROOT/build/emsdk.sh"

cargo build --manifest-path "$ROOT/Cargo.toml" \
  -p shim --target wasm32-unknown-emscripten --release

RUST_LIB="$ROOT/target/wasm32-unknown-emscripten/release/libshim.a"
test -f "$RUST_LIB" || { echo "missing $RUST_LIB" >&2; exit 1; }

mkdir -p "$OUT_DIR"
em++ "$LIB_DIR/lib/libembedpdf.a" "$RUST_LIB" \
  -sENVIRONMENT=node \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sWASM=1 \
  -sALLOW_MEMORY_GROWTH=1 \
  -sALLOW_TABLE_GROWTH=1 \
  -sEXPORT_NAME=createProbe \
  -sASSERTIONS=1 \
  -sEXPORTED_RUNTIME_METHODS=cwrap,getValue,setValue,UTF8ToString,UTF16ToString,stringToUTF8,wasmExports \
  -sEXPORTED_FUNCTIONS=_malloc,_free,_FPDF_InitLibrary,_FPDF_LoadMemDocument,_FPDF_CloseDocument,_EPDF_GetPageSizeByIndexNormalized,_rs_probe_page_size \
  -I"$LIB_DIR/include" \
  -std=c++17 --no-entry \
  -o "$OUT_DIR/probe.mjs"

echo "linked $OUT_DIR/probe.mjs"
```

- [ ] **Step 6: Write the failing test**

`pdfium-binding-benchmarks/bench/probe.test.mjs`:

```js
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

const here = import.meta.dirname;
const PDF = resolve(here, '../../examples/snippet-react/public/report.pdf');

test('a Rust function linked into the PDFium wasm module returns the same page size as PDFium', async () => {
  const { default: createProbe } = await import('../build/out/probe.mjs');
  const m = await createProbe();

  m.cwrap('FPDF_InitLibrary', null, [])();

  const bytes = readFileSync(PDF);
  const buf = m._malloc(bytes.length);
  m.HEAPU8.set(bytes, buf);
  const doc = m.cwrap('FPDF_LoadMemDocument', 'number', ['number', 'number', 'number'])(
    buf,
    bytes.length,
    0,
  );
  assert.notEqual(doc, 0, 'FPDF_LoadMemDocument returned null');

  const out = m._malloc(8);

  const direct = m.cwrap('EPDF_GetPageSizeByIndexNormalized', 'number', [
    'number',
    'number',
    'number',
  ])(doc, 0, out);
  assert.equal(direct, 1);
  const directW = m.getValue(out, 'float');
  const directH = m.getValue(out + 4, 'float');

  const viaRust = m.cwrap('rs_probe_page_size', 'number', ['number', 'number', 'number'])(
    doc,
    0,
    out,
  );
  assert.equal(viaRust, 1, 'rs_probe_page_size failed');
  assert.equal(m.getValue(out, 'float'), directW);
  assert.equal(m.getValue(out + 4, 'float'), directH);

  assert.ok(directW > 0 && directH > 0, `implausible page size ${directW}x${directH}`);

  m._free(out);
  m._free(buf);
  m.cwrap('FPDF_CloseDocument', null, ['number'])(doc);
});
```

- [ ] **Step 7: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/probe.test.mjs
```

Expected: FAIL with `Cannot find module '.../build/out/probe.mjs'`.

- [ ] **Step 8: Fetch, link, and run**

```bash
cd pdfium-binding-benchmarks
chmod +x build/*.sh
bash build/fetch.sh wasm32
bash build/link-probe.sh
node --test bench/probe.test.mjs
```

Expected: PASS, one test.

If `em++` fails with an error naming an unsupported wasm object version or a relocation it cannot handle, that is the gate failing. Try, in order: (a) `rustup toolchain install` a version whose LLVM matches emsdk 3.1.72 and change `rust-toolchain.toml`; (b) rebuild the Rust as an Emscripten `SIDE_MODULE` with `-sSIDE_MODULE=1` on the Rust link and `-sMAIN_MODULE=2` on the PDFium link, which shares linear memory; (c) stop, and write the failure into `docs/research/pdfium-rust-wrapper-benchmarks.md` under a new `## What happened` heading, quoting the exact `em++` error text. Option (c) ends the spike, and that is an acceptable outcome.

- [ ] **Step 9: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "spike: link a Rust staticlib into the PDFium wasm module"
```

---

### Task 2: Fixture selection

Every workload runs against one page of one PDF. The page has to be chosen by measurement, not by guess, and its glyph count has to end up in the results header.

**Files:**
- Create: `pdfium-binding-benchmarks/crates/pdfium-sys/src/lib.rs` (extend; full contents below)
- Create: `pdfium-binding-benchmarks/build/link-wasm.sh`
- Create: `pdfium-binding-benchmarks/bench/arm.mjs`
- Create: `pdfium-binding-benchmarks/bench/fixture.mjs`
- Test: `pdfium-binding-benchmarks/bench/fixture.test.mjs`
- Delete: `pdfium-binding-benchmarks/build/link-probe.sh` (superseded)

**Interfaces:**
- Consumes: `rs_probe_page_size` from Task 1 (kept, as a liveness check).
- Produces: `loadArm(name)` from `bench/arm.mjs` returning `{ m, fn, mem, close }` where `fn` is an object of cwrapped functions keyed by bare PDFium name and `mem` has `{ alloc, free, peek32, peekF32, readBytes, writeBytes }`. `openFixture(arm)` from `bench/fixture.mjs` returning `{ doc, pageIndex, page, textPage, glyphCount, bytes, close }`.

- [ ] **Step 1: Extend the sys crate to the full 21 functions**

Replace `pdfium-binding-benchmarks/crates/pdfium-sys/src/lib.rs` entirely:

```rust
//! Hand-written declarations for the slice of the PDFium C API this spike
//! touches. Handles are `u32`, not pointers: the wasm artifact is linked
//! without WASM_BIGINT, so anything 64 bits wide gets legalized at the JS
//! boundary and the spike stays clear of that entirely.
#![allow(non_camel_case_types, non_snake_case)]

pub type FPDF_DOCUMENT = u32;
pub type FPDF_PAGE = u32;
pub type FPDF_TEXTPAGE = u32;
pub type FPDF_PAGEOBJECT = u32;
pub type FPDF_BITMAP = u32;
pub type FPDF_SCHHANDLE = u32;
pub type FPDF_BOOL = i32;

pub const FPDFBitmap_BGRA: i32 = 4;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_RECTF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_QUADPOINTSF {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
    pub x4: f32,
    pub y4: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_MATRIX {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

/// Mirrors EPDF_CHAR_GEOMETRY in
/// packages/engine/runtime/runtime-src/public/epdf_text.h:35.
#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct EPDF_CHAR_GEOMETRY {
    pub loose_box: FS_RECTF,
    pub tight_box: FS_RECTF,
    pub loose_quad: FS_QUADPOINTSF,
    pub tight_quad: FS_QUADPOINTSF,
    pub matrix: FS_MATRIX,
    pub flags: u32,
}

pub const EPDF_CHARGEO_HAS_TIGHT_BOX: u32 = 1 << 0;
pub const EPDF_CHARGEO_HAS_LOOSE_QUAD: u32 = 1 << 1;
pub const EPDF_CHARGEO_HAS_TIGHT_QUAD: u32 = 1 << 2;
pub const EPDF_CHARGEO_UPRIGHT: u32 = 1 << 3;
pub const EPDF_CHARGEO_SPACE: u32 = 1 << 4;
pub const EPDF_CHARGEO_EMPTY: u32 = 1 << 5;

extern "C" {
    pub fn FPDF_InitLibrary();
    pub fn FPDF_DestroyLibrary();
    pub fn FPDF_LoadMemDocument(
        data_buf: *const u8,
        size: i32,
        password: *const i8,
    ) -> FPDF_DOCUMENT;
    pub fn FPDF_CloseDocument(document: FPDF_DOCUMENT);
    pub fn FPDF_GetPageCount(document: FPDF_DOCUMENT) -> i32;
    pub fn FPDF_LoadPage(document: FPDF_DOCUMENT, page_index: i32) -> FPDF_PAGE;
    pub fn FPDF_ClosePage(page: FPDF_PAGE);
    pub fn FPDF_GetPageWidthF(page: FPDF_PAGE) -> f32;
    pub fn FPDF_GetPageHeightF(page: FPDF_PAGE) -> f32;
    pub fn EPDF_GetPageSizeByIndexNormalized(
        document: FPDF_DOCUMENT,
        page_index: i32,
        size: *mut FS_SIZEF,
    ) -> FPDF_BOOL;

    pub fn FPDFText_LoadPage(page: FPDF_PAGE) -> FPDF_TEXTPAGE;
    pub fn FPDFText_ClosePage(text_page: FPDF_TEXTPAGE);
    pub fn FPDFText_CountChars(text_page: FPDF_TEXTPAGE) -> i32;
    pub fn FPDFText_GetTextObject(text_page: FPDF_TEXTPAGE, index: i32) -> FPDF_PAGEOBJECT;
    pub fn FPDFText_GetFontSize(text_page: FPDF_TEXTPAGE, index: i32) -> f64;
    pub fn EPDFText_GetCharGeometry(
        text_page: FPDF_TEXTPAGE,
        index: i32,
        geometry: *mut EPDF_CHAR_GEOMETRY,
    ) -> FPDF_BOOL;
    pub fn EPDFText_GetTextFull(
        text_page: FPDF_TEXTPAGE,
        buffer: *mut u16,
        buffer_len: i32,
    ) -> i32;

    pub fn FPDFText_FindStart(
        text_page: FPDF_TEXTPAGE,
        findwhat: *const u16,
        flags: u32,
        start_index: i32,
    ) -> FPDF_SCHHANDLE;
    pub fn FPDFText_FindNext(handle: FPDF_SCHHANDLE) -> FPDF_BOOL;
    pub fn FPDFText_GetSchResultIndex(handle: FPDF_SCHHANDLE) -> i32;
    pub fn FPDFText_GetSchCount(handle: FPDF_SCHHANDLE) -> i32;
    pub fn FPDFText_FindClose(handle: FPDF_SCHHANDLE);
    pub fn FPDFText_GetRect(
        text_page: FPDF_TEXTPAGE,
        rect_index: i32,
        left: *mut f64,
        top: *mut f64,
        right: *mut f64,
        bottom: *mut f64,
    ) -> FPDF_BOOL;
    pub fn FPDFText_CountRects(text_page: FPDF_TEXTPAGE, start: i32, count: i32) -> i32;

    pub fn FPDFBitmap_CreateEx(
        width: i32,
        height: i32,
        format: i32,
        first_scan: *mut core::ffi::c_void,
        stride: i32,
    ) -> FPDF_BITMAP;
    pub fn FPDFBitmap_FillRect(
        bitmap: FPDF_BITMAP,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        color: u32,
    );
    pub fn FPDFBitmap_Destroy(bitmap: FPDF_BITMAP);
    pub fn FPDF_RenderPageBitmap(
        bitmap: FPDF_BITMAP,
        page: FPDF_PAGE,
        start_x: i32,
        start_y: i32,
        size_x: i32,
        size_y: i32,
        rotate: i32,
        flags: i32,
    );
}
```

- [ ] **Step 2: Write the real link script**

`pdfium-binding-benchmarks/build/link-wasm.sh`:

```bash
#!/usr/bin/env bash
# Builds all four wasm arms from ONE em++ invocation each, with identical
# flags. That identity is the whole point: differing flags would mean
# comparing builds rather than comparing wrappers.
#
#   a  libembedpdf.a alone                      (today)
#   b  + libshim.a, one rs_* per PDFium call    (the tax)
#   c  + libops.a, four coarse operations       (the benefit)
#   d  + ops.cc, the same four in C++           (the control)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out"
ARMS="${*:-a b c d}"

# shellcheck disable=SC1091
source "$ROOT/build/emsdk.sh"

PDFIUM_EXPORTS="_malloc,_free\
,_FPDF_InitLibrary,_FPDF_DestroyLibrary,_FPDF_LoadMemDocument,_FPDF_CloseDocument\
,_FPDF_GetPageCount,_FPDF_LoadPage,_FPDF_ClosePage,_FPDF_GetPageWidthF,_FPDF_GetPageHeightF\
,_EPDF_GetPageSizeByIndexNormalized\
,_FPDFText_LoadPage,_FPDFText_ClosePage,_FPDFText_CountChars,_FPDFText_GetTextObject\
,_FPDFText_GetFontSize,_EPDFText_GetCharGeometry,_EPDFText_GetTextFull\
,_FPDFText_FindStart,_FPDFText_FindNext,_FPDFText_GetSchResultIndex,_FPDFText_GetSchCount\
,_FPDFText_FindClose,_FPDFText_GetRect,_FPDFText_CountRects\
,_FPDFBitmap_CreateEx,_FPDFBitmap_FillRect,_FPDFBitmap_Destroy,_FPDF_RenderPageBitmap"

SHIM_EXPORTS="_rs_probe_page_size\
,_rs_FPDFText_CountChars,_rs_FPDFText_GetTextObject,_rs_FPDFText_GetFontSize\
,_rs_EPDFText_GetCharGeometry,_rs_EPDFText_GetTextFull\
,_rs_FPDFText_FindStart,_rs_FPDFText_FindNext,_rs_FPDFText_GetSchResultIndex\
,_rs_FPDFText_GetSchCount,_rs_FPDFText_FindClose,_rs_FPDFText_CountRects,_rs_FPDFText_GetRect\
,_rs_FPDF_RenderPageBitmap,_rs_EPDF_GetPageSizeByIndexNormalized,_rs_peek_i32"

OPS_EXPORTS="_rs_read_page_geometry,_rs_read_page_text,_rs_render_page,_rs_search_page"
CPP_EXPORTS="_cc_read_page_geometry,_cc_read_page_text,_cc_render_page,_cc_search_page"

link() {
  local arm="$1"; shift
  local exports="$1"; shift
  mkdir -p "$OUT_DIR/$arm"
  em++ "$LIB_DIR/lib/libembedpdf.a" "$@" \
    -sENVIRONMENT=node \
    -sMODULARIZE=1 \
    -sEXPORT_ES6=1 \
    -sWASM=1 \
    -sALLOW_MEMORY_GROWTH=1 \
    -sALLOW_TABLE_GROWTH=1 \
    -sEXPORT_NAME=createArm \
    -sASSERTIONS=1 \
    -sEXPORTED_RUNTIME_METHODS=cwrap,getValue,setValue,UTF8ToString,UTF16ToString,stringToUTF8,stringToUTF16,wasmExports \
    -sEXPORTED_FUNCTIONS="$exports" \
    -I"$LIB_DIR/include" \
    -std=c++17 --no-entry \
    -o "$OUT_DIR/$arm/arm.mjs"
  echo "linked arm $arm -> $OUT_DIR/$arm/arm.wasm ($(wc -c < "$OUT_DIR/$arm/arm.wasm") bytes)"
}

build_rust() {
  cargo build --manifest-path "$ROOT/Cargo.toml" \
    -p "$1" --target wasm32-unknown-emscripten --release
}

for arm in $ARMS; do
  case "$arm" in
    a) link a "$PDFIUM_EXPORTS" ;;
    b) build_rust shim
       link b "$PDFIUM_EXPORTS,$SHIM_EXPORTS" \
         "$ROOT/target/wasm32-unknown-emscripten/release/libshim.a" ;;
    c) build_rust ops
       link c "$PDFIUM_EXPORTS,$OPS_EXPORTS" \
         "$ROOT/target/wasm32-unknown-emscripten/release/libops.a" ;;
    d) link d "$PDFIUM_EXPORTS,$CPP_EXPORTS" "$ROOT/cpp/ops.cc" ;;
    *) echo "unknown arm: $arm" >&2; exit 1 ;;
  esac
done
```

- [ ] **Step 3: Write the arm loader**

`pdfium-binding-benchmarks/bench/arm.mjs`:

```js
/**
 * The only file that knows how an arm is loaded.
 *
 * Every arm is cwrapped here, by hand and identically, rather than going
 * through `createWasmFunctions` in
 * packages/engine/runtime/src/wasm/wasm-runtime.ts. That bridge costs the
 * same for every arm, so including it would add noise without separating
 * anything. The consequence: these numbers exclude the engine's per-call
 * argument marshalling, and are therefore a LOWER bound on real per-call cost.
 */
import { resolve } from 'node:path';

const here = import.meta.dirname;

/** [name, returnType, argTypes] for every PDFium function the workloads call. */
const PDFIUM = [
  ['FPDF_InitLibrary', null, []],
  ['FPDF_DestroyLibrary', null, []],
  ['FPDF_LoadMemDocument', 'number', ['number', 'number', 'number']],
  ['FPDF_CloseDocument', null, ['number']],
  ['FPDF_GetPageCount', 'number', ['number']],
  ['FPDF_LoadPage', 'number', ['number', 'number']],
  ['FPDF_ClosePage', null, ['number']],
  ['FPDF_GetPageWidthF', 'number', ['number']],
  ['FPDF_GetPageHeightF', 'number', ['number']],
  ['EPDF_GetPageSizeByIndexNormalized', 'number', ['number', 'number', 'number']],
  ['FPDFText_LoadPage', 'number', ['number']],
  ['FPDFText_ClosePage', null, ['number']],
  ['FPDFText_CountChars', 'number', ['number']],
  ['FPDFText_GetTextObject', 'number', ['number', 'number']],
  ['FPDFText_GetFontSize', 'number', ['number', 'number']],
  ['EPDFText_GetCharGeometry', 'number', ['number', 'number', 'number']],
  ['EPDFText_GetTextFull', 'number', ['number', 'number', 'number']],
  ['FPDFText_FindStart', 'number', ['number', 'number', 'number', 'number']],
  ['FPDFText_FindNext', 'number', ['number']],
  ['FPDFText_GetSchResultIndex', 'number', ['number']],
  ['FPDFText_GetSchCount', 'number', ['number']],
  ['FPDFText_FindClose', null, ['number']],
  ['FPDFText_CountRects', 'number', ['number', 'number', 'number']],
  ['FPDFText_GetRect', 'number', ['number', 'number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_CreateEx', 'number', ['number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_FillRect', null, ['number', 'number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_Destroy', null, ['number']],
  ['FPDF_RenderPageBitmap', null, [
    'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number',
  ]],
];

/** Arm B mirrors PDFIUM one for one under an rs_ prefix. */
const SHIM = [
  ['rs_EPDF_GetPageSizeByIndexNormalized', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_CountChars', 'number', ['number']],
  ['rs_FPDFText_GetTextObject', 'number', ['number', 'number']],
  ['rs_FPDFText_GetFontSize', 'number', ['number', 'number']],
  ['rs_EPDFText_GetCharGeometry', 'number', ['number', 'number', 'number']],
  ['rs_EPDFText_GetTextFull', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_FindStart', 'number', ['number', 'number', 'number', 'number']],
  ['rs_FPDFText_FindNext', 'number', ['number']],
  ['rs_FPDFText_GetSchResultIndex', 'number', ['number']],
  ['rs_FPDFText_GetSchCount', 'number', ['number']],
  ['rs_FPDFText_FindClose', null, ['number']],
  ['rs_FPDFText_CountRects', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_GetRect', 'number', [
    'number', 'number', 'number', 'number', 'number', 'number',
  ]],
  ['rs_FPDF_RenderPageBitmap', null, [
    'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number',
  ]],
  ['rs_peek_i32', 'number', ['number']],
  ['rs_probe_page_size', 'number', ['number', 'number', 'number']],
];

const COARSE = (prefix) => [
  [`${prefix}_read_page_geometry`, 'number', ['number', 'number', 'number']],
  [`${prefix}_read_page_text`, 'number', ['number', 'number', 'number']],
  [`${prefix}_render_page`, 'number', ['number', 'number', 'number', 'number', 'number']],
  [`${prefix}_search_page`, 'number', ['number', 'number', 'number', 'number']],
];

const EXTRA = { a: [], b: SHIM, c: COARSE('rs'), d: COARSE('cc') };

export async function loadArm(name) {
  if (!EXTRA[name]) throw new Error(`unknown arm: ${name}`);
  const { default: createArm } = await import(resolve(here, `../build/out/${name}/arm.mjs`));
  const m = await createArm();

  const fn = {};
  for (const [n, ret, args] of [...PDFIUM, ...EXTRA[name]]) fn[n] = m.cwrap(n, ret, args);

  const mem = {
    alloc: (bytes) => m._malloc(bytes),
    free: (ptr) => m._free(ptr),
    peek32: (ptr) => m.getValue(ptr, 'i32'),
    peekF32: (ptr) => m.getValue(ptr, 'float'),
    peekF64: (ptr) => m.getValue(ptr, 'double'),
    readBytes: (ptr, len) => m.HEAPU8.slice(ptr, ptr + len),
    writeBytes: (ptr, data) => m.HEAPU8.set(data, ptr),
    writeU16: (str) => {
      const bytes = (str.length + 1) * 2;
      const ptr = m._malloc(bytes);
      m.stringToUTF16(str, ptr, bytes);
      return ptr;
    },
    readU16(ptr, units) {
      const bytes = m.HEAPU8.slice(ptr, ptr + units * 2);
      const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, units);
      let out = '';
      const CHUNK = 0x2000;
      for (let i = 0; i < u16.length; i += CHUNK) {
        out += String.fromCharCode(...u16.subarray(i, Math.min(i + CHUNK, u16.length)));
      }
      return out;
    },
  };

  fn.FPDF_InitLibrary();
  return { name, m, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
}

export async function wasmBytes(name) {
  const { statSync } = await import('node:fs');
  return statSync(resolve(here, `../build/out/${name}/arm.wasm`)).size;
}
```

- [ ] **Step 4: Write the fixture chooser**

`pdfium-binding-benchmarks/bench/fixture.mjs`:

```js
/**
 * One PDF, one page, chosen by glyph count rather than by guess, and recorded
 * in every results file. `examples/snippet-react/public/report.pdf` is a real document
 * from the repo's own examples, not a synthetic page.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
export const PDF_PATH = resolve(here, '../../examples/snippet-react/public/report.pdf');
export const SEARCH_TERM = 'the';

/** Picked once by `node bench/fixture.mjs`, then frozen here so every round agrees. */
export const PAGE_INDEX = Number(process.env.BENCH_PAGE ?? 0);

export function openFixture(arm) {
  const { fn, mem } = arm;
  const bytes = readFileSync(PDF_PATH);
  const buf = mem.alloc(bytes.length);
  mem.writeBytes(buf, bytes);
  const doc = fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  if (!doc) throw new Error(`FPDF_LoadMemDocument failed for ${PDF_PATH}`);
  const page = fn.FPDF_LoadPage(doc, PAGE_INDEX);
  if (!page) throw new Error(`FPDF_LoadPage failed for index ${PAGE_INDEX}`);
  const textPage = fn.FPDFText_LoadPage(page);
  if (!textPage) throw new Error(`FPDFText_LoadPage failed for index ${PAGE_INDEX}`);
  const glyphCount = Math.max(fn.FPDFText_CountChars(textPage), 0);

  return {
    doc,
    page,
    textPage,
    pageIndex: PAGE_INDEX,
    glyphCount,
    width: fn.FPDF_GetPageWidthF(page),
    height: fn.FPDF_GetPageHeightF(page),
    close() {
      fn.FPDFText_ClosePage(textPage);
      fn.FPDF_ClosePage(page);
      fn.FPDF_CloseDocument(doc);
      mem.free(buf);
    },
  };
}

/** `node bench/fixture.mjs` prints glyph counts so a page can be chosen. */
if (import.meta.filename === process.argv[1]) {
  const { loadArm } = await import('./arm.mjs');
  const arm = await loadArm('a');
  const bytes = readFileSync(PDF_PATH);
  const buf = arm.mem.alloc(bytes.length);
  arm.mem.writeBytes(buf, bytes);
  const doc = arm.fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  const pages = arm.fn.FPDF_GetPageCount(doc);
  const rows = [];
  for (let i = 0; i < Math.min(pages, 40); i++) {
    const p = arm.fn.FPDF_LoadPage(doc, i);
    const tp = arm.fn.FPDFText_LoadPage(p);
    rows.push({ page: i, glyphs: Math.max(arm.fn.FPDFText_CountChars(tp), 0) });
    arm.fn.FPDFText_ClosePage(tp);
    arm.fn.FPDF_ClosePage(p);
  }
  rows.sort((x, y) => y.glyphs - x.glyphs);
  console.log(`${pages} pages; densest first:`);
  for (const r of rows.slice(0, 10)) console.log(`  page ${r.page}: ${r.glyphs} glyphs`);
  arm.fn.FPDF_CloseDocument(doc);
}
```

- [ ] **Step 5: Write the failing test**

`pdfium-binding-benchmarks/bench/fixture.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadArm } from './arm.mjs';
import { openFixture } from './fixture.mjs';

test('arm a opens the fixture and finds a page with real text on it', async () => {
  const arm = await loadArm('a');
  const fx = openFixture(arm);
  assert.ok(fx.glyphCount > 500, `expected a text-heavy page, got ${fx.glyphCount} glyphs`);
  assert.ok(fx.width > 0 && fx.height > 0);
  fx.close();
  arm.close();
});
```

- [ ] **Step 6: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/fixture.test.mjs
```

Expected: FAIL, `Cannot find module '.../build/out/a/arm.mjs'`.

- [ ] **Step 7: Build arm a, pick the page, make it pass**

```bash
cd pdfium-binding-benchmarks
rm build/link-probe.sh
bash build/link-wasm.sh a
node bench/fixture.mjs
```

Read the densest page from the output. Set it as the default by editing `PAGE_INDEX` in `bench/fixture.mjs` to that literal index, replacing `Number(process.env.BENCH_PAGE ?? 0)` with `Number(process.env.BENCH_PAGE ?? <index>)`. Then:

```bash
node --test bench/fixture.test.mjs
```

Expected: PASS, one test.

- [ ] **Step 8: Commit**

```bash
git add pdfium-binding-benchmarks
git rm --cached pdfium-binding-benchmarks/build/link-probe.sh 2>/dev/null || true
git commit -m "spike: arm loader, four-arm link script, and fixture selection"
```

---

### Task 3: The five workloads and the fairness gate, arm A only

Nothing is timed yet. This task produces a canonical answer per workload so later arms can be checked against it.

**Files:**
- Create: `pdfium-binding-benchmarks/bench/workloads.mjs`
- Create: `pdfium-binding-benchmarks/bench/gate.mjs`
- Test: `pdfium-binding-benchmarks/bench/gate.test.mjs`

**Interfaces:**
- Consumes: `loadArm`, `wasmBytes` from `bench/arm.mjs`; `openFixture`, `SEARCH_TERM` from `bench/fixture.mjs`.
- Produces: from `bench/workloads.mjs`, `makeWorkloads(arm, fx)` returning an object keyed by workload id, each `{ id, arms, run(), canonical() }` where `run()` returns a number for the harness sink and `canonical()` returns a JSON-serializable result. Workload ids: `null-call`, `null-peek`, `null-readbytes`, `geometry`, `text`, `render-1x`, `render-4x`, `search`. From `bench/gate.mjs`, `canonicalFor(armName)` returning `Record<string, unknown>`.

- [ ] **Step 1: Write the workloads**

`pdfium-binding-benchmarks/bench/workloads.mjs`:

```js
/**
 * The only file that knows what is measured.
 *
 * `geometry` is a transcription of the loop in
 * packages/engine/services/src/features/geometry/PageGeometryReader.ts:109
 * and its `readGlyphRaw` helper at :135, call for call. It is a transcription
 * and not an import because the real reader needs DocumentSession and a built
 * engine-runtime dist, which would drag the whole workspace into a spike. So
 * this measures the SHAPE of the real reader, not the reader.
 *
 * Each workload declares which arms can run it. The null probes have nothing
 * for a coarse arm to collapse, so arms c and d skip them.
 */
import { SEARCH_TERM } from './fixture.mjs';

const ALL = ['a', 'b', 'c', 'd'];
const THIN = ['a', 'b'];

/**
 * EPDF_CHAR_GEOMETRY field offsets, from epdf_text.h:35:
 * loose_box 16 + tight_box 16 + loose_quad 32 + tight_quad 32 + matrix 24 + flags 4.
 * Step 5 of this task verifies them with offsetof rather than trusting the sum.
 */
const GEO = {
  bytes: 124,
  looseBox: 0,
  tightBox: 16,
  looseQuad: 32,
  tightQuad: 64,
  matrix: 96,
  flags: 120,
};

const F = {
  HAS_TIGHT_BOX: 1 << 0,
  HAS_LOOSE_QUAD: 1 << 1,
  HAS_TIGHT_QUAD: 1 << 2,
  UPRIGHT: 1 << 3,
  SPACE: 1 << 4,
  EMPTY: 1 << 5,
};

/** The packed record arms c and d write, one per glyph. See cpp/record.h. */
export const RECORD_BYTES = 112;

const rectAt = (mem, ptr, off) => [
  mem.peekF32(ptr + off),
  mem.peekF32(ptr + off + 4),
  mem.peekF32(ptr + off + 8),
  mem.peekF32(ptr + off + 12),
];

const quadAt = (mem, ptr, off) => {
  const out = new Array(8);
  for (let i = 0; i < 8; i++) out[i] = mem.peekF32(ptr + off + i * 4);
  return out;
};

const emptyGlyph = () => ({
  flags: 0,
  fontSize: 0,
  rotation: 0,
  ascentFlip: false,
  upright: true,
  looseBox: [0, 0, 0, 0],
  tightBox: null,
  looseQuad: null,
  tightQuad: null,
});

export function makeWorkloads(arm, fx) {
  const { fn, mem } = arm;
  const coarse = arm.name === 'c' ? 'rs' : arm.name === 'd' ? 'cc' : null;
  const p = arm.name === 'b' ? 'rs_' : '';

  // Scratch buffers, allocated once so allocation is not in any hot loop.
  const geoPtr = mem.alloc(GEO.bytes);
  const scratch8 = mem.alloc(8);
  const recordCap = Math.max(fx.glyphCount, 1);
  const recordPtr = mem.alloc(recordCap * RECORD_BYTES);
  const textUnits = fx.glyphCount * 2 + 1;
  const textPtr = mem.alloc(textUnits * 2);
  const rectPtrs = [mem.alloc(8), mem.alloc(8), mem.alloc(8), mem.alloc(8)];
  const termPtr = mem.writeU16(SEARCH_TERM);
  const searchCap = 4096;
  const searchPtr = mem.alloc(searchCap * 16);

  const W = Math.round(fx.width);
  const H = Math.round(fx.height);
  const bmp = (scale) => {
    const w = Math.max(1, Math.round(W * scale));
    const h = Math.max(1, Math.round(H * scale));
    const stride = w * 4;
    const buf = mem.alloc(stride * h);
    const handle = fn.FPDFBitmap_CreateEx(w, h, 4 /* BGRA */, buf, stride);
    return { w, h, stride, buf, handle };
  };
  const bmp1 = bmp(1);
  const bmp4 = bmp(4);

  /** Transcribed from PageGeometryReader.readGlyphRaw at :135. */
  const readGlyphRaw = () => {
    const native = mem.peek32(geoPtr + GEO.flags) >>> 0;
    if (native & F.EMPTY) return emptyGlyph();
    const upright = Boolean(native & F.UPRIGHT);
    const g = {
      flags: native & F.SPACE ? 1 : 0,
      fontSize: 0,
      rotation: 0,
      ascentFlip: false,
      upright,
      looseBox: rectAt(mem, geoPtr, GEO.looseBox),
      tightBox: null,
      looseQuad: null,
      tightQuad: null,
    };
    if (native & F.HAS_TIGHT_BOX) g.tightBox = rectAt(mem, geoPtr, GEO.tightBox);
    if (native & F.HAS_LOOSE_QUAD) {
      g.looseQuad = quadAt(mem, geoPtr, GEO.looseQuad);
      if (native & F.HAS_TIGHT_QUAD) g.tightQuad = quadAt(mem, geoPtr, GEO.tightQuad);
      if (!upright) {
        const a = mem.peekF32(geoPtr + GEO.matrix);
        const b = mem.peekF32(geoPtr + GEO.matrix + 4);
        const c = mem.peekF32(geoPtr + GEO.matrix + 8);
        const d = mem.peekF32(geoPtr + GEO.matrix + 12);
        g.rotation = Math.atan2(b, a);
        g.ascentFlip = a * d - b * c < 0;
      }
    }
    return g;
  };

  /** Transcribed from PageGeometryReader.read at :109. Arms a and b. */
  const geometryThin = () => {
    const out = new Array(fx.glyphCount);
    let prevKey = null;
    for (let i = 0; i < fx.glyphCount; i++) {
      const objectKey = fn[`${p}FPDFText_GetTextObject`](fx.textPage, i);
      const ok = fn[`${p}EPDFText_GetCharGeometry`](fx.textPage, i, geoPtr);
      const g = ok ? readGlyphRaw() : emptyGlyph();
      g.objectKey = objectKey;
      if (objectKey !== prevKey) {
        g.fontSize = fn[`${p}FPDFText_GetFontSize`](fx.textPage, i);
        prevKey = objectKey;
      }
      out[i] = g;
    }
    return out;
  };

  /** Arms c and d. One crossing, then one typed-array walk. */
  const geometryCoarse = () => {
    const n = fn[`${coarse}_read_page_geometry`](fx.textPage, recordPtr, recordCap);
    const bytes = mem.readBytes(recordPtr, n * RECORD_BYTES);
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const out = new Array(n);
    for (let i = 0; i < n; i++) {
      const o = i * RECORD_BYTES;
      const native = dv.getUint32(o, true);
      if (native & F.EMPTY) {
        const g = emptyGlyph();
        g.objectKey = dv.getUint32(o + 4, true);
        out[i] = g;
        continue;
      }
      const upright = Boolean(native & F.UPRIGHT);
      const g = {
        flags: native & F.SPACE ? 1 : 0,
        objectKey: dv.getUint32(o + 4, true),
        fontSize: dv.getFloat32(o + 8, true),
        rotation: upright ? 0 : dv.getFloat32(o + 12, true),
        ascentFlip: !upright && (native & (1 << 6)) !== 0,
        upright,
        looseBox: [16, 20, 24, 28].map((k) => dv.getFloat32(o + k, true)),
        tightBox: null,
        looseQuad: null,
        tightQuad: null,
      };
      if (native & F.HAS_TIGHT_BOX) {
        g.tightBox = [32, 36, 40, 44].map((k) => dv.getFloat32(o + k, true));
      }
      if (native & F.HAS_LOOSE_QUAD) {
        g.looseQuad = Array.from({ length: 8 }, (_, k) => dv.getFloat32(o + 48 + k * 4, true));
        if (native & F.HAS_TIGHT_QUAD) {
          g.tightQuad = Array.from({ length: 8 }, (_, k) => dv.getFloat32(o + 80 + k * 4, true));
        }
      }
      out[i] = g;
    }
    return out;
  };

  const textThin = () => {
    const written = fn[`${p}EPDFText_GetTextFull`](fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  const textCoarse = () => {
    const written = fn[`${coarse}_read_page_text`](fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  const renderThin = (b) => {
    fn.FPDFBitmap_FillRect(b.handle, 0, 0, b.w, b.h, 0xffffffff);
    fn[`${p}FPDF_RenderPageBitmap`](b.handle, fx.page, 0, 0, b.w, b.h, 0, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const renderCoarse = (b) => {
    fn[`${coarse}_render_page`](fx.page, b.handle, b.w, b.h, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const searchThin = () => {
    const handle = fn[`${p}FPDFText_FindStart`](fx.textPage, termPtr, 0, 0);
    const hits = [];
    while (fn[`${p}FPDFText_FindNext`](handle)) {
      const start = fn[`${p}FPDFText_GetSchResultIndex`](handle);
      const count = fn[`${p}FPDFText_GetSchCount`](handle);
      const rects = fn[`${p}FPDFText_CountRects`](fx.textPage, start, count);
      for (let r = 0; r < rects; r++) {
        fn[`${p}FPDFText_GetRect`](fx.textPage, r, ...rectPtrs);
        hits.push(rectPtrs.map((q) => mem.peekF64(q)));
      }
    }
    fn[`${p}FPDFText_FindClose`](handle);
    return hits;
  };

  const searchCoarse = () => {
    const n = fn[`${coarse}_search_page`](fx.textPage, termPtr, searchPtr, searchCap);
    const bytes = mem.readBytes(searchPtr, n * 16);
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const hits = new Array(n);
    for (let i = 0; i < n; i++) {
      hits[i] = [0, 4, 8, 12].map((k) => dv.getFloat32(i * 16 + k, true));
    }
    return hits;
  };

  const geometry = () => (coarse ? geometryCoarse() : geometryThin());
  const text = () => (coarse ? textCoarse() : textThin());
  const render = (b) => (coarse ? renderCoarse(b) : renderThin(b));
  const search = () => (coarse ? searchCoarse() : searchThin());

  const hash = (bytes) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < bytes.length; i++) {
      h ^= bytes[i];
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16).padStart(8, '0');
  };

  const round = (v) => (typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
  const roundDeep = (v) =>
    Array.isArray(v) ? v.map(roundDeep) : typeof v === 'number' ? round(v) : v;

  return {
    'null-call': {
      id: 'null-call',
      arms: THIN,
      run: () => fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8),
      canonical: () => {
        fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8);
        return [round(mem.peekF32(scratch8)), round(mem.peekF32(scratch8 + 4))];
      },
    },
    'null-peek': {
      id: 'null-peek',
      arms: THIN,
      run: () => mem.peek32(geoPtr + GEO.flags),
      canonical: () => 'n/a',
    },
    'null-readbytes': {
      id: 'null-readbytes',
      arms: THIN,
      run: () => mem.readBytes(geoPtr, 64)[0],
      canonical: () => 'n/a',
    },
    geometry: {
      id: 'geometry',
      arms: ALL,
      run: () => geometry().length,
      canonical: () => roundDeep(geometry().map((g) => [
        g.flags, g.fontSize, g.rotation, g.ascentFlip ? 1 : 0, g.upright ? 1 : 0,
        g.looseBox, g.tightBox, g.looseQuad, g.tightQuad,
      ])),
    },
    text: {
      id: 'text',
      arms: ALL,
      run: () => text().length,
      canonical: () => text(),
    },
    'render-1x': {
      id: 'render-1x',
      arms: ALL,
      run: () => render(bmp1)[0],
      canonical: () => hash(render(bmp1)),
    },
    'render-4x': {
      id: 'render-4x',
      arms: ALL,
      run: () => render(bmp4)[0],
      canonical: () => hash(render(bmp4)),
    },
    search: {
      id: 'search',
      arms: ALL,
      run: () => search().length,
      canonical: () => roundDeep(search()),
    },
    dispose() {
      fn.FPDFBitmap_Destroy(bmp1.handle);
      fn.FPDFBitmap_Destroy(bmp4.handle);
      for (const q of [
        geoPtr, scratch8, recordPtr, textPtr, termPtr, searchPtr,
        bmp1.buf, bmp4.buf, ...rectPtrs,
      ]) mem.free(q);
    },
  };
}
```

- [ ] **Step 2: Write the gate**

`pdfium-binding-benchmarks/bench/gate.mjs`:

```js
#!/usr/bin/env node
/**
 * The only file that knows what "the same answer" means.
 *
 * This runs before any timing. A wrong arm can be fast, and nothing about a
 * timing table would reveal it. The previous study on this branch caught real
 * divergence exactly here, which is the reason to keep paying for it.
 */
import { loadArm } from './arm.mjs';
import { openFixture } from './fixture.mjs';
import { makeWorkloads } from './workloads.mjs';

export async function canonicalFor(armName) {
  const arm = await loadArm(armName);
  const fx = openFixture(arm);
  const w = makeWorkloads(arm, fx);
  const out = { glyphCount: fx.glyphCount, pageIndex: fx.pageIndex };
  for (const [id, wl] of Object.entries(w)) {
    if (id === 'dispose' || !wl.arms.includes(armName)) continue;
    out[id] = wl.canonical();
  }
  w.dispose();
  fx.close();
  arm.close();
  return out;
}

export function compare(baselineName, baseline, armName, actual) {
  const failures = [];
  for (const [id, want] of Object.entries(baseline)) {
    if (!(id in actual)) continue;
    const a = JSON.stringify(want);
    const b = JSON.stringify(actual[id]);
    if (a === b) continue;
    failures.push({
      id,
      baseline: a.length > 400 ? `${a.slice(0, 400)}… (${a.length} chars)` : a,
      actual: b.length > 400 ? `${b.slice(0, 400)}… (${b.length} chars)` : b,
    });
  }
  if (failures.length === 0) return true;
  console.error(`\nFAIRNESS GATE FAILED: arm ${armName} disagrees with arm ${baselineName}.`);
  for (const f of failures) {
    console.error(`  ${f.id}\n    ${baselineName}: ${f.baseline}\n    ${armName}: ${f.actual}`);
  }
  return false;
}

if (import.meta.filename === process.argv[1]) {
  const arms = process.argv.slice(2).length ? process.argv.slice(2) : ['a'];
  const results = {};
  for (const a of arms) results[a] = await canonicalFor(a);
  const [first, ...rest] = arms;
  let ok = true;
  for (const a of rest) ok = compare(first, results[first], a, results[a]) && ok;
  if (!ok) process.exit(1);
  console.log(
    `fairness gate: ${arms.join(', ')} agree on ${results[first].glyphCount} glyphs, ` +
      `page ${results[first].pageIndex}`,
  );
}
```

- [ ] **Step 3: Write the failing test**

`pdfium-binding-benchmarks/bench/gate.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor } from './gate.mjs';

test('arm a produces a stable canonical answer for every workload it runs', async () => {
  const first = await canonicalFor('a');
  const second = await canonicalFor('a');

  assert.ok(first.glyphCount > 500);
  assert.equal(JSON.stringify(first.geometry), JSON.stringify(second.geometry));
  assert.equal(first.text, second.text);
  assert.equal(first['render-1x'], second['render-1x']);
  assert.equal(first['render-4x'], second['render-4x']);
  assert.equal(JSON.stringify(first.search), JSON.stringify(second.search));

  assert.equal(first.geometry.length, first.glyphCount);
  assert.ok(first.text.length > 100, 'expected real text on the fixture page');
  assert.ok(first.search.length > 0, 'expected the search term to hit');
  assert.notEqual(first['render-1x'], '00000000');
  assert.notEqual(first['render-1x'], first['render-4x']);
});
```

- [ ] **Step 4: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/gate.test.mjs
```

Expected: FAIL. The likely first failure is a `GEO` offset mismatch producing wrong `looseBox` values or a zero glyph count.

- [ ] **Step 5: Verify the EPDF_CHAR_GEOMETRY offsets against the header**

The struct at `packages/engine/runtime/runtime-src/public/epdf_text.h:35` is `FS_RECTF loose_box; FS_RECTF tight_box; FS_QUADPOINTSF loose_quad; FS_QUADPOINTSF tight_quad; FS_MATRIX matrix; uint32_t flags;`. With `FS_RECTF` at 16 bytes, `FS_QUADPOINTSF` at 32, and `FS_MATRIX` at 24, the offsets are 0, 16, 32, 64, 96, 120 and the total is 124.

Confirm rather than trust. Compile a one-line check:

```bash
cd pdfium-binding-benchmarks
source build/emsdk.sh
cat > /tmp/geo.cc <<'EOF'
#include <cstdio>
#include <cstddef>
#include "epdf_text.h"
int main() {
  printf("sizeof=%zu loose=%zu tight=%zu lq=%zu tq=%zu m=%zu flags=%zu\n",
    sizeof(EPDF_CHAR_GEOMETRY),
    offsetof(EPDF_CHAR_GEOMETRY, loose_box), offsetof(EPDF_CHAR_GEOMETRY, tight_box),
    offsetof(EPDF_CHAR_GEOMETRY, loose_quad), offsetof(EPDF_CHAR_GEOMETRY, tight_quad),
    offsetof(EPDF_CHAR_GEOMETRY, matrix), offsetof(EPDF_CHAR_GEOMETRY, flags));
  return 0;
}
EOF
em++ /tmp/geo.cc -I build/libpdfium/wasm32/include -std=c++17 -o /tmp/geo.mjs -sENVIRONMENT=node -sEXPORT_ES6=1
node /tmp/geo.mjs
```

Expected: `sizeof=124 loose=0 tight=16 lq=32 tq=64 m=96 flags=120`. If it prints anything else, correct the `GEO` object in `bench/workloads.mjs` to match.

- [ ] **Step 6: Run the test to verify it passes**

```bash
cd pdfium-binding-benchmarks && node --test bench/gate.test.mjs && node bench/gate.mjs a
```

Expected: PASS, one test, then a line reading `fairness gate: a agree on <N> glyphs, page <i>`.

- [ ] **Step 7: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "spike: five workloads and the fairness gate against arm a"
```

---

### Task 4: Arm B, the thin Rust shim

**Files:**
- Modify: `pdfium-binding-benchmarks/crates/shim/src/lib.rs` (append; full contents below)
- Test: `pdfium-binding-benchmarks/bench/arm-b.test.mjs`

**Interfaces:**
- Consumes: every `extern "C"` in `crates/pdfium-sys/src/lib.rs`.
- Produces: sixteen `rs_*` exports matching the `SHIM` table in `bench/arm.mjs`. Each takes the same arguments as its PDFium counterpart with pointers as `u32`-compatible raw pointers, and returns the same value. `rs_FPDFText_GetFontSize` returns `f64`. `rs_peek_i32(ptr) -> i32` exists so the null-peek probe has a Rust counterpart.

- [ ] **Step 1: Write the failing test**

`pdfium-binding-benchmarks/bench/arm-b.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm b agrees with arm a on every workload they share', async () => {
  const a = await canonicalFor('a');
  const b = await canonicalFor('b');
  assert.equal(compare('a', a, 'b', b), true);
  assert.equal(a.glyphCount, b.glyphCount);
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/arm-b.test.mjs
```

Expected: FAIL, `Cannot find module '.../build/out/b/arm.mjs'`.

- [ ] **Step 3: Write the shim**

Replace `pdfium-binding-benchmarks/crates/shim/src/lib.rs` entirely:

```rust
//! Arm B. Every export is `rs_`-prefixed, takes and returns only i32/f32/f64,
//! and does exactly what the corresponding PDFium function does. The point is
//! that the call count is identical to arm A and the only difference is one
//! extra wasm frame.
#![allow(non_snake_case)]

use pdfium_sys::*;

/// Writes width into out[0] and height into out[1]. Returns 1 on success.
#[no_mangle]
pub extern "C" fn rs_probe_page_size(doc: u32, index: i32, out: *mut f32) -> i32 {
    let mut size = FS_SIZEF::default();
    let ok = unsafe { EPDF_GetPageSizeByIndexNormalized(doc, index, &mut size) };
    if ok == 0 {
        return 0;
    }
    unsafe {
        *out = size.width;
        *out.add(1) = size.height;
    }
    1
}

#[no_mangle]
pub extern "C" fn rs_EPDF_GetPageSizeByIndexNormalized(
    doc: u32,
    index: i32,
    size: *mut FS_SIZEF,
) -> i32 {
    unsafe { EPDF_GetPageSizeByIndexNormalized(doc, index, size) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_CountChars(text_page: u32) -> i32 {
    unsafe { FPDFText_CountChars(text_page) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetTextObject(text_page: u32, index: i32) -> u32 {
    unsafe { FPDFText_GetTextObject(text_page, index) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetFontSize(text_page: u32, index: i32) -> f64 {
    unsafe { FPDFText_GetFontSize(text_page, index) }
}

#[no_mangle]
pub extern "C" fn rs_EPDFText_GetCharGeometry(
    text_page: u32,
    index: i32,
    geometry: *mut EPDF_CHAR_GEOMETRY,
) -> i32 {
    unsafe { EPDFText_GetCharGeometry(text_page, index, geometry) }
}

#[no_mangle]
pub extern "C" fn rs_EPDFText_GetTextFull(text_page: u32, buffer: *mut u16, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(text_page, buffer, len) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindStart(
    text_page: u32,
    findwhat: *const u16,
    flags: u32,
    start_index: i32,
) -> u32 {
    unsafe { FPDFText_FindStart(text_page, findwhat, flags, start_index) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindNext(handle: u32) -> i32 {
    unsafe { FPDFText_FindNext(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetSchResultIndex(handle: u32) -> i32 {
    unsafe { FPDFText_GetSchResultIndex(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetSchCount(handle: u32) -> i32 {
    unsafe { FPDFText_GetSchCount(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindClose(handle: u32) {
    unsafe { FPDFText_FindClose(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_CountRects(text_page: u32, start: i32, count: i32) -> i32 {
    unsafe { FPDFText_CountRects(text_page, start, count) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetRect(
    text_page: u32,
    rect_index: i32,
    left: *mut f64,
    top: *mut f64,
    right: *mut f64,
    bottom: *mut f64,
) -> i32 {
    unsafe { FPDFText_GetRect(text_page, rect_index, left, top, right, bottom) }
}

#[no_mangle]
pub extern "C" fn rs_FPDF_RenderPageBitmap(
    bitmap: u32,
    page: u32,
    start_x: i32,
    start_y: i32,
    size_x: i32,
    size_y: i32,
    rotate: i32,
    flags: i32,
) {
    unsafe { FPDF_RenderPageBitmap(bitmap, page, start_x, start_y, size_x, size_y, rotate, flags) }
}

/// The Rust counterpart to the null-peek probe: one wasm crossing that reads
/// four bytes of linear memory and returns them.
#[no_mangle]
pub extern "C" fn rs_peek_i32(ptr: *const i32) -> i32 {
    unsafe { *ptr }
}
```

- [ ] **Step 4: Build arm b and run the test**

```bash
cd pdfium-binding-benchmarks
bash build/link-wasm.sh b
node --test bench/arm-b.test.mjs
node bench/gate.mjs a b
```

Expected: PASS, one test, then `fairness gate: a, b agree on <N> glyphs, page <i>`.

- [ ] **Step 5: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "spike: arm b, a 1:1 Rust shim over the PDFium calls the workloads make"
```

---

### Task 5: Arm C, the coarse Rust layer

**Files:**
- Create: `pdfium-binding-benchmarks/crates/ops/Cargo.toml`
- Create: `pdfium-binding-benchmarks/crates/ops/src/record.rs`
- Create: `pdfium-binding-benchmarks/crates/ops/src/lib.rs`
- Modify: `pdfium-binding-benchmarks/Cargo.toml:3` (add `crates/ops` to `members`)
- Test: `pdfium-binding-benchmarks/bench/arm-c.test.mjs`

**Interfaces:**
- Consumes: every `extern "C"` in `crates/pdfium-sys/src/lib.rs`.
- Produces: `rs_read_page_geometry(text_page: u32, out: *mut u8, cap: i32) -> i32` returning the glyph count and writing `min(count, cap)` records of exactly 112 bytes. `rs_read_page_text(text_page: u32, buf: *mut u16, len: i32) -> i32`. `rs_render_page(page: u32, bitmap: u32, w: i32, h: i32, rotate: i32) -> i32`. `rs_search_page(text_page: u32, term: *const u16, out: *mut f32, cap: i32) -> i32` returning the hit-rect count and writing four little-endian `f32`s per hit.

The 112-byte record layout, shared with arm D:

| offset | type | field |
|---|---|---|
| 0 | u32 | flags (EPDF_CHARGEO bits, plus bit 6 = ascent flip) |
| 4 | u32 | objectKey |
| 8 | f32 | fontSize |
| 12 | f32 | rotation |
| 16 | 4 × f32 | looseBox |
| 32 | 4 × f32 | tightBox |
| 48 | 8 × f32 | looseQuad |
| 80 | 8 × f32 | tightQuad |

- [ ] **Step 1: Write the failing test**

`pdfium-binding-benchmarks/bench/arm-c.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm c agrees with arm a on every workload they share', async () => {
  const a = await canonicalFor('a');
  const c = await canonicalFor('c');
  assert.equal(compare('a', a, 'c', c), true);
  assert.equal(a.glyphCount, c.glyphCount);
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/arm-c.test.mjs
```

Expected: FAIL, `Cannot find module '.../build/out/c/arm.mjs'`.

- [ ] **Step 3: Add the ops crate to the workspace**

`pdfium-binding-benchmarks/Cargo.toml`, replace the `members` line:

```toml
members = ["crates/pdfium-sys", "crates/shim", "crates/ops"]
```

`pdfium-binding-benchmarks/crates/ops/Cargo.toml`:

```toml
[package]
name = "ops"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["staticlib", "rlib"]

[dependencies]
pdfium-sys = { path = "../pdfium-sys" }
```

- [ ] **Step 4: Write the record writer**

`pdfium-binding-benchmarks/crates/ops/src/record.rs`:

```rust
//! The 112-byte glyph record arms C and D both write. The layout is duplicated
//! in cpp/record.h and decoded in bench/workloads.mjs; all three must agree,
//! and the fairness gate is what proves they do.

pub const RECORD_BYTES: usize = 112;
/// Set alongside the EPDF_CHARGEO bits when the char matrix has det < 0.
pub const FLAG_ASCENT_FLIP: u32 = 1 << 6;

pub struct Writer {
    base: *mut u8,
    cap: usize,
}

impl Writer {
    /// # Safety
    /// `base` must point to at least `cap * RECORD_BYTES` writable bytes.
    pub unsafe fn new(base: *mut u8, cap: usize) -> Self {
        Self { base, cap }
    }

    fn slot(&self, index: usize) -> Option<*mut u8> {
        if index >= self.cap {
            return None;
        }
        Some(unsafe { self.base.add(index * RECORD_BYTES) })
    }

    pub fn write(&mut self, index: usize, rec: &Record) {
        let Some(p) = self.slot(index) else { return };
        unsafe {
            core::ptr::write_unaligned(p as *mut u32, rec.flags);
            core::ptr::write_unaligned(p.add(4) as *mut u32, rec.object_key);
            core::ptr::write_unaligned(p.add(8) as *mut f32, rec.font_size);
            core::ptr::write_unaligned(p.add(12) as *mut f32, rec.rotation);
            for (i, v) in rec.loose_box.iter().enumerate() {
                core::ptr::write_unaligned(p.add(16 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.tight_box.iter().enumerate() {
                core::ptr::write_unaligned(p.add(32 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.loose_quad.iter().enumerate() {
                core::ptr::write_unaligned(p.add(48 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.tight_quad.iter().enumerate() {
                core::ptr::write_unaligned(p.add(80 + i * 4) as *mut f32, *v);
            }
        }
    }
}

#[derive(Default)]
pub struct Record {
    pub flags: u32,
    pub object_key: u32,
    pub font_size: f32,
    pub rotation: f32,
    pub loose_box: [f32; 4],
    pub tight_box: [f32; 4],
    pub loose_quad: [f32; 8],
    pub tight_quad: [f32; 8],
}
```

- [ ] **Step 5: Write the coarse operations**

`pdfium-binding-benchmarks/crates/ops/src/lib.rs`:

```rust
//! Arm C. Four operations that each run a whole loop inside Rust and hand back
//! one packed buffer, so a page of geometry costs one boundary crossing
//! instead of tens of thousands.
#![allow(non_snake_case)]

mod record;

use pdfium_sys::*;
use record::{Record, Writer, FLAG_ASCENT_FLIP, RECORD_BYTES};

const _: () = assert!(RECORD_BYTES == 112);

/// Transcribes the loop in
/// packages/engine/services/src/features/geometry/PageGeometryReader.ts:109.
/// Returns the total glyph count; writes at most `cap` records.
#[no_mangle]
pub extern "C" fn rs_read_page_geometry(text_page: u32, out: *mut u8, cap: i32) -> i32 {
    let count = unsafe { FPDFText_CountChars(text_page) }.max(0);
    if out.is_null() || cap <= 0 {
        return count;
    }
    let mut w = unsafe { Writer::new(out, cap as usize) };
    let mut prev_key: Option<u32> = None;

    for i in 0..count {
        let mut geo = EPDF_CHAR_GEOMETRY::default();
        let object_key = unsafe { FPDFText_GetTextObject(text_page, i) };
        let ok = unsafe { EPDFText_GetCharGeometry(text_page, i, &mut geo) };

        let mut rec = Record {
            object_key,
            ..Default::default()
        };

        if ok == 0 || (geo.flags & EPDF_CHARGEO_EMPTY) != 0 {
            rec.flags = EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_UPRIGHT;
            w.write(i as usize, &rec);
            prev_key = Some(object_key);
            continue;
        }

        rec.flags = geo.flags;
        rec.loose_box = [
            geo.loose_box.left,
            geo.loose_box.top,
            geo.loose_box.right,
            geo.loose_box.bottom,
        ];
        if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_BOX) != 0 {
            rec.tight_box = [
                geo.tight_box.left,
                geo.tight_box.top,
                geo.tight_box.right,
                geo.tight_box.bottom,
            ];
        }
        if (geo.flags & EPDF_CHARGEO_HAS_LOOSE_QUAD) != 0 {
            rec.loose_quad = quad(&geo.loose_quad);
            if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_QUAD) != 0 {
                rec.tight_quad = quad(&geo.tight_quad);
            }
            if (geo.flags & EPDF_CHARGEO_UPRIGHT) == 0 {
                let m = geo.matrix;
                rec.rotation = m.b.atan2(m.a);
                if m.a * m.d - m.b * m.c < 0.0 {
                    rec.flags |= FLAG_ASCENT_FLIP;
                }
            }
        }

        if prev_key != Some(object_key) {
            rec.font_size = unsafe { FPDFText_GetFontSize(text_page, i) } as f32;
            prev_key = Some(object_key);
        }

        w.write(i as usize, &rec);
    }
    count
}

fn quad(q: &FS_QUADPOINTSF) -> [f32; 8] {
    [q.x1, q.y1, q.x2, q.y2, q.x3, q.y3, q.x4, q.y4]
}

#[no_mangle]
pub extern "C" fn rs_read_page_text(text_page: u32, buf: *mut u16, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(text_page, buf, len) }
}

#[no_mangle]
pub extern "C" fn rs_render_page(page: u32, bitmap: u32, w: i32, h: i32, rotate: i32) -> i32 {
    unsafe {
        FPDFBitmap_FillRect(bitmap, 0, 0, w, h, 0xffff_ffff);
        FPDF_RenderPageBitmap(bitmap, page, 0, 0, w, h, rotate, 0);
    }
    1
}

/// Runs the whole FindNext loop in Rust. Returns the hit-rect count; writes
/// four little-endian f32s per rect (left, top, right, bottom).
#[no_mangle]
pub extern "C" fn rs_search_page(text_page: u32, term: *const u16, out: *mut f32, cap: i32) -> i32 {
    let handle = unsafe { FPDFText_FindStart(text_page, term, 0, 0) };
    if handle == 0 {
        return 0;
    }
    let mut written = 0i32;
    unsafe {
        while FPDFText_FindNext(handle) != 0 {
            let start = FPDFText_GetSchResultIndex(handle);
            let count = FPDFText_GetSchCount(handle);
            let rects = FPDFText_CountRects(text_page, start, count);
            for r in 0..rects {
                let (mut l, mut t, mut ri, mut b) = (0f64, 0f64, 0f64, 0f64);
                if FPDFText_GetRect(text_page, r, &mut l, &mut t, &mut ri, &mut b) == 0 {
                    continue;
                }
                if written < cap {
                    let p = out.add((written * 4) as usize);
                    core::ptr::write_unaligned(p, l as f32);
                    core::ptr::write_unaligned(p.add(1), t as f32);
                    core::ptr::write_unaligned(p.add(2), ri as f32);
                    core::ptr::write_unaligned(p.add(3), b as f32);
                }
                written += 1;
            }
        }
        FPDFText_FindClose(handle);
    }
    written
}
```

- [ ] **Step 6: Build arm c and run the test**

```bash
cd pdfium-binding-benchmarks
bash build/link-wasm.sh c
node --test bench/arm-c.test.mjs
```

Expected: PASS.

The likeliest failure is the `search` workload disagreeing, because `FPDFText_GetRect` returns `double` and the record stores `f32`. `bench/workloads.mjs` rounds to 1e-6 on both sides, which absorbs the narrowing for page coordinates in the hundreds. If it still disagrees, widen the record's rect slots to `f64` in `crates/ops/src/lib.rs`, `cpp/ops.cc`, and the `searchCoarse` decoder in `bench/workloads.mjs`, changing the stride from 16 to 32 bytes in all three.

- [ ] **Step 7: Run the full gate and commit**

```bash
node bench/gate.mjs a b c
git add pdfium-binding-benchmarks
git commit -m "spike: arm c, four coarse Rust operations over one boundary crossing"
```

Expected gate output: `fairness gate: a, b, c agree on <N> glyphs, page <i>`.

---

### Task 6: Arm D, the coarse C++ control

This is the arm that separates "Rust helped" from "moving the loop helped". `packages/engine/services/src/features/text/PageTextReader.ts:93` already reads a whole page of text through one `EPDFText_GetTextFull` call, so the fork has been collapsing these loops in C++ for a while and can keep doing so without any Rust.

**Files:**
- Create: `pdfium-binding-benchmarks/cpp/record.h`
- Create: `pdfium-binding-benchmarks/cpp/ops.cc`
- Test: `pdfium-binding-benchmarks/bench/arm-d.test.mjs`

**Interfaces:**
- Consumes: the PDFium headers in `build/libpdfium/wasm32/include`.
- Produces: `cc_read_page_geometry`, `cc_read_page_text`, `cc_render_page`, `cc_search_page` with signatures identical to their `rs_` counterparts in Task 5, writing the identical 112-byte record.

- [ ] **Step 1: Write the failing test**

`pdfium-binding-benchmarks/bench/arm-d.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm d agrees with arm a, and matches arm c byte for byte', async () => {
  const a = await canonicalFor('a');
  const c = await canonicalFor('c');
  const d = await canonicalFor('d');
  assert.equal(compare('a', a, 'd', d), true);
  assert.equal(
    JSON.stringify(c.geometry),
    JSON.stringify(d.geometry),
    'arms c and d write the same record layout and must agree exactly',
  );
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/arm-d.test.mjs
```

Expected: FAIL, `Cannot find module '.../build/out/d/arm.mjs'`.

- [ ] **Step 3: Write the record header**

`pdfium-binding-benchmarks/cpp/record.h`:

```cpp
// The 112-byte glyph record, duplicated from crates/ops/src/record.rs and
// decoded in bench/workloads.mjs. All three must agree; the fairness gate is
// what proves they do.
#pragma once
#include <cstdint>
#include <cstring>

constexpr size_t kRecordBytes = 112;
// Set alongside the EPDF_CHARGEO bits when the char matrix has det < 0.
constexpr uint32_t kFlagAscentFlip = 1u << 6;

struct Record {
  uint32_t flags = 0;
  uint32_t object_key = 0;
  float font_size = 0;
  float rotation = 0;
  float loose_box[4] = {0, 0, 0, 0};
  float tight_box[4] = {0, 0, 0, 0};
  float loose_quad[8] = {0, 0, 0, 0, 0, 0, 0, 0};
  float tight_quad[8] = {0, 0, 0, 0, 0, 0, 0, 0};
};

inline void WriteRecord(uint8_t* base, size_t index, size_t cap, const Record& r) {
  if (index >= cap) return;
  uint8_t* p = base + index * kRecordBytes;
  std::memcpy(p + 0, &r.flags, 4);
  std::memcpy(p + 4, &r.object_key, 4);
  std::memcpy(p + 8, &r.font_size, 4);
  std::memcpy(p + 12, &r.rotation, 4);
  std::memcpy(p + 16, r.loose_box, 16);
  std::memcpy(p + 32, r.tight_box, 16);
  std::memcpy(p + 48, r.loose_quad, 32);
  std::memcpy(p + 80, r.tight_quad, 32);
}
```

- [ ] **Step 4: Write the C++ operations**

`pdfium-binding-benchmarks/cpp/ops.cc`:

```cpp
// Arm D. The same four coarse operations as crates/ops/src/lib.rs, in C++,
// linked into the same em++ command. If this lands within noise of arm C, the
// win belongs to moving the loop and not to Rust.
#include <cmath>
#include <cstdint>

#include "epdf_text.h"
#include "fpdf_text.h"
#include "fpdfview.h"

#include "record.h"

extern "C" {

int cc_read_page_geometry(FPDF_TEXTPAGE text_page, uint8_t* out, int cap) {
  int count = FPDFText_CountChars(text_page);
  if (count < 0) count = 0;
  if (out == nullptr || cap <= 0) return count;

  bool have_prev = false;
  FPDF_PAGEOBJECT prev_key = nullptr;

  for (int i = 0; i < count; i++) {
    EPDF_CHAR_GEOMETRY geo = {};
    FPDF_PAGEOBJECT object_key = FPDFText_GetTextObject(text_page, i);
    FPDF_BOOL ok = EPDFText_GetCharGeometry(text_page, i, &geo);

    Record rec;
    rec.object_key = static_cast<uint32_t>(reinterpret_cast<uintptr_t>(object_key));

    if (!ok || (geo.flags & EPDF_CHARGEO_EMPTY)) {
      rec.flags = EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_UPRIGHT;
      WriteRecord(out, static_cast<size_t>(i), static_cast<size_t>(cap), rec);
      prev_key = object_key;
      have_prev = true;
      continue;
    }

    rec.flags = geo.flags;
    rec.loose_box[0] = geo.loose_box.left;
    rec.loose_box[1] = geo.loose_box.top;
    rec.loose_box[2] = geo.loose_box.right;
    rec.loose_box[3] = geo.loose_box.bottom;

    if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_BOX) {
      rec.tight_box[0] = geo.tight_box.left;
      rec.tight_box[1] = geo.tight_box.top;
      rec.tight_box[2] = geo.tight_box.right;
      rec.tight_box[3] = geo.tight_box.bottom;
    }

    if (geo.flags & EPDF_CHARGEO_HAS_LOOSE_QUAD) {
      const FS_QUADPOINTSF& l = geo.loose_quad;
      float lq[8] = {l.x1, l.y1, l.x2, l.y2, l.x3, l.y3, l.x4, l.y4};
      for (int k = 0; k < 8; k++) rec.loose_quad[k] = lq[k];
      if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_QUAD) {
        const FS_QUADPOINTSF& t = geo.tight_quad;
        float tq[8] = {t.x1, t.y1, t.x2, t.y2, t.x3, t.y3, t.x4, t.y4};
        for (int k = 0; k < 8; k++) rec.tight_quad[k] = tq[k];
      }
      if (!(geo.flags & EPDF_CHARGEO_UPRIGHT)) {
        const FS_MATRIX& m = geo.matrix;
        rec.rotation = std::atan2(m.b, m.a);
        if (m.a * m.d - m.b * m.c < 0) rec.flags |= kFlagAscentFlip;
      }
    }

    if (!have_prev || prev_key != object_key) {
      rec.font_size = static_cast<float>(FPDFText_GetFontSize(text_page, i));
      prev_key = object_key;
      have_prev = true;
    }

    WriteRecord(out, static_cast<size_t>(i), static_cast<size_t>(cap), rec);
  }
  return count;
}

int cc_read_page_text(FPDF_TEXTPAGE text_page, unsigned short* buf, int len) {
  return EPDFText_GetTextFull(text_page, buf, len);
}

int cc_render_page(FPDF_PAGE page, FPDF_BITMAP bitmap, int w, int h, int rotate) {
  FPDFBitmap_FillRect(bitmap, 0, 0, w, h, 0xffffffff);
  FPDF_RenderPageBitmap(bitmap, page, 0, 0, w, h, rotate, 0);
  return 1;
}

int cc_search_page(FPDF_TEXTPAGE text_page, const unsigned short* term, float* out, int cap) {
  FPDF_SCHHANDLE handle = FPDFText_FindStart(text_page, term, 0, 0);
  if (handle == nullptr) return 0;
  int written = 0;
  while (FPDFText_FindNext(handle)) {
    int start = FPDFText_GetSchResultIndex(handle);
    int count = FPDFText_GetSchCount(handle);
    int rects = FPDFText_CountRects(text_page, start, count);
    for (int r = 0; r < rects; r++) {
      double l = 0, t = 0, ri = 0, b = 0;
      if (!FPDFText_GetRect(text_page, r, &l, &t, &ri, &b)) continue;
      if (written < cap) {
        out[written * 4 + 0] = static_cast<float>(l);
        out[written * 4 + 1] = static_cast<float>(t);
        out[written * 4 + 2] = static_cast<float>(ri);
        out[written * 4 + 3] = static_cast<float>(b);
      }
      written++;
    }
  }
  FPDFText_FindClose(handle);
  return written;
}

}  // extern "C"
```

- [ ] **Step 5: Build arm d and run the test**

```bash
cd pdfium-binding-benchmarks
bash build/link-wasm.sh d
node --test bench/arm-d.test.mjs
node bench/gate.mjs a b c d
```

Expected: PASS, then `fairness gate: a, b, c, d agree on <N> glyphs, page <i>`.

If arms C and D disagree on `geometry`, the cause is almost always the object key: Rust reads `FPDF_PAGEOBJECT` as `u32` while C++ casts a pointer. On wasm32 both are 32 bits and must produce the same value. If they differ, the `count` returned when `out` is null differs, or the empty-glyph branch sets different flags. Compare the first differing record printed by the gate.

- [ ] **Step 6: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "spike: arm d, the same coarse operations in C++ as a control"
```

---

### Task 7: Timing, the round-robin driver, and the wasm results

**Files:**
- Create: `pdfium-binding-benchmarks/bench/node.mjs`
- Create: `pdfium-binding-benchmarks/bench/run.mjs`
- Create: `pdfium-binding-benchmarks/bench/compare.mjs`
- Create: `pdfium-binding-benchmarks/RESULTS.md`
- Create: `pdfium-binding-benchmarks/README.md`
- Test: `pdfium-binding-benchmarks/bench/run.test.mjs`

**Interfaces:**
- Consumes: `measure`, `environment`, `sinkValue` from `../../viewer-rust-port-benchmarks/bench/harness.mjs`; `loadArm`, `wasmBytes`; `openFixture`; `makeWorkloads`; `canonicalFor`, `compare`.
- Produces: `bench/results/<arm>-r<round>.json` shaped `{ arm, round, env, fixture: { pageIndex, glyphCount }, wasmBytes, cells: { <workloadId>: { ns, p95, batches, batch, iters } } }`.

- [ ] **Step 1: Write the per-arm, per-round runner**

`pdfium-binding-benchmarks/bench/node.mjs`:

```js
#!/usr/bin/env node
/**
 * One arm, one round, one results file. Spawned by run.mjs. Each arm gets its
 * own process because each loads a different Emscripten module under the same
 * export name, and one process holding four PDFium instances would share
 * neither a clean heap nor a comparable one.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { environment, measure, sinkValue } from '../../viewer-rust-port-benchmarks/bench/harness.mjs';
import { loadArm, wasmBytes } from './arm.mjs';
import { openFixture } from './fixture.mjs';
import { makeWorkloads } from './workloads.mjs';

const here = import.meta.dirname;
const armName = process.env.BENCH_ARM;
const round = Number(process.env.BENCH_ROUND ?? '1');
const budgetMs = Number(process.env.BENCH_BUDGET_MS ?? '500');

const arm = await loadArm(armName);
const fx = openFixture(arm);
const workloads = makeWorkloads(arm, fx);

const cells = {};
for (const [id, wl] of Object.entries(workloads)) {
  if (id === 'dispose' || !wl.arms.includes(armName)) continue;
  cells[id] = measure(wl.run, { budgetMs });
  process.stdout.write(
    `  ${armName} ${id.padEnd(16)} ${cells[id].ns.toFixed(1).padStart(12)} ns` +
      `  p95 ${cells[id].p95.toFixed(1).padStart(12)} ns\n`,
  );
}

const out = {
  arm: armName,
  round,
  env: await environment(),
  emsdk: process.env.BENCH_EMSDK ?? 'unknown',
  rustc: process.env.BENCH_RUSTC ?? 'unknown',
  fixture: { pageIndex: fx.pageIndex, glyphCount: fx.glyphCount },
  wasmBytes: await wasmBytes(armName),
  sink: sinkValue(),
  cells,
};

const dir = resolve(here, 'results');
mkdirSync(dir, { recursive: true });
writeFileSync(resolve(dir, `${armName}-r${round}.json`), `${JSON.stringify(out, null, 2)}\n`);

workloads.dispose();
fx.close();
arm.close();
```

- [ ] **Step 2: Write the round-robin driver**

`pdfium-binding-benchmarks/bench/run.mjs`:

```js
#!/usr/bin/env node
/**
 * Rounds are INTERLEAVED (round 1: a, b, c, d; round 2: a, b, c, d; ...). A
 * laptop's clock speed drifts over a multi-minute run, and running all of a
 * then all of b would hand the drift to whoever went last. Only medians across
 * interleaved rounds are comparable; a single round's numbers are not.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { canonicalFor, compare } from './gate.mjs';

const here = import.meta.dirname;
const RESULTS = resolve(here, 'results');
const ARMS = ['a', 'b', 'c', 'd'];

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? fallback : process.argv[i + 1];
};
const rounds = Number(arg('rounds', '7'));
const budgetMs = Number(arg('budget', '500'));

for (const a of ARMS) {
  const f = resolve(here, `../build/out/${a}/arm.mjs`);
  if (!existsSync(f)) {
    console.error(`arm ${a} is not built: ${f} is missing.\nRun: bash build/link-wasm.sh`);
    process.exit(1);
  }
}

const version = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8' }).trim().split('\n')[0];
  } catch {
    return 'unknown';
  }
};
const emsdk = version('em++', ['--version']);
const rustc = version('rustc', ['--version']);

// The gate runs FIRST. A wrong arm can be fast, and no timing table shows it.
const canonical = {};
for (const a of ARMS) canonical[a] = await canonicalFor(a);
let ok = true;
for (const a of ARMS.slice(1)) ok = compare('a', canonical.a, a, canonical[a]) && ok;
if (!ok) {
  console.error('\nThe arms do not produce the same answers. Timings would be meaningless.');
  process.exit(1);
}
console.log(
  `fairness gate: ${ARMS.join(', ')} agree on ${canonical.a.glyphCount} glyphs, ` +
    `page ${canonical.a.pageIndex}\n`,
);

rmSync(RESULTS, { recursive: true, force: true });
mkdirSync(RESULTS, { recursive: true });

for (let r = 1; r <= rounds; r++) {
  console.log(`── round ${r}/${rounds} ──`);
  for (const a of ARMS) {
    execFileSync('node', [resolve(here, 'node.mjs')], {
      stdio: ['ignore', 'inherit', 'inherit'],
      env: {
        ...process.env,
        BENCH_ARM: a,
        BENCH_ROUND: String(r),
        BENCH_BUDGET_MS: String(budgetMs),
        BENCH_EMSDK: emsdk,
        BENCH_RUSTC: rustc,
      },
    });
  }
}

console.log(
  `\n${readdirSync(RESULTS).length} result files in bench/results. ` +
    `Now run: node bench/compare.mjs`,
);
```

- [ ] **Step 3: Write the aggregator**

`pdfium-binding-benchmarks/bench/compare.mjs`:

```js
#!/usr/bin/env node
/**
 * Medians across interleaved rounds, printed as a markdown table ready to
 * paste into RESULTS.md. Ratios are against arm a, which is what ships today.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
const RESULTS = resolve(here, 'results');
const ARMS = ['a', 'b', 'c', 'd'];
const LABEL = {
  a: 'a direct',
  b: 'b rust thin',
  c: 'c rust coarse',
  d: 'd c++ coarse',
};

const files = readdirSync(RESULTS).filter((f) => f.endsWith('.json'));
if (files.length === 0) {
  console.error('no results; run: node bench/run.mjs');
  process.exit(1);
}

const byArm = {};
let header = null;
for (const f of files) {
  const j = JSON.parse(readFileSync(resolve(RESULTS, f), 'utf8'));
  header ??= j;
  (byArm[j.arm] ??= []).push(j);
}

const median = (xs) => {
  const s = [...xs].sort((x, y) => x - y);
  if (s.length === 0) return NaN;
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

const ids = [
  ...new Set(Object.values(byArm).flatMap((rs) => rs.flatMap((r) => Object.keys(r.cells)))),
];

const fmt = (ns) => {
  if (!Number.isFinite(ns)) return '—';
  if (ns < 1_000) return `${ns.toFixed(1)} ns`;
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(2)} µs`;
  return `${(ns / 1_000_000).toFixed(3)} ms`;
};

console.log(`\nNode ${header.env.node} · ${header.env.cpu} · ${header.env.platform}`);
console.log(`emsdk ${header.emsdk} · ${header.rustc}`);
console.log(
  `fixture: page ${header.fixture.pageIndex}, ${header.fixture.glyphCount} glyphs · ` +
    `${Object.values(byArm)[0].length} rounds\n`,
);

console.log(`| workload | ${ARMS.map((a) => LABEL[a]).join(' | ')} | b/a | c/a | d/a |`);
console.log(`|---|${ARMS.map(() => '---').join('|')}|---|---|---|`);

for (const id of ids) {
  const med = {};
  for (const a of ARMS) {
    const rows = (byArm[a] ?? []).map((r) => r.cells[id]?.ns).filter(Number.isFinite);
    med[a] = rows.length ? median(rows) : NaN;
  }
  const ratio = (a) => (Number.isFinite(med[a]) ? `${(med[a] / med.a).toFixed(2)}x` : '—');
  console.log(
    `| \`${id}\` | ${ARMS.map((a) => fmt(med[a])).join(' | ')} | ` +
      `${ratio('b')} | ${ratio('c')} | ${ratio('d')} |`,
  );
}

console.log(`\n| arm | wasm bytes | delta vs a |`);
console.log(`|---|---|---|`);
const baseBytes = byArm.a?.[0]?.wasmBytes ?? NaN;
for (const a of ARMS) {
  const b = byArm[a]?.[0]?.wasmBytes;
  if (!Number.isFinite(b)) continue;
  const delta = a === 'a' ? '—' : `+${(((b - baseBytes) / 1024).toFixed(1))} kB`;
  console.log(`| ${LABEL[a]} | ${b.toLocaleString()} | ${delta} |`);
}

const spread = [];
for (const id of ids) {
  for (const a of ARMS) {
    const rows = (byArm[a] ?? []).map((r) => r.cells[id]?.ns).filter(Number.isFinite);
    if (rows.length < 2) continue;
    spread.push({ id, arm: a, ratio: Math.max(...rows) / Math.min(...rows) });
  }
}
spread.sort((x, y) => y.ratio - x.ratio);
console.log(`\nworst round-to-round spread: ${spread
  .slice(0, 3)
  .map((s) => `${s.arm}/${s.id} ${s.ratio.toFixed(2)}x`)
  .join(', ')}`);
console.log('A spread past about 1.3x means thermal drift; say so in the write-up.');
```

- [ ] **Step 4: Write the smoke test**

`pdfium-binding-benchmarks/bench/run.test.mjs`:

```js
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

const here = import.meta.dirname;

test('a two-round run writes eight result files with plausible cells', () => {
  execFileSync('node', [resolve(here, 'run.mjs'), '--rounds', '2', '--budget', '100'], {
    stdio: ['ignore', 'inherit', 'inherit'],
  });

  for (const arm of ['a', 'b', 'c', 'd']) {
    for (const r of [1, 2]) {
      const f = resolve(here, `results/${arm}-r${r}.json`);
      assert.ok(existsSync(f), `missing ${f}`);
      const j = JSON.parse(readFileSync(f, 'utf8'));
      assert.equal(j.arm, arm);
      assert.ok(j.wasmBytes > 100_000, `implausible wasm size ${j.wasmBytes}`);
      assert.ok(j.cells.geometry.ns > 0);
      assert.ok(j.env.cpu && j.env.node, 'environment stamp missing');
      assert.notEqual(j.emsdk, 'unknown');
    }
  }

  const a = JSON.parse(readFileSync(resolve(here, 'results/a-r1.json'), 'utf8'));
  assert.ok(
    a.cells['render-4x'].ns > a.cells['render-1x'].ns,
    'rendering at 4x should cost more than at 1x; if not, the bitmap is not being drawn',
  );
  assert.ok(
    a.cells.geometry.ns > a.cells['null-call'].ns * 100,
    'the glyph loop should dwarf a single null call',
  );
});
```

- [ ] **Step 5: Run the test to verify it fails, then passes**

```bash
cd pdfium-binding-benchmarks && node --test bench/run.test.mjs
```

Expected before writing the files: FAIL, `Cannot find module '.../bench/run.mjs'`. After Steps 1 through 3 are in place: PASS, one test, taking a few minutes.

- [ ] **Step 6: Run the real measurement**

```bash
cd pdfium-binding-benchmarks
bash build/link-wasm.sh
node bench/run.mjs --rounds 7
node bench/compare.mjs | tee /tmp/wasm-tables.md
```

Expected: the gate line, seven rounds of four arms, then two markdown tables and a spread line.

- [ ] **Step 7: Write the results and the README**

Create `pdfium-binding-benchmarks/RESULTS.md` with an `## Environment` section (paste the header `compare.mjs` printed), a `## Wasm` section (paste both tables), and a `## What the numbers say` section written against the predictions in `docs/research/pdfium-rust-wrapper-benchmarks.md`. State plainly, in that last section: that `geometry` is a transcription of `PageGeometryReader` and not the reader itself; that the arm loader skips the engine's per-call argument bridge, so these are lower bounds on real per-call cost; and what the arm C versus arm D comparison came out at, because that is the result the port decision turns on.

Create `pdfium-binding-benchmarks/README.md` saying what the four arms are, the three commands to reproduce (`bash build/fetch.sh wasm32`, `bash build/link-wasm.sh`, `node bench/run.mjs --rounds 7`), and that this is a spike nothing published depends on.

- [ ] **Step 8: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "bench: wasm results for four PDFium wrapper arms over five workloads"
```

---

### Task 8: Native arms A and B

The native baseline is the addon that actually ships, so arm A here is `packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node` and not a spike rebuild. That introduces one confound worth naming in the write-up: arm A is `node-addon-api` C++ and arm B is `napi-rs` Rust, so an A-versus-B difference mixes the Rust hop with the binding framework. Arm C versus arm B is clean, and arm A versus arm D is clean.

**Files:**
- Create: `pdfium-binding-benchmarks/crates/napi-arms/Cargo.toml`
- Create: `pdfium-binding-benchmarks/crates/napi-arms/build.rs`
- Create: `pdfium-binding-benchmarks/crates/napi-arms/src/lib.rs`
- Create: `pdfium-binding-benchmarks/build/build-native.sh`
- Modify: `pdfium-binding-benchmarks/Cargo.toml:3` (add `crates/napi-arms` to `members`)
- Modify: `pdfium-binding-benchmarks/bench/arm.mjs` (add the native loader; full replacement of `loadArm` shown below)
- Test: `pdfium-binding-benchmarks/bench/native-b.test.mjs`

**Interfaces:**
- Consumes: `crates/pdfium-sys`, `crates/shim`, `crates/ops`.
- Produces: `loadNativeArm(name)` from `bench/arm.mjs` with the same `{ name, m, fn, mem, close }` shape `loadArm` returns, so `makeWorkloads` and `gate.mjs` need no changes. The napi addon exports `alloc`, `free`, `peek32`, `peekF32`, `peekF64`, `readBytes`, `writeBytes`, `writeU16`, plus every `rs_*` from Task 4 and Task 5.

- [ ] **Step 1: Fetch the native PDFium and build the shipping addon**

```bash
cd pdfium-binding-benchmarks && bash build/fetch.sh darwin-arm64
cd ../packages/engine/runtime && pnpm build:target darwin-arm64
ls -la npm/darwin-arm64/lib/
```

Expected: `pdf-runtime.node` and `libembedpdf.dylib` present. If `cmake-js` fails, that is the shipping build failing, not this spike; fix it there.

- [ ] **Step 2: Write the failing test**

`pdfium-binding-benchmarks/bench/native-b.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalForNative, compare } from './gate.mjs';

test('native arm b agrees with native arm a on every workload they share', async () => {
  const a = await canonicalForNative('a');
  const b = await canonicalForNative('b');
  assert.equal(compare('native-a', a, 'native-b', b), true);
  assert.equal(a.glyphCount, b.glyphCount);
});
```

- [ ] **Step 3: Add `canonicalForNative` to the gate**

Append to `pdfium-binding-benchmarks/bench/gate.mjs`, before the `import.meta.filename` block:

```js
export async function canonicalForNative(armName) {
  const { loadNativeArm } = await import('./arm.mjs');
  const arm = await loadNativeArm(armName);
  const fx = openFixture(arm);
  const w = makeWorkloads(arm, fx);
  const out = { glyphCount: fx.glyphCount, pageIndex: fx.pageIndex };
  for (const [id, wl] of Object.entries(w)) {
    if (id === 'dispose' || !wl.arms.includes(armName)) continue;
    out[id] = wl.canonical();
  }
  w.dispose();
  fx.close();
  arm.close();
  return out;
}
```

- [ ] **Step 4: Write the napi crate**

`pdfium-binding-benchmarks/crates/napi-arms/Cargo.toml`:

```toml
[package]
name = "napi-arms"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
napi = { version = "2", default-features = false, features = ["napi8"] }
napi-derive = "2"
pdfium-sys = { path = "../pdfium-sys" }
shim = { path = "../shim" }
ops = { path = "../ops" }

[build-dependencies]
napi-build = "2"
```

`pdfium-binding-benchmarks/crates/napi-arms/build.rs`:

```rust
fn main() {
    napi_build::setup();
    let lib_dir = std::env::var("PDFIUM_LIB_DIR").expect("PDFIUM_LIB_DIR is required");
    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=dylib=embedpdf");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
}
```

`pdfium-binding-benchmarks/crates/napi-arms/src/lib.rs`:

```rust
//! Native arms B and C behind one addon. Every export mirrors the wasm arm of
//! the same name, so bench/workloads.mjs needs no native-specific branch.
//!
//! Pointers cross as f64. JavaScript numbers hold every integer below 2^53
//! exactly and a macOS user-space pointer is 47 bits, so this is lossless, and
//! it avoids the BigInt conversion the shipping addon pays on every call.
#![allow(non_snake_case)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use pdfium_sys::*;

#[inline]
fn p(v: f64) -> usize {
    v as usize
}

#[napi]
pub fn init_library() {
    unsafe { FPDF_InitLibrary() }
}

#[napi]
pub fn destroy_library() {
    unsafe { FPDF_DestroyLibrary() }
}

#[napi]
pub fn alloc(bytes: u32) -> f64 {
    let mut v: Vec<u8> = Vec::with_capacity(bytes as usize);
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr as usize as f64
}

#[napi]
pub fn free_ptr(_ptr: f64) {
    // Deliberately leaks. The benchmark allocates a fixed set of scratch
    // buffers once and lives for a few seconds; tracking capacities to
    // reconstruct the Vec would add bookkeeping to no measured benefit.
}

#[napi]
pub fn peek32(ptr: f64) -> i32 {
    unsafe { *(p(ptr) as *const i32) }
}

#[napi]
pub fn peek_f32(ptr: f64) -> f64 {
    unsafe { *(p(ptr) as *const f32) as f64 }
}

#[napi]
pub fn peek_f64(ptr: f64) -> f64 {
    unsafe { *(p(ptr) as *const f64) }
}

#[napi]
pub fn read_bytes(ptr: f64, len: u32) -> Buffer {
    let slice = unsafe { std::slice::from_raw_parts(p(ptr) as *const u8, len as usize) };
    Buffer::from(slice.to_vec())
}

#[napi]
pub fn write_bytes(ptr: f64, data: Buffer) {
    let dst = unsafe { std::slice::from_raw_parts_mut(p(ptr) as *mut u8, data.len()) };
    dst.copy_from_slice(&data);
}

#[napi]
pub fn write_u16(s: String) -> f64 {
    let mut units: Vec<u16> = s.encode_utf16().collect();
    units.push(0);
    let ptr = units.as_mut_ptr();
    std::mem::forget(units);
    ptr as usize as f64
}

#[napi]
pub fn load_mem_document(data: Buffer) -> f64 {
    let mut owned = data.to_vec();
    let ptr = owned.as_mut_ptr();
    let len = owned.len() as i32;
    std::mem::forget(owned);
    unsafe { FPDF_LoadMemDocument(ptr, len, std::ptr::null()) as f64 }
}

macro_rules! passthrough {
    ($name:ident, $target:path, ($($arg:ident : $ty:ty),*) -> $ret:ty) => {
        #[napi]
        pub fn $name($($arg: $ty),*) -> $ret {
            unsafe { $target($($arg as _),*) as $ret }
        }
    };
}

passthrough!(close_document, FPDF_CloseDocument, (doc: f64) -> ());
passthrough!(get_page_count, FPDF_GetPageCount, (doc: f64) -> i32);
passthrough!(load_page, FPDF_LoadPage, (doc: f64, index: i32) -> f64);
passthrough!(close_page, FPDF_ClosePage, (page: f64) -> ());
passthrough!(get_page_width, FPDF_GetPageWidthF, (page: f64) -> f64);
passthrough!(get_page_height, FPDF_GetPageHeightF, (page: f64) -> f64);
passthrough!(text_load_page, FPDFText_LoadPage, (page: f64) -> f64);
passthrough!(text_close_page, FPDFText_ClosePage, (tp: f64) -> ());
passthrough!(text_count_chars, FPDFText_CountChars, (tp: f64) -> i32);
passthrough!(text_get_text_object, FPDFText_GetTextObject, (tp: f64, i: i32) -> f64);
passthrough!(text_get_font_size, FPDFText_GetFontSize, (tp: f64, i: i32) -> f64);
passthrough!(text_find_next, FPDFText_FindNext, (h: f64) -> i32);
passthrough!(text_get_sch_result_index, FPDFText_GetSchResultIndex, (h: f64) -> i32);
passthrough!(text_get_sch_count, FPDFText_GetSchCount, (h: f64) -> i32);
passthrough!(text_find_close, FPDFText_FindClose, (h: f64) -> ());
passthrough!(text_count_rects, FPDFText_CountRects, (tp: f64, s: i32, c: i32) -> i32);
passthrough!(bitmap_destroy, FPDFBitmap_Destroy, (b: f64) -> ());

#[napi]
pub fn page_size_normalized(doc: f64, index: i32, out: f64) -> i32 {
    unsafe { EPDF_GetPageSizeByIndexNormalized(p(doc) as u32, index, p(out) as *mut FS_SIZEF) }
}

#[napi]
pub fn char_geometry(tp: f64, index: i32, out: f64) -> i32 {
    unsafe {
        EPDFText_GetCharGeometry(p(tp) as u32, index, p(out) as *mut EPDF_CHAR_GEOMETRY)
    }
}

#[napi]
pub fn text_full(tp: f64, buf: f64, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(p(tp) as u32, p(buf) as *mut u16, len) }
}

#[napi]
pub fn text_find_start(tp: f64, term: f64, flags: u32, start: i32) -> f64 {
    unsafe { FPDFText_FindStart(p(tp) as u32, p(term) as *const u16, flags, start) as f64 }
}

#[napi]
pub fn text_get_rect(tp: f64, i: i32, l: f64, t: f64, r: f64, b: f64) -> i32 {
    unsafe {
        FPDFText_GetRect(
            p(tp) as u32,
            i,
            p(l) as *mut f64,
            p(t) as *mut f64,
            p(r) as *mut f64,
            p(b) as *mut f64,
        )
    }
}

#[napi]
pub fn bitmap_create(w: i32, h: i32, buf: f64, stride: i32) -> f64 {
    unsafe { FPDFBitmap_CreateEx(w, h, FPDFBitmap_BGRA, p(buf) as *mut _, stride) as f64 }
}

#[napi]
pub fn bitmap_fill_rect(b: f64, l: i32, t: i32, w: i32, h: i32, color: u32) {
    unsafe { FPDFBitmap_FillRect(p(b) as u32, l, t, w, h, color) }
}

#[napi]
pub fn render_page_bitmap(b: f64, page: f64, w: i32, h: i32, rotate: i32, flags: i32) {
    unsafe { FPDF_RenderPageBitmap(p(b) as u32, p(page) as u32, 0, 0, w, h, rotate, flags) }
}

// Arm C: the four coarse operations, calling straight into the ops crate.

#[napi]
pub fn rs_read_page_geometry(tp: f64, out: f64, cap: i32) -> i32 {
    ops::rs_read_page_geometry(p(tp) as u32, p(out) as *mut u8, cap)
}

#[napi]
pub fn rs_read_page_text(tp: f64, buf: f64, len: i32) -> i32 {
    ops::rs_read_page_text(p(tp) as u32, p(buf) as *mut u16, len)
}

#[napi]
pub fn rs_render_page(page: f64, bitmap: f64, w: i32, h: i32, rotate: i32) -> i32 {
    ops::rs_render_page(p(page) as u32, p(bitmap) as u32, w, h, rotate)
}

#[napi]
pub fn rs_search_page(tp: f64, term: f64, out: f64, cap: i32) -> i32 {
    ops::rs_search_page(p(tp) as u32, p(term) as *const u16, p(out) as *mut f32, cap)
}
```

Note on handle width: `crates/pdfium-sys` declares handles as `u32` because the wasm arms need them that way, and on `aarch64-apple-darwin` a real `FPDF_DOCUMENT` is 64 bits. Before building, add a `#[cfg]` to `crates/pdfium-sys/src/lib.rs` so the aliases widen on 64-bit targets:

```rust
#[cfg(target_pointer_width = "32")]
pub type FPDF_DOCUMENT = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_DOCUMENT = u64;
```

Repeat the same pair for `FPDF_PAGE`, `FPDF_TEXTPAGE`, `FPDF_PAGEOBJECT`, `FPDF_BITMAP` and `FPDF_SCHHANDLE`, and change the `as u32` casts in `crates/ops/src/lib.rs` and `crates/napi-arms/src/lib.rs` to `as _`. The record's `object_key` stays `u32`: it is a grouping key compared for equality within one page, and truncating a pointer preserves that.

- [ ] **Step 5: Write the native build script**

`pdfium-binding-benchmarks/build/build-native.sh`:

```bash
#!/usr/bin/env bash
# Native arms. Arm A is the addon that actually ships, built out of
# packages/engine/runtime; arms B and C are one napi-rs addon; arm D is a
# cmake-js addon mirroring the shipping CMakeLists.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-darwin-arm64}"
LIB_DIR="$ROOT/build/libpdfium/$TARGET/lib"
OUT_DIR="$ROOT/build/out/native"

mkdir -p "$OUT_DIR"

PDFIUM_LIB_DIR="$LIB_DIR" cargo build --manifest-path "$ROOT/Cargo.toml" \
  -p napi-arms --release
cp "$ROOT/target/release/libnapi_arms.dylib" "$OUT_DIR/napi-arms.node.tmp"
mv -f "$OUT_DIR/napi-arms.node.tmp" "$OUT_DIR/napi-arms.node"
cp "$LIB_DIR/libembedpdf.dylib" "$OUT_DIR/libembedpdf.dylib.tmp"
mv -f "$OUT_DIR/libembedpdf.dylib.tmp" "$OUT_DIR/libembedpdf.dylib"

echo "native arms b,c -> $OUT_DIR/napi-arms.node"
```

The `.tmp`-then-`mv` dance is not decoration. Copying onto an existing binary rewrites the same inode, and the macOS kernel's per-inode code-signature cache then kills the next process that `dlopen`s it. The shipping script explains this at `packages/engine/runtime/scripts/build-target.sh:47`.

- [ ] **Step 6: Add the native loader**

Append to `pdfium-binding-benchmarks/bench/arm.mjs`:

```js
/**
 * Native arms. Arm a is the addon that ships, from
 * packages/engine/runtime/npm/<target>/lib/pdf-runtime.node, which uses BigInt
 * pointers; arms b and c are one napi-rs addon using f64 pointers. The
 * adapters below hide that difference so makeWorkloads sees one shape.
 */
export async function loadNativeArm(name, target = 'darwin-arm64') {
  const { createRequire } = await import('node:module');
  const require = createRequire(import.meta.url);

  if (name === 'a') {
    const addon = require(
      resolve(here, `../../packages/engine/runtime/npm/${target}/lib/pdf-runtime.node`),
    );
    // The shipping addon takes BigInt pointers. Which ARGUMENTS are pointers
    // is per-function, so it is spelled out rather than guessed: an earlier
    // draft keyed the conversion off the function name and silently passed
    // Numbers where the addon expects BigInt.
    const P = (v) => (typeof v === 'bigint' ? v : BigInt(Math.trunc(v)));
    const raw = addon.fn ?? addon;
    const fn = {
      FPDF_InitLibrary: () => raw.FPDF_InitLibrary(),
      FPDF_DestroyLibrary: () => raw.FPDF_DestroyLibrary(),
      FPDF_LoadMemDocument: (buf, len, pw) => raw.FPDF_LoadMemDocument(P(buf), len, P(pw)),
      FPDF_CloseDocument: (doc) => raw.FPDF_CloseDocument(P(doc)),
      FPDF_GetPageCount: (doc) => raw.FPDF_GetPageCount(P(doc)),
      FPDF_LoadPage: (doc, i) => raw.FPDF_LoadPage(P(doc), i),
      FPDF_ClosePage: (pg) => raw.FPDF_ClosePage(P(pg)),
      FPDF_GetPageWidthF: (pg) => raw.FPDF_GetPageWidthF(P(pg)),
      FPDF_GetPageHeightF: (pg) => raw.FPDF_GetPageHeightF(P(pg)),
      EPDF_GetPageSizeByIndexNormalized: (doc, i, out) =>
        raw.EPDF_GetPageSizeByIndexNormalized(P(doc), i, P(out)),
      FPDFText_LoadPage: (pg) => raw.FPDFText_LoadPage(P(pg)),
      FPDFText_ClosePage: (tp) => raw.FPDFText_ClosePage(P(tp)),
      FPDFText_CountChars: (tp) => raw.FPDFText_CountChars(P(tp)),
      FPDFText_GetTextObject: (tp, i) => raw.FPDFText_GetTextObject(P(tp), i),
      FPDFText_GetFontSize: (tp, i) => raw.FPDFText_GetFontSize(P(tp), i),
      EPDFText_GetCharGeometry: (tp, i, out) =>
        raw.EPDFText_GetCharGeometry(P(tp), i, P(out)),
      EPDFText_GetTextFull: (tp, buf, len) => raw.EPDFText_GetTextFull(P(tp), P(buf), len),
      FPDFText_FindStart: (tp, term, flags, start) =>
        raw.FPDFText_FindStart(P(tp), P(term), flags, start),
      FPDFText_FindNext: (h) => raw.FPDFText_FindNext(P(h)),
      FPDFText_GetSchResultIndex: (h) => raw.FPDFText_GetSchResultIndex(P(h)),
      FPDFText_GetSchCount: (h) => raw.FPDFText_GetSchCount(P(h)),
      FPDFText_FindClose: (h) => raw.FPDFText_FindClose(P(h)),
      FPDFText_CountRects: (tp, s, c) => raw.FPDFText_CountRects(P(tp), s, c),
      FPDFText_GetRect: (tp, i, l, t, r, b) =>
        raw.FPDFText_GetRect(P(tp), i, P(l), P(t), P(r), P(b)),
      FPDFBitmap_CreateEx: (w, h, fmt, buf, stride) =>
        raw.FPDFBitmap_CreateEx(w, h, fmt, P(buf), stride),
      FPDFBitmap_FillRect: (b, l, t, w, h, color) =>
        raw.FPDFBitmap_FillRect(P(b), l, t, w, h, color),
      FPDFBitmap_Destroy: (b) => raw.FPDFBitmap_Destroy(P(b)),
      FPDF_RenderPageBitmap: (b, pg, x, y, w, h, rot, flags) =>
        raw.FPDF_RenderPageBitmap(P(b), P(pg), x, y, w, h, rot, flags),
    };
    const mem = {
      alloc: (b) => raw.alloc(b),
      free: (p) => raw.free(P(p)),
      peek32: (p) => Number(raw.peek(P(p), 'i32')),
      peekF32: (p) => Number(raw.peek(P(p), 'f32')),
      peekF64: (p) => Number(raw.peek(P(p), 'f64')),
      readBytes: (p, len) => new Uint8Array(raw.readBytes(P(p), len)),
      writeBytes: (p, d) =>
        raw.writeBytes(P(p), d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength)),
      writeU16: (s) => raw.writeU16String(s),
      readU16: (p, units) => {
        const bytes = new Uint8Array(raw.readBytes(P(p), units * 2));
        const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, units);
        let out = '';
        const CHUNK = 0x2000;
        for (let i = 0; i < u16.length; i += CHUNK) {
          out += String.fromCharCode(...u16.subarray(i, Math.min(i + CHUNK, u16.length)));
        }
        return out;
      },
    };
    fn.FPDF_InitLibrary();
    return { name, m: addon, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
  }

  const addon = require(resolve(here, '../build/out/native/napi-arms.node'));
  const fn = {
    FPDF_InitLibrary: addon.initLibrary,
    FPDF_DestroyLibrary: addon.destroyLibrary,
    FPDF_LoadMemDocument: (_ptr, _len, _pw) => {
      throw new Error('native arms load through loadMemDocument(Buffer)');
    },
    FPDF_CloseDocument: addon.closeDocument,
    FPDF_GetPageCount: addon.getPageCount,
    FPDF_LoadPage: addon.loadPage,
    FPDF_ClosePage: addon.closePage,
    FPDF_GetPageWidthF: addon.getPageWidth,
    FPDF_GetPageHeightF: addon.getPageHeight,
    EPDF_GetPageSizeByIndexNormalized: addon.pageSizeNormalized,
    rs_EPDF_GetPageSizeByIndexNormalized: addon.pageSizeNormalized,
    FPDFText_LoadPage: addon.textLoadPage,
    FPDFText_ClosePage: addon.textClosePage,
    FPDFText_CountChars: addon.textCountChars,
    rs_FPDFText_CountChars: addon.textCountChars,
    FPDFText_GetTextObject: addon.textGetTextObject,
    rs_FPDFText_GetTextObject: addon.textGetTextObject,
    FPDFText_GetFontSize: addon.textGetFontSize,
    rs_FPDFText_GetFontSize: addon.textGetFontSize,
    EPDFText_GetCharGeometry: addon.charGeometry,
    rs_EPDFText_GetCharGeometry: addon.charGeometry,
    EPDFText_GetTextFull: addon.textFull,
    rs_EPDFText_GetTextFull: addon.textFull,
    FPDFText_FindStart: addon.textFindStart,
    rs_FPDFText_FindStart: addon.textFindStart,
    FPDFText_FindNext: addon.textFindNext,
    rs_FPDFText_FindNext: addon.textFindNext,
    FPDFText_GetSchResultIndex: addon.textGetSchResultIndex,
    rs_FPDFText_GetSchResultIndex: addon.textGetSchResultIndex,
    FPDFText_GetSchCount: addon.textGetSchCount,
    rs_FPDFText_GetSchCount: addon.textGetSchCount,
    FPDFText_FindClose: addon.textFindClose,
    rs_FPDFText_FindClose: addon.textFindClose,
    FPDFText_CountRects: addon.textCountRects,
    rs_FPDFText_CountRects: addon.textCountRects,
    FPDFText_GetRect: addon.textGetRect,
    rs_FPDFText_GetRect: addon.textGetRect,
    FPDFBitmap_CreateEx: (w, h, _fmt, buf, stride) => addon.bitmapCreate(w, h, buf, stride),
    FPDFBitmap_FillRect: addon.bitmapFillRect,
    FPDFBitmap_Destroy: addon.bitmapDestroy,
    FPDF_RenderPageBitmap: (b, page, _x, _y, w, h, rot, flags) =>
      addon.renderPageBitmap(b, page, w, h, rot, flags),
    rs_FPDF_RenderPageBitmap: (b, page, _x, _y, w, h, rot, flags) =>
      addon.renderPageBitmap(b, page, w, h, rot, flags),
    rs_peek_i32: addon.peek32,
    rs_read_page_geometry: addon.rsReadPageGeometry,
    rs_read_page_text: addon.rsReadPageText,
    rs_render_page: addon.rsRenderPage,
    rs_search_page: addon.rsSearchPage,
  };
  const mem = {
    alloc: addon.alloc,
    free: addon.freePtr,
    peek32: addon.peek32,
    peekF32: addon.peekF32,
    peekF64: addon.peekF64,
    readBytes: (p, len) => new Uint8Array(addon.readBytes(p, len)),
    writeBytes: (p, d) => addon.writeBytes(p, Buffer.from(d)),
    writeU16: addon.writeU16,
    readU16: (p, units) => {
      const bytes = new Uint8Array(addon.readBytes(p, units * 2));
      const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, units);
      let out = '';
      const CHUNK = 0x2000;
      for (let i = 0; i < u16.length; i += CHUNK) {
        out += String.fromCharCode(...u16.subarray(i, Math.min(i + CHUNK, u16.length)));
      }
      return out;
    },
  };
  fn.FPDF_InitLibrary();
  return { name, m: addon, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
}
```

`openFixture` in `bench/fixture.mjs` calls `FPDF_LoadMemDocument(buf, len, 0)`, which native arms cannot serve that way. Change `openFixture` to prefer a document loader when the arm provides one, by replacing its first six lines with:

```js
  const { fn, mem } = arm;
  const bytes = readFileSync(PDF_PATH);
  let buf = 0;
  let doc;
  if (arm.m.loadMemDocument) {
    doc = arm.m.loadMemDocument(Buffer.from(bytes));
  } else {
    buf = mem.alloc(bytes.length);
    mem.writeBytes(buf, bytes);
    doc = fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  }
```

and its `close()` to `if (buf) mem.free(buf);`.

- [ ] **Step 7: Build and run the test**

```bash
cd pdfium-binding-benchmarks
bash build/build-native.sh darwin-arm64
node --test bench/native-b.test.mjs
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add pdfium-binding-benchmarks
git commit -m "spike: native arms a and b, shipping C++ addon against a napi-rs shim"
```

---

### Task 9: Native arms C and D, and the native results

**Files:**
- Create: `pdfium-binding-benchmarks/build/CMakeLists.txt`
- Create: `pdfium-binding-benchmarks/cpp/ops_napi.cc`
- Modify: `pdfium-binding-benchmarks/build/build-native.sh` (append the arm D build)
- Modify: `pdfium-binding-benchmarks/bench/arm.mjs` (add arm d to `loadNativeArm`)
- Modify: `pdfium-binding-benchmarks/bench/run.mjs` (add `--native`)
- Modify: `pdfium-binding-benchmarks/bench/node.mjs` (honour `BENCH_NATIVE`)
- Modify: `pdfium-binding-benchmarks/RESULTS.md`
- Test: `pdfium-binding-benchmarks/bench/native-cd.test.mjs`

**Interfaces:**
- Consumes: `cpp/ops.cc` and `cpp/record.h` from Task 6, unchanged.
- Produces: a `cc-ops.node` addon exporting `ccReadPageGeometry`, `ccReadPageText`, `ccRenderPage`, `ccSearchPage` with the same argument shapes as their `rs*` counterparts in Task 8. `loadNativeArm('c')` returns the napi addon with `rs_*` coarse functions wired; `loadNativeArm('d')` returns the cmake addon with `cc_*` wired and the napi addon supplying memory helpers.

- [ ] **Step 1: Write the failing test**

`pdfium-binding-benchmarks/bench/native-cd.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalForNative, compare } from './gate.mjs';

test('native arms c and d agree with native arm a, and with each other', async () => {
  const a = await canonicalForNative('a');
  const c = await canonicalForNative('c');
  const d = await canonicalForNative('d');
  assert.equal(compare('native-a', a, 'native-c', c), true);
  assert.equal(compare('native-a', a, 'native-d', d), true);
  assert.equal(JSON.stringify(c.geometry), JSON.stringify(d.geometry));
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd pdfium-binding-benchmarks && node --test bench/native-cd.test.mjs
```

Expected: FAIL, `unknown arm: d` from `loadNativeArm`.

- [ ] **Step 3: Write the C++ N-API wrapper for arm D**

`pdfium-binding-benchmarks/cpp/ops_napi.cc`:

```cpp
// Arm D, native. Wraps the SAME cpp/ops.cc used by the wasm arm in a
// node-addon-api shim, so arm A and arm D share a binding framework and
// A-versus-D isolates "coarse" from everything else.
#include <napi.h>

#include <cstdint>

extern "C" {
int cc_read_page_geometry(void* text_page, uint8_t* out, int cap);
int cc_read_page_text(void* text_page, unsigned short* buf, int len);
int cc_render_page(void* page, void* bitmap, int w, int h, int rotate);
int cc_search_page(void* text_page, const unsigned short* term, float* out, int cap);
}

namespace {

template <typename T>
T Ptr(const Napi::Value& v) {
  return reinterpret_cast<T>(static_cast<uintptr_t>(v.As<Napi::Number>().DoubleValue()));
}

Napi::Value ReadPageGeometry(const Napi::CallbackInfo& info) {
  int n = cc_read_page_geometry(Ptr<void*>(info[0]), Ptr<uint8_t*>(info[1]),
                                info[2].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value ReadPageText(const Napi::CallbackInfo& info) {
  int n = cc_read_page_text(Ptr<void*>(info[0]), Ptr<unsigned short*>(info[1]),
                            info[2].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value RenderPage(const Napi::CallbackInfo& info) {
  int n = cc_render_page(Ptr<void*>(info[0]), Ptr<void*>(info[1]),
                         info[2].As<Napi::Number>().Int32Value(),
                         info[3].As<Napi::Number>().Int32Value(),
                         info[4].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value SearchPage(const Napi::CallbackInfo& info) {
  int n = cc_search_page(Ptr<void*>(info[0]), Ptr<const unsigned short*>(info[1]),
                         Ptr<float*>(info[2]), info[3].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports.Set("ccReadPageGeometry", Napi::Function::New(env, ReadPageGeometry));
  exports.Set("ccReadPageText", Napi::Function::New(env, ReadPageText));
  exports.Set("ccRenderPage", Napi::Function::New(env, RenderPage));
  exports.Set("ccSearchPage", Napi::Function::New(env, SearchPage));
  return exports;
}

}  // namespace

NODE_API_MODULE(cc_ops, Init)
```

`cpp/ops.cc` declares its PDFium handle parameters with PDFium's own typedefs, which are pointers on a 64-bit target and `unsigned int` under Emscripten. The `void*` declarations above match on 64-bit. Add this guard at the top of `cpp/ops.cc`, right after the includes, so the wasm build is unaffected:

```cpp
// On wasm32 the FPDF_* handle typedefs are 32-bit; on 64-bit natives they are
// pointers. ops_napi.cc passes void*, which matches the native case.
static_assert(sizeof(FPDF_TEXTPAGE) == sizeof(void*) || sizeof(FPDF_TEXTPAGE) == 4,
              "unexpected PDFium handle width");
```

- [ ] **Step 4: Write the CMake file**

`pdfium-binding-benchmarks/build/CMakeLists.txt`:

```cmake
# Mirrors packages/engine/runtime/build/CMakeLists.txt, trimmed to macOS and to
# the two sources arm D needs.
cmake_minimum_required(VERSION 3.15)
project(cc_ops)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED PDFIUM_LIB_DIR)
  message(FATAL_ERROR "PDFIUM_LIB_DIR is required")
endif()
if(NOT DEFINED PDFIUM_INCLUDE_DIR)
  message(FATAL_ERROR "PDFIUM_INCLUDE_DIR is required")
endif()

add_library(${PROJECT_NAME} SHARED
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops.cc"
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops_napi.cc"
)

target_include_directories(${PROJECT_NAME} PRIVATE
  ${CMAKE_JS_INC}
  "${PDFIUM_INCLUDE_DIR}"
  "${CMAKE_CURRENT_LIST_DIR}/../cpp"
)

target_link_libraries(${PROJECT_NAME} PRIVATE
  ${CMAKE_JS_LIB}
  "${PDFIUM_LIB_DIR}/libembedpdf.dylib"
  "-framework AppKit"
  "-framework CoreFoundation"
  "-framework CoreGraphics"
  "-framework Foundation"
)

set_target_properties(${PROJECT_NAME} PROPERTIES
  PREFIX ""
  SUFFIX ".node"
  OUTPUT_NAME "cc-ops"
  BUILD_RPATH "@loader_path"
  INSTALL_RPATH "@loader_path"
)
```

`cmake-js` needs `node-addon-api` on disk. Add `pdfium-binding-benchmarks/package.json`:

```json
{
  "name": "pdfium-binding-benchmarks",
  "private": true,
  "type": "module",
  "devDependencies": {
    "cmake-js": "^7.3.1",
    "node-addon-api": "^8.0.0"
  }
}
```

Install it with `npm install --prefix pdfium-binding-benchmarks` rather than `pnpm`, so the spike stays out of the workspace lockfile.

- [ ] **Step 5: Append arm D to the native build script**

Append to `pdfium-binding-benchmarks/build/build-native.sh`:

```bash
INCLUDE_DIR="$ROOT/build/libpdfium/$TARGET/include"
npx --prefix "$ROOT" cmake-js compile \
  --directory "$ROOT/build" \
  --arch arm64 \
  --CDPDFIUM_LIB_DIR="$LIB_DIR" \
  --CDPDFIUM_INCLUDE_DIR="$INCLUDE_DIR"

cp "$ROOT/build/build/Release/cc-ops.node" "$OUT_DIR/cc-ops.node.tmp"
mv -f "$OUT_DIR/cc-ops.node.tmp" "$OUT_DIR/cc-ops.node"
echo "native arm d -> $OUT_DIR/cc-ops.node"
```

- [ ] **Step 6: Wire arms c and d into the native loader**

In `pdfium-binding-benchmarks/bench/arm.mjs`, inside `loadNativeArm`, after the `const addon = require(...napi-arms.node)` line, add:

```js
  if (name === 'd') {
    const cc = require(resolve(here, '../build/out/native/cc-ops.node'));
    Object.assign(fnExtra, {
      cc_read_page_geometry: cc.ccReadPageGeometry,
      cc_read_page_text: cc.ccReadPageText,
      cc_render_page: cc.ccRenderPage,
      cc_search_page: cc.ccSearchPage,
    });
  }
```

and declare `const fnExtra = {};` above the `fn` literal, then spread it in: change the closing of the `fn` object literal from `};` to `, ...fnExtra };`. Arm `c` needs nothing extra: its `rs_*` coarse entries are already in the `fn` literal from Task 8.

- [ ] **Step 7: Teach the driver about native**

In `pdfium-binding-benchmarks/bench/run.mjs`, add after the `budgetMs` line:

```js
const native = process.argv.includes('--native');
```

Change the build-check loop to skip when `native` is set, and check `build/out/native/napi-arms.node` and `build/out/native/cc-ops.node` instead. Change the gate loop to call `canonicalForNative` when `native` is set (import it alongside `canonicalFor`). Pass `BENCH_NATIVE: native ? '1' : ''` in the spawn env.

In `pdfium-binding-benchmarks/bench/node.mjs`, change the arm load to:

```js
const arm = process.env.BENCH_NATIVE
  ? await (await import('./arm.mjs')).loadNativeArm(armName)
  : await loadArm(armName);
```

and guard the size field: `wasmBytes: process.env.BENCH_NATIVE ? null : await wasmBytes(armName),`. Write native results to `results/native-${armName}-r${round}.json`. In `bench/compare.mjs`, accept a `--native` flag that reads only the `native-` prefixed files and skips the wasm-size table.

- [ ] **Step 8: Run the test, then the measurement**

```bash
cd pdfium-binding-benchmarks
bash build/build-native.sh darwin-arm64
node --test bench/native-cd.test.mjs
node bench/run.mjs --native --rounds 7
node bench/compare.mjs --native | tee /tmp/native-tables.md
```

Expected: PASS on the test, then the gate line, seven rounds of four arms, then one markdown table.

- [ ] **Step 9: Add the native section to RESULTS.md and commit**

Add a `## Native` section with the table, and add to `## What the numbers say`: the native arm C-versus-arm D comparison, and an explicit note that native arm A is `node-addon-api` C++ while arm B is `napi-rs` Rust, so an A-versus-B gap mixes the Rust hop with the binding framework and only B-versus-C and A-versus-D are clean comparisons.

```bash
git add pdfium-binding-benchmarks
git commit -m "bench: native results for four PDFium wrapper arms"
```

---

### Task 10: Write the findings back into the research docs

**Files:**
- Modify: `docs/research/pdfium-rust-wrapper-benchmarks.md` (append a `## What actually happened` section)
- Modify: `docs/research/rust-core-port.md:203-210` (the `## What I do not know` section)

**Interfaces:**
- Consumes: `pdfium-binding-benchmarks/RESULTS.md`.
- Produces: nothing code depends on.

- [ ] **Step 1: Append the findings section**

Add `## What actually happened` to the end of `docs/research/pdfium-rust-wrapper-benchmarks.md`, following the shape `docs/research/rust-core-port-benchmarks.md` uses for the same section: lead with the predictions that were wrong, link to `pdfium-binding-benchmarks/RESULTS.md` for the full tables, and give each surprise its own short paragraph.

Cover, at minimum: the thin shim's measured tax against the predicted 15%; the arm C versus arm D result and what it means for whether the fork should grow `EPDF*` functions in C++ instead; whether wasm and native disagreed the way the `mem.peek` asymmetry predicted; and the wasm size delta.

Follow the house voice. Load the `writing-style` skill first and use the essay register, matching the two sibling documents.

- [ ] **Step 2: Replace the missing number in the port plan**

`docs/research/rust-core-port.md` currently reads, in `## What I do not know`:

> I also have no measurement of what a realistic WASM crossing costs on our actual `Geom` payloads.

Replace that paragraph with the measured per-call cost from the null probe and a pointer to the new document. Leave the rest of `rust-core-port.md` untouched; the writing-style scope rule applies and only this paragraph is in scope.

- [ ] **Step 3: Commit**

```bash
git add docs/research
git commit -m "docs: record what the PDFium Rust wrapper benchmark measured"
```

---

## Self-Review

**Spec coverage.** Gate: Task 1. Four arms: Tasks 2, 4, 5, 6 for wasm, Tasks 8 and 9 for native. Fairness gate: Task 3, enforced before timing in Task 7's `run.mjs`. Five workloads: Task 3, including both render scales and the three separate null probes. Method (warmup, budget, median and p95, sink, round-robin at k=7, environment stamp): Task 7, inherited from `viewer-rust-port-benchmarks/bench/harness.mjs`. Wasm byte size: Task 7. Native phase: Tasks 8 and 9. Write-up: Tasks 7, 9 and 10.

Two spec items are deliberately not implemented, both recorded under "Deviations from the spec" above: `bindgen`, replaced by 21 hand-written declarations, and importing `PageGeometryReader`, replaced by a transcription of its loop.

**Name consistency.** `loadArm` / `loadNativeArm` / `wasmBytes` (arm.mjs); `openFixture` / `PDF_PATH` / `SEARCH_TERM` / `PAGE_INDEX` (fixture.mjs); `makeWorkloads` / `RECORD_BYTES` (workloads.mjs); `canonicalFor` / `canonicalForNative` / `compare` (gate.mjs). Workload ids are `null-call`, `null-peek`, `null-readbytes`, `geometry`, `text`, `render-1x`, `render-4x`, `search`, used identically in `workloads.mjs`, `gate.mjs`, `node.mjs` and `compare.mjs`. The 112-byte record layout appears in `crates/ops/src/record.rs`, `cpp/record.h` and the `geometryCoarse` decoder in `bench/workloads.mjs`, and Task 6's test asserts arms C and D produce it identically.
