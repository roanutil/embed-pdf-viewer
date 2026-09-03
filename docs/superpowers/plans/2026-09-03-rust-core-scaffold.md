# Rust Core Scaffold Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rust-core-scaffold/`, a self-contained multiplatform scaffold where one Rust crate exposes six coarse PDF operations over the EmbedPDF PDFium fork, wrapped by Swift, Kotlin, Node and wasm packages, each driving a demo app.

**Architecture:** Five Rust crates. `pdfium-sys` is `bindgen` over the fork's C headers. `pdfium` is a safe RAII layer. `epdf-ops` holds the six operations and owns one dedicated OS thread, because every non-wasm fork build sets `embedpdf_thread_local_globals=true` and PDFium handles cannot cross threads. `epdf-node` (napi-rs) and `epdf-wasm` (`staticlib` for `wasm32-unknown-emscripten`) are host adapters; Swift and Kotlin come from uniffi. One frozen JSON vectors file plus one golden BGRA buffer pin every host to the same answers.

**Tech Stack:** Rust 1.98.0, uniffi 0.32.0, napi-rs 3.12.2, bindgen 0.72.1, cargo-ndk 4.1.2, emsdk 3.1.72, Node 24.20.0, Swift 6.3.3 / Xcode 26.6, Kotlin with Gradle and Jetpack Compose, Vite for the web demo.

**Spec:** `docs/superpowers/specs/2026-09-03-rust-core-scaffold-design.md`

## Global Constraints

- Everything lives under `rust-core-scaffold/` at the repo root. Nothing in `packages/`, `cloudpdf/`, `examples/`, `tooling/` or `website/` may import from it or reference it.
- Outside the pnpm workspace by construction: `pnpm-workspace.yaml` globs `packages/**`, `examples/**`, `cloudpdf/**`, `tooling/*`, `website`, `docs/*`. Do not add a glob for the scaffold.
- JavaScript inside the scaffold uses `npm` with its own `package-lock.json`, not `pnpm`. This matches `pdfium-rust-spike/package-lock.json`.
- `rust-toolchain.toml` pins channel `1.98.0`.
- Every Rust crate uses `edition = "2024"`. It stabilized in Rust 1.85 and the toolchain is pinned to 1.98.0. Verified free: the five completed crates build and pass all 17 tests under 2024 with zero warnings, because nothing here has an `unsafe fn`, a `static mut`, a `gen` identifier, or an `-> impl Trait`, and bindgen 0.72.1 already emits `unsafe extern "C"` blocks. `pdfium-rust-spike` and `ts-wasm-interop-poc` use 2021 because they predate 1.85; do not copy that.
- Exact dependency pins: `uniffi = "=0.32.0"`, `napi = "=3.12.2"`, `napi-derive = "=3.6.3"`, `napi-build = "=2.4.1"`, `bindgen = "=0.72.1"`. `cargo-ndk` is installed at `4.1.2`.
- `[profile.release]` sets `opt-level = 3`, `lto = true`, `codegen-units = 1`. It must NOT set `panic = "abort"`, even though `pdfium-rust-spike/Cargo.toml` does: uniffi and napi both map panics to host exceptions through `catch_unwind`, which `panic = "abort"` deletes.
- Fork pin is `d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`, copied verbatim from `packages/engine/runtime/engine-runtime-build.json`.
- Target names follow the fork's spelling (`darwin-arm64`, `wasm32`, `ios-arm64`), which is Node's spelling, not Rust's. `pdfium-rust-spike/crates/napi-arms/build.rs:56-78` has the mapping function to copy.
- emsdk 3.1.72 is already cloned at `pdfium-rust-spike/build/emsdk`; source `pdfium-rust-spike/build/emsdk.sh` to get `em++`. A newer `wasm-opt` must be symlinked over emsdk's bundled v119, which rejects two wasm features present in rustc 1.98.0's precompiled std.
- `build/test-swift.sh` passes `-Xlinker -rpath -Xlinker "$LIBS"` to `swift test`. macOS strips `DYLD_*` from Apple-signed binaries under the hardened runtime, and `swiftpm-testing-helper` is one, so `DYLD_LIBRARY_PATH` never reaches the test bundle. `LIBRARY_PATH` still handles link-time resolution.
- `packages/swift/EpdfOps/Sources/EpdfOpsFFI/`, `Sources/EpdfOps/Generated/` and `Frameworks/` are all gitignored, so a fresh clone must run `bash build/build-swift.sh` before `swift build` can resolve the `systemLibrary` target. Task 7's `packages/swift/EpdfOps/README.md` documents that two-step contract.
- Prose in `README.md` files follows the house voice: no em dashes, exact versions and paths, receipts for claims. Rust doc comments and commit messages follow repo convention (Conventional Commits, as in `bench: re-run both halves` and `fix: correct four code review findings`).
- No changesets. Nothing here is a published package.
- Test helpers are shared, not copied. `crates/test-support` (a dev-dependency, created in Task 4) holds the frozen-vectors loader and `mean_abs_diff` for every Rust suite. `packages/test-support/vectors.mjs` (created in Task 9) holds the same two things for every JS suite. No suite re-declares either.
- The workspace `members` list grows one line per crate task, so `cargo metadata` and `cargo test --workspace` succeed at every commit on the branch.
- `crates/build-support` is a `[build-dependencies]`-only crate holding `lib_root()`, `fork_target()`, `emit_link_directives()` and `emit_rpath_only()`. Every crate that links libembedpdf uses it, and no crate re-declares the target-name mapping.
- Every crate that links libembedpdf needs its OWN `build.rs` calling `build_support::emit_rpath_only()`. Cargo propagates `rustc-link-lib` and `rustc-link-search` from a dependency's build script but NOT `rustc-link-arg`, so an inherited rpath never reaches a dependent package's test binaries: `cargo test -p <crate>` then links and fails at runtime.
- `pdfium::Document::from_bytes` takes `Rc<Library>`, not `&Library`. The document holds the clone, so `FPDF_DestroyLibrary` cannot run while a handle is open. `Rc` rather than `Arc` because `Library`'s refcount is a `thread_local` and `Rc` being `!Send` reinforces that.
- The render comparison asserts BOTH `mean_abs_diff <= render.meanAbsDiffTolerance` (2.0) and `max_abs_diff <= render.maxAbsDiffTolerance` (16). The mean alone lets one wrong region hide inside 121,176 bytes.
- Out of scope for this plan: adding `ios-arm64`, `ios-sim-arm64`, `android-arm64` and `android-x64` to `embedpdf/runtime`. That is a separate spec and a separate PR to a separate repository, running in parallel. Tasks 8 and 14 end at the gate rather than working around it.

---

### Task 1: Directory skeleton, Cargo workspace, and the artifact fetch gate

**Files:**
- Create: `rust-core-scaffold/Cargo.toml`
- Create: `rust-core-scaffold/rust-toolchain.toml`
- Create: `rust-core-scaffold/.gitignore`
- Create: `rust-core-scaffold/build/runtime-build.json`
- Create: `rust-core-scaffold/build/fetch-libpdfium.sh`
- Test: `rust-core-scaffold/build/fetch-libpdfium.test.mjs`

**Interfaces:**
- Consumes: nothing.
- Produces: `bash build/fetch-libpdfium.sh <target>` prints the absolute directory it extracted into on success, exits 2 with a message naming the target on a `"status": "pending"` target, exits 1 on an unknown target or a sha256 mismatch. Artifacts land at `rust-core-scaffold/build/libpdfium/<target>/{include,lib}`.

- [ ] **Step 1: Write the failing test**

Create `rust-core-scaffold/build/fetch-libpdfium.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const script = resolve(here, 'fetch-libpdfium.sh');
const run = promisify(execFile);

async function fetchTarget(target) {
  try {
    const { stdout } = await run('bash', [script, target]);
    return { code: 0, stdout, stderr: '' };
  } catch (error) {
    return { code: error.code, stdout: error.stdout ?? '', stderr: error.stderr ?? '' };
  }
}

test('pin file carries the nine published targets and four pending mobile ones', async () => {
  const pin = JSON.parse(await readFile(resolve(here, 'runtime-build.json'), 'utf8'));
  assert.equal(pin.sha, 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6');

  const published = Object.entries(pin.artifacts)
    .filter(([, a]) => a.url)
    .map(([t]) => t)
    .sort();
  assert.deepEqual(published, [
    'darwin-arm64', 'darwin-x64',
    'linux-arm64', 'linux-x64',
    'linuxmusl-arm64', 'linuxmusl-x64',
    'wasm32',
    'win32-arm64', 'win32-x64',
  ]);

  const pending = Object.entries(pin.artifacts)
    .filter(([, a]) => a.status === 'pending')
    .map(([t]) => t)
    .sort();
  assert.deepEqual(pending, ['android-arm64', 'android-x64', 'ios-arm64', 'ios-sim-arm64']);
  for (const target of pending) {
    assert.equal(pin.artifacts[target].url, undefined);
    assert.equal(pin.artifacts[target].sha256, undefined);
  }
});

test('a pending mobile target exits 2 and names itself', async () => {
  const { code, stderr } = await fetchTarget('ios-arm64');
  assert.equal(code, 2);
  assert.match(stderr, /ios-arm64/);
  assert.match(stderr, /embedpdf\/runtime does not build it yet/);
});

test('an unknown target exits 1', async () => {
  const { code, stderr } = await fetchTarget('solaris-sparc');
  assert.equal(code, 1);
  assert.match(stderr, /unknown target: solaris-sparc/);
});

test('darwin-arm64 extracts a dylib and an include tree', async () => {
  const { code, stdout } = await fetchTarget('darwin-arm64');
  assert.equal(code, 0);
  const dir = stdout.trim();
  await readFile(resolve(dir, 'lib/libembedpdf.dylib'));
  await readFile(resolve(dir, 'include/fpdfview.h'));
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && node --test build/fetch-libpdfium.test.mjs`
Expected: FAIL, all four tests error with `ENOENT` on `runtime-build.json` and `fetch-libpdfium.sh`.

- [ ] **Step 3: Create the pin file**

Create `rust-core-scaffold/build/runtime-build.json` by copying `packages/engine/runtime/engine-runtime-build.json` verbatim, then adding the four pending mobile entries inside `artifacts`. The nine published entries keep their exact `url` and `sha256` values; do not retype them, copy them.

```bash
cd /Users/andrew/repos/embed-pdf-viewer
node -e '
const fs = require("node:fs");
const pin = JSON.parse(fs.readFileSync("packages/engine/runtime/engine-runtime-build.json", "utf8"));
for (const target of ["ios-arm64", "ios-sim-arm64", "android-arm64", "android-x64"]) {
  pin.artifacts[target] = {
    status: "pending",
    note: "embedpdf/runtime does not build this target yet; see docs/research/pdfium-rust-wrapper-decision.md recommendation 6",
  };
}
fs.mkdirSync("rust-core-scaffold/build", { recursive: true });
fs.writeFileSync("rust-core-scaffold/build/runtime-build.json", JSON.stringify(pin, null, 2) + "\n");
'
```

- [ ] **Step 4: Create the fetch script**

Create `rust-core-scaffold/build/fetch-libpdfium.sh`. This is `packages/engine/runtime/scripts/fetch-libpdfium.sh` with the pending-target branch added and the pin path changed:

```bash
#!/usr/bin/env bash
# Fetch one prebuilt libembedpdf artifact from the fork's releases, verified by
# sha256. Adapted from packages/engine/runtime/scripts/fetch-libpdfium.sh; the
# only additions are the `status: pending` branch, which is how this scaffold
# reports the iOS and Android artifacts embedpdf/runtime does not build yet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"
PIN_FILE="${EPDF_SCAFFOLD_PIN_FILE:-$ROOT/build/runtime-build.json}"
OUT_DIR="${EPDF_SCAFFOLD_LIB_DIR:-$ROOT/build/libpdfium}"

if [[ -z "$TARGET" ]]; then
  echo "usage: $0 <target>" >&2
  exit 1
fi

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    echo "missing sha256 checksum tool: install shasum or sha256sum" >&2
    exit 1
  fi
}

STATUS="$(node -e "
const fs = require('node:fs');
const pin = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
const artifact = pin.artifacts && pin.artifacts[process.argv[2]];
if (!artifact) process.stdout.write('unknown');
else if (artifact.status === 'pending') process.stdout.write('pending');
else if (!artifact.url || !artifact.sha256) process.stdout.write('incomplete');
else process.stdout.write('ok');
" "$PIN_FILE" "$TARGET")"

case "$STATUS" in
  unknown)
    echo "unknown target: $TARGET" >&2
    exit 1
    ;;
  pending)
    cat >&2 <<MSG
no libembedpdf for $TARGET: embedpdf/runtime does not build it yet.

The fork's release matrix in .github/workflows/release-libpdfium.yml covers nine
targets and neither mobile one. Adding them is recommendation 6 of
docs/research/pdfium-rust-wrapper-decision.md and has its own design doc.

Until that lands, this target cannot be built. Everything that does not need it
still works: see rust-core-scaffold/README.md.
MSG
    exit 2
    ;;
  incomplete)
    echo "missing url or sha256 for target: $TARGET" >&2
    exit 1
    ;;
esac

URL="$(node -e "process.stdout.write(JSON.parse(require('node:fs').readFileSync(process.argv[1],'utf8')).artifacts[process.argv[2]].url)" "$PIN_FILE" "$TARGET")"
SHA256="$(node -e "process.stdout.write(JSON.parse(require('node:fs').readFileSync(process.argv[1],'utf8')).artifacts[process.argv[2]].sha256)" "$PIN_FILE" "$TARGET")"

DEST="$OUT_DIR/$TARGET"
CACHE="$ROOT/build/cache"
ARCHIVE="$CACHE/libembedpdf-runtime-$TARGET.tar.gz"

mkdir -p "$CACHE" "$DEST"

needs_refresh() {
  [[ ! -f "$ARCHIVE" ]] && return 0
  [[ "$(sha256_file "$ARCHIVE")" != "$SHA256" ]]
}

if needs_refresh; then
  echo "Downloading $TARGET from $URL" >&2
  if [[ "$URL" == file://* ]]; then
    cp "${URL#file://}" "$ARCHIVE"
  else
    curl -fL "$URL" -o "$ARCHIVE"
  fi
fi

ACTUAL_SHA="$(sha256_file "$ARCHIVE")"
if [[ "$ACTUAL_SHA" != "$SHA256" ]]; then
  echo "sha256 mismatch for $ARCHIVE" >&2
  echo "expected: $SHA256" >&2
  echo "actual:   $ACTUAL_SHA" >&2
  rm -f "$ARCHIVE"
  exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST"
tar -xzf "$ARCHIVE" -C "$DEST"

echo "$DEST"
```

Then `chmod +x rust-core-scaffold/build/fetch-libpdfium.sh`.

- [ ] **Step 5: Create the workspace, toolchain, and gitignore**

`rust-core-scaffold/Cargo.toml`:

```toml
[workspace]
resolver = "2"
# One line is added here by each crate task, so `cargo metadata` and
# `cargo test --workspace` succeed at every commit on this branch. The final
# shape is crates/{pdfium-sys, pdfium, test-support, epdf-ops, epdf-uniffi,
# epdf-node, epdf-wasm, uniffi-bindgen}.
members = []

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
# Deliberately NOT panic = "abort", unlike pdfium-rust-spike/Cargo.toml. uniffi
# and napi both turn a Rust panic into a host-language exception via
# catch_unwind, and panic = "abort" removes that path.
```

`rust-core-scaffold/rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.98.0"
targets = [
  "aarch64-apple-darwin",
  "wasm32-unknown-emscripten",
]
```

`rust-core-scaffold/.gitignore`:

```
build/cache/
build/libpdfium/
build/out/
target/
node_modules/
packages/swift/EpdfOps/Frameworks/
packages/swift/EpdfOps/Sources/EpdfOps/Generated/
packages/kotlin/epdf-ops/src/main/kotlin/generated/
packages/kotlin/epdf-ops/src/main/resources/
packages/node/*.node
packages/web/dist/
.build/
.gradle/
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd rust-core-scaffold && node --test build/fetch-libpdfium.test.mjs`
Expected: PASS, 4 tests. The `darwin-arm64` test downloads roughly 30 MB on first run.

- [ ] **Step 7: Verify the workspace parses**

Run: `cd rust-core-scaffold && cargo metadata --format-version 1 --no-deps`
Expected: success, with an empty `packages` array. Each later crate task adds its own `members` line, so this command works at every commit rather than only at the end.

- [ ] **Step 8: Commit**

```bash
git add rust-core-scaffold/Cargo.toml rust-core-scaffold/rust-toolchain.toml \
  rust-core-scaffold/.gitignore rust-core-scaffold/build/runtime-build.json \
  rust-core-scaffold/build/fetch-libpdfium.sh rust-core-scaffold/build/fetch-libpdfium.test.mjs
git commit -m "feat(scaffold): add cargo workspace and libembedpdf fetch gate"
```

---

### Task 2: Fixture and frozen parity vectors

**Files:**
- Create: `rust-core-scaffold/fixtures/report.pdf` (copy of `examples/snippet-react/public/report.pdf`)
- Create: `rust-core-scaffold/build/generate-vectors.mjs`
- Create: `rust-core-scaffold/vectors/report-page4.json` (generated, then committed and frozen)
- Create: `rust-core-scaffold/vectors/report-p4-scale0.25.bgra` (generated, then committed and frozen)
- Test: `rust-core-scaffold/vectors/vectors.test.mjs`

**Interfaces:**
- Consumes: `build/fetch-libpdfium.sh` from Task 1 is not needed here; this task drives the already-built native addon at `packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node`.
- Produces: `vectors/report-page4.json` with this exact shape, which every later task asserts against:

```jsonc
{
  "fixture": "fixtures/report.pdf",
  "forkSha": "d7b4caa26289d4846e94ff54e63b586b9bbe9fd6",
  "generatedBy": "build/generate-vectors.mjs",
  "pageIndex": 4,
  "pageCount": 0,               // real value written by the generator
  "size": { "width": 0, "height": 0 },   // points, f32, real values written by the generator
  "charCount": 8106,
  "text": "…",                  // full page-4 text, exact
  "search": {
    "query": "the",
    "caseSensitive": false,
    "hitCount": 0,              // real value
    "firstHits": [ { "charIndex": 0, "charCount": 3, "rects": [[0,0,0,0]] } ]
  },
  "render": {
    "scale": 0.25,
    "width": 0, "height": 0, "stride": 0,
    "golden": "report-p4-scale0.25.bgra",
    "meanAbsDiffTolerance": 2.0
  }
}
```

Page 4 and its 8,106 glyph count are the fixture facts `pdfium-rust-spike/bench/fixture.mjs:19-31` uses and `pdfium-rust-spike/bench/results/a-r1.json` records, so both are already known-good.

- [ ] **Step 1: Build the runtime the generator drives**

The generator imports the runtime's built Node entry. `packages/engine/runtime/dist/` does not exist in a fresh clone, but `packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node` and `libembedpdf.dylib` do.

Run:
```bash
cd /Users/andrew/repos/embed-pdf-viewer
pnpm --filter @embedpdf/engine-runtime build
ls packages/engine/runtime/dist/index.node.js
```
Expected: the file exists.

- [ ] **Step 2: Copy the fixture**

```bash
cd /Users/andrew/repos/embed-pdf-viewer
mkdir -p rust-core-scaffold/fixtures
cp examples/snippet-react/public/report.pdf rust-core-scaffold/fixtures/report.pdf
shasum -a 256 rust-core-scaffold/fixtures/report.pdf
```
Expected: a 1,016,315 byte file. Record the printed sha256; Step 4 puts it in the vectors file.

- [ ] **Step 3: Write the failing test**

Create `rust-core-scaffold/vectors/vectors.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

test('vectors file is internally consistent and frozen', async () => {
  const v = JSON.parse(await readFile(resolve(here, 'report-page4.json'), 'utf8'));

  assert.equal(v.fixture, 'fixtures/report.pdf');
  assert.equal(v.forkSha, 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6');
  assert.equal(v.pageIndex, 4);
  assert.equal(v.charCount, 8106);
  assert.equal(v.text.length, v.charCount);
  assert.ok(v.pageCount > v.pageIndex, 'page count must include page 4');
  assert.ok(v.size.width > 0 && v.size.height > 0);
  assert.ok(v.search.hitCount >= v.search.firstHits.length);

  const golden = resolve(here, v.render.golden);
  const { size } = await stat(golden);
  assert.equal(size, v.render.stride * v.render.height);
  assert.equal(v.render.stride, v.render.width * 4);
});
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cd rust-core-scaffold && node --test vectors/vectors.test.mjs`
Expected: FAIL with `ENOENT` on `report-page4.json`.

- [ ] **Step 5: Write the generator**

Create `rust-core-scaffold/build/generate-vectors.mjs`. It talks to the runtime's raw `PdfFunctions` surface and its `mem` helpers, exactly as `pdfium-rust-spike/bench/fixture.mjs` does. Write the file with this content:

```js
/**
 * Writes vectors/report-page4.json and vectors/report-p4-scale0.25.bgra ONCE,
 * from the shipping TypeScript path, and is then never run again by a test.
 * That is the whole point: a Rust, Swift, Kotlin or JS host that drifts from
 * these numbers fails a test instead of being discovered in a browser. Same
 * mechanism as ts-wasm-interop-poc/vectors/anchor-vectors.json.
 *
 * Requires `pnpm --filter @embedpdf/engine-runtime build` first.
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createPdfRuntime, NULL_PTR } from '../../packages/engine/runtime/dist/index.node.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const PAGE_INDEX = 4;
const SCALE = 0.25;
const QUERY = 'the';

const pdfBytes = readFileSync(resolve(root, 'fixtures/report.pdf'));
const rt = await createPdfRuntime({ prefer: 'native' });
const fn = rt.pdf;

fn.FPDF_InitLibrary();

const buf = rt.mem.malloc(pdfBytes.length);
rt.mem.write(buf, pdfBytes);
const doc = fn.FPDF_LoadMemDocument(buf, pdfBytes.length, NULL_PTR);
if (!doc) throw new Error('FPDF_LoadMemDocument failed');

const pageCount = fn.FPDF_GetPageCount(doc);
const page = fn.FPDF_LoadPage(doc, PAGE_INDEX);
if (!page) throw new Error(`FPDF_LoadPage failed for index ${PAGE_INDEX}`);

const size = {
  width: fn.FPDF_GetPageWidthF(page),
  height: fn.FPDF_GetPageHeightF(page),
};

const textPage = fn.FPDFText_LoadPage(page);
if (!textPage) throw new Error('FPDFText_LoadPage failed');
const charCount = fn.FPDFText_CountChars(textPage);

// EPDFText_GetTextFull writes UTF-16LE and returns the code-unit count,
// including the terminator. Two calls: size, then fill.
const needed = fn.EPDFText_GetTextFull(textPage, NULL_PTR, 0);
const textBuf = rt.mem.malloc(needed * 2);
fn.EPDFText_GetTextFull(textPage, textBuf, needed);
const text = rt.mem.readUtf16(textBuf, needed - 1);
rt.mem.free(textBuf);

const search = (() => {
  const queryBuf = rt.mem.mallocUtf16(QUERY);
  const handle = fn.FPDFText_FindStart(textPage, queryBuf, 0, 0);
  const firstHits = [];
  let hitCount = 0;
  while (fn.FPDFText_FindNext(handle)) {
    const charIndex = fn.FPDFText_GetSchResultIndex(handle);
    const count = fn.FPDFText_GetSchCount(handle);
    if (firstHits.length < 3) {
      const rectCount = fn.FPDFText_CountRects(textPage, charIndex, count);
      const rects = [];
      for (let i = 0; i < rectCount; i += 1) {
        const out = rt.mem.malloc(8 * 4);
        fn.FPDFText_GetRect(textPage, i, out, out + 8, out + 16, out + 24);
        rects.push([
          rt.mem.peekF64(out), rt.mem.peekF64(out + 8),
          rt.mem.peekF64(out + 16), rt.mem.peekF64(out + 24),
        ].map((n) => Number(n.toFixed(4))));
        rt.mem.free(out);
      }
      firstHits.push({ charIndex, charCount: count, rects });
    }
    hitCount += 1;
  }
  fn.FPDFText_FindClose(handle);
  rt.mem.free(queryBuf);
  return { query: QUERY, caseSensitive: false, hitCount, firstHits };
})();

const width = Math.max(1, Math.round(size.width * SCALE));
const height = Math.max(1, Math.round(size.height * SCALE));
const stride = width * 4;
const pixels = rt.mem.malloc(stride * height);
const bitmap = fn.FPDFBitmap_CreateEx(width, height, 4 /* FPDFBitmap_BGRA */, pixels, stride);
fn.FPDFBitmap_FillRect(bitmap, 0, 0, width, height, 0xffffffff);
fn.FPDF_RenderPageBitmap(bitmap, page, 0, 0, width, height, 0, 0);
const golden = Buffer.from(rt.mem.read(pixels, stride * height));
fn.FPDFBitmap_Destroy(bitmap);
rt.mem.free(pixels);

fn.FPDFText_ClosePage(textPage);
fn.FPDF_ClosePage(page);
fn.FPDF_CloseDocument(doc);
rt.mem.free(buf);
fn.FPDF_DestroyLibrary();

mkdirSync(resolve(root, 'vectors'), { recursive: true });
writeFileSync(resolve(root, 'vectors/report-p4-scale0.25.bgra'), golden);
writeFileSync(
  resolve(root, 'vectors/report-page4.json'),
  JSON.stringify(
    {
      fixture: 'fixtures/report.pdf',
      fixtureSha256: createHash('sha256').update(pdfBytes).digest('hex'),
      forkSha: 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6',
      generatedBy: 'build/generate-vectors.mjs',
      pageIndex: PAGE_INDEX,
      pageCount,
      size: { width: Number(size.width.toFixed(4)), height: Number(size.height.toFixed(4)) },
      charCount,
      text,
      search,
      render: {
        scale: SCALE,
        width,
        height,
        stride,
        golden: 'report-p4-scale0.25.bgra',
        meanAbsDiffTolerance: 2.0,
      },
    },
    null,
    2,
  ) + '\n',
);

console.error(`wrote vectors: ${pageCount} pages, ${charCount} chars, ${search.hitCount} hits, ${width}x${height} render`);
```

The `rt.mem` method names above (`malloc`, `free`, `read`, `write`, `readUtf16`, `mallocUtf16`, `peekF64`) are the ones this generator needs. Before running it, read `packages/engine/runtime/src/core/pdf-runtime-module.ts` and confirm the real names on `PdfRuntimeMemory`, then correct the calls to match. Do not invent a helper: if one does not exist, build it from the primitives that do.

- [ ] **Step 6: Generate the vectors**

Run:
```bash
cd rust-core-scaffold && node build/generate-vectors.mjs
```
Expected on stderr: `wrote vectors: <pageCount> pages, 8106 chars, <n> hits, <w>x<h> render`.

The glyph count must be exactly 8106. That is the one number here with a committed receipt behind it, from `pdfium-rust-spike/bench/fixture.mjs:3-4` and `pdfium-rust-spike/bench/results/a-r1.json`. Anything else means the fixture or the page index is wrong, and you should stop rather than commit a different number. The page count, page size, hit count and render dimensions have no prior receipt: whatever the generator prints is the truth, and it becomes the frozen value.

- [ ] **Step 7: Run the test to verify it passes**

Run: `cd rust-core-scaffold && node --test vectors/vectors.test.mjs`
Expected: PASS, 1 test.

- [ ] **Step 8: Commit**

```bash
git add rust-core-scaffold/fixtures/report.pdf rust-core-scaffold/build/generate-vectors.mjs \
  rust-core-scaffold/vectors/report-page4.json rust-core-scaffold/vectors/report-p4-scale0.25.bgra \
  rust-core-scaffold/vectors/vectors.test.mjs
git commit -m "feat(scaffold): freeze parity vectors for report.pdf page 4"
```

---

### Task 3: `pdfium-sys`, bindgen over the fork headers

**Files:**
- Create: `rust-core-scaffold/crates/pdfium-sys/Cargo.toml`
- Create: `rust-core-scaffold/crates/pdfium-sys/build.rs`
- Create: `rust-core-scaffold/crates/pdfium-sys/wrapper.h`
- Create: `rust-core-scaffold/crates/pdfium-sys/src/lib.rs`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/pdfium-sys"` to `members`)
- Test: `rust-core-scaffold/crates/pdfium-sys/tests/smoke.rs`

**Interfaces:**
- Consumes: `build/fetch-libpdfium.sh <target>` from Task 1, which populates `build/libpdfium/<target>/{include,lib}`.
- Produces: `pdfium_sys::*` with every `FPDF_*`, `EPDF_*`, `FORM_*` and `PDFiumExt_*` function and type, generated at build time. Also `pdfium_sys::lib_dir_for_host() -> PathBuf` is NOT produced; the directory logic lives only in `build.rs`.

- [ ] **Step 1: Write the failing test**

Create `rust-core-scaffold/crates/pdfium-sys/tests/smoke.rs`:

```rust
//! Proves the generated bindings link and that one FPDF and one EPDF symbol are
//! both reachable. The EPDF one matters: it is a fork extension, so its presence
//! is what distinguishes our libembedpdf from vanilla PDFium.

use std::ffi::c_void;
use std::ptr;

fn fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/report.pdf");
    std::fs::read(path).expect("fixtures/report.pdf; run Task 2 first")
}

#[test]
fn loads_the_fixture_and_reads_a_page_size_through_an_epdf_extension() {
    let bytes = fixture();
    unsafe {
        pdfium_sys::FPDF_InitLibrary();

        let doc = pdfium_sys::FPDF_LoadMemDocument(
            bytes.as_ptr() as *const c_void,
            bytes.len() as i32,
            ptr::null(),
        );
        assert!(!doc.is_null(), "FPDF_LoadMemDocument returned null");

        let page_count = pdfium_sys::FPDF_GetPageCount(doc);
        assert!(page_count > 4, "fixture must have a page 4, got {page_count}");

        let mut size = pdfium_sys::FS_SIZEF { width: 0.0, height: 0.0 };
        let ok = pdfium_sys::EPDF_GetPageSizeByIndexNormalized(doc, 4, &mut size);
        assert_ne!(ok, 0, "EPDF_GetPageSizeByIndexNormalized failed");
        assert!(size.width > 0.0 && size.height > 0.0, "got {size:?}");

        pdfium_sys::FPDF_CloseDocument(doc);
        pdfium_sys::FPDF_DestroyLibrary();
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cd rust-core-scaffold
bash build/fetch-libpdfium.sh darwin-arm64
cargo test -p pdfium-sys
```
Expected: FAIL with `error: failed to read .../crates/pdfium-sys/Cargo.toml`.

- [ ] **Step 3: Write the manifest and the bindgen header**

Add `"crates/pdfium-sys"` to the `members` array in `rust-core-scaffold/Cargo.toml`, then write `rust-core-scaffold/crates/pdfium-sys/Cargo.toml`:

```toml
[package]
name = "pdfium-sys"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
crate-type = ["rlib"]

[build-dependencies]
bindgen = "=0.72.1"
```

`rust-core-scaffold/crates/pdfium-sys/wrapper.h`:

```c
/* Every public header the fork ships, which is what
 * packages/engine/runtime/build/generate-functions.mjs walks too. The `cpp/`
 * subdirectory is excluded there (isCppHelperHeader) and here, because it is
 * C++ helpers rather than the C ABI. */
#include "fpdfview.h"
#include "fpdf_annot.h"
#include "fpdf_attachment.h"
#include "fpdf_catalog.h"
#include "fpdf_dataavail.h"
#include "fpdf_doc.h"
#include "fpdf_edit.h"
#include "fpdf_ext.h"
#include "fpdf_flatten.h"
#include "fpdf_formfill.h"
#include "fpdf_fwlevent.h"
#include "fpdf_javascript.h"
#include "fpdf_jpeg.h"
#include "fpdf_png.h"
#include "fpdf_ppo.h"
#include "fpdf_progressive.h"
#include "fpdf_save.h"
#include "fpdf_searchex.h"
#include "fpdf_signature.h"
#include "fpdf_structtree.h"
#include "fpdf_sysfontinfo.h"
#include "fpdf_text.h"
#include "fpdf_thumbnail.h"
#include "epdf_action.h"
#include "epdf_font.h"
#include "epdf_form.h"
#include "epdf_pieceinfo.h"
#include "epdf_redact.h"
#include "epdf_text.h"
```

Before committing, run `ls rust-core-scaffold/build/libpdfium/darwin-arm64/include/*.h` and reconcile: any header present there and missing above gets added, any listed here and absent gets removed. The include list must match the tarball, not this plan.

- [ ] **Step 4: Write build.rs**

```rust
use std::env;
use std::path::PathBuf;

fn main() {
    let lib_root = lib_root();
    let target = fork_target();
    let dir = lib_root.join(&target);
    let include = dir.join("include");

    if !include.is_dir() {
        panic!(
            "no libembedpdf headers for {target} at {}.\n\
             Fix with: bash build/fetch-libpdfium.sh {target}",
            include.display()
        );
    }

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_LIB_DIR");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include.display()))
        // The same filter packages/engine/runtime/build/generate-functions.mjs
        // applies: /^(?:FPDF|EPDF|FORM|PDFiumExt_)/.
        .allowlist_function("^(FPDF|EPDF|FORM|PDFiumExt_).*")
        .allowlist_type("^(FPDF|EPDF|FORM|FS_|FX_|PDFiumExt_|IPDF_|fpdf_).*")
        .allowlist_var("^(FPDF|EPDF|FORM|FPDFBitmap_|FPDF_ANNOT|FWL_).*")
        .derive_debug(true)
        .derive_default(true)
        .derive_copy(true)
        .layout_tests(false)
        .generate()
        .expect("bindgen failed over the fork headers");

    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("write bindings.rs");

    emit_link_directives(&dir);
}

/// wasm32-unknown-emscripten links nothing here: build/link-wasm.sh puts
/// libembedpdf.a on the em++ command line alongside the Rust staticlib, exactly
/// as pdfium-rust-spike/build/link-wasm.sh does.
fn emit_link_directives(dir: &PathBuf) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }

    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());

    // The fork ships a dylib where build-target.sh sets pdf_is_complete_lib=false
    // (darwin, linux-gnu, win32) and a static archive where it sets true (wasm32,
    // linuxmusl). iOS will be static; Android will be a .so.
    if lib.join("libembedpdf.dylib").exists() || lib.join("libembedpdf.so").exists() {
        println!("cargo:rustc-link-lib=dylib=embedpdf");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    } else {
        println!("cargo:rustc-link-lib=static=embedpdf");
        println!("cargo:rustc-link-lib=dylib=c++");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for framework in ["AppKit", "CoreFoundation", "CoreGraphics", "Foundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}

fn lib_root() -> PathBuf {
    if let Ok(dir) = env::var("EPDF_SCAFFOLD_LIB_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .join("build/libpdfium")
}

/// The fork's target spelling, which is Node's rather than Rust's. Copied from
/// pdfium-rust-spike/crates/napi-arms/build.rs:56-78 and extended with the two
/// mobile OSes.
fn fork_target() -> String {
    if let Ok(target) = env::var("EPDF_SCAFFOLD_TARGET") {
        return target;
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    // aarch64-apple-ios-sim reports target_abi = "sim" with an EMPTY target_env,
    // so the simulator cannot be detected from target_env.
    let target_abi = env::var("CARGO_CFG_TARGET_ABI").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if os == "emscripten" {
        return "wasm32".to_string();
    }

    let platform = match os.as_str() {
        "macos" => "darwin",
        "ios" if target_abi == "sim" => "ios-sim",
        "ios" => "ios",
        "android" => "android",
        "linux" if target_env == "musl" => "linuxmusl",
        "linux" => "linux",
        "windows" => "win32",
        _ => panic!("unsupported target_os `{os}`; set EPDF_SCAFFOLD_TARGET"),
    };
    let arch = match arch.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => panic!("unsupported target_arch `{arch}`; set EPDF_SCAFFOLD_TARGET"),
    };
    format!("{platform}-{arch}")
}
```

- [ ] **Step 5: Write src/lib.rs**

```rust
//! Raw bindings to the EmbedPDF PDFium fork's C ABI, generated by bindgen over
//! the same headers packages/engine/runtime/build/generate-functions.mjs walks,
//! with the same FPDF|EPDF|FORM|PDFiumExt_ filter.
//!
//! This crate is INTERNAL to the scaffold. Nothing reaches it from Swift,
//! Kotlin, Node or the browser: recommendation 3 of
//! docs/research/pdfium-rust-wrapper-decision.md prices a per-call host boundary
//! at 2.6x (wasm) to 7.16x (native) against a per-operation one, so exposing
//! this surface would be building the expensive thing on purpose.
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd rust-core-scaffold && cargo test -p pdfium-sys`
Expected: PASS, `test loads_the_fixture_and_reads_a_page_size_through_an_epdf_extension ... ok`.

If bindgen fails on a missing system header, add `.clang_arg(format!("-isysroot{}", …))` using the output of `xcrun --show-sdk-path`. Do not work around it by deleting headers from `wrapper.h`.

- [ ] **Step 7: Commit**

```bash
git add rust-core-scaffold/crates/pdfium-sys
git commit -m "feat(scaffold): generate pdfium-sys bindings over the fork headers"
```

---

### Task 4: `pdfium`, the safe RAII wrapper

**Files:**
- Create: `rust-core-scaffold/crates/test-support/Cargo.toml`
- Create: `rust-core-scaffold/crates/test-support/src/lib.rs`
- Create: `rust-core-scaffold/crates/pdfium/Cargo.toml`
- Create: `rust-core-scaffold/crates/pdfium/src/lib.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/library.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/document.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/page.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/text.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/bitmap.rs`
- Create: `rust-core-scaffold/crates/pdfium/src/error.rs`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/test-support"` and `"crates/pdfium"` to `members`)
- Test: `rust-core-scaffold/crates/pdfium/tests/fixture.rs`

**Interfaces:**
- Consumes: `pdfium_sys::*` from Task 3.
- Produces, from `test-support`, the one loader every Rust suite in this plan uses:

```rust
pub struct Vectors {
    pub page_index: u32,
    pub page_count: u32,
    pub width: f32,
    pub height: f32,
    pub char_count: u32,
    pub text: String,
    pub search_query: String,
    pub search_case_sensitive: bool,
    pub search_hit_count: usize,
    pub search_first_hits: Vec<(u32, u32)>,   // (char_index, char_count)
    pub render_scale: f32,
    pub render_width: u32,
    pub render_height: u32,
    pub render_stride: u32,
    pub golden: Vec<u8>,
    pub tolerance: f64,
}

pub fn scaffold_root() -> std::path::PathBuf;
pub fn vectors() -> Vectors;
pub fn fixture_bytes() -> Vec<u8>;
pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64;
```

- Produces, from `pdfium`:

```rust
pub enum PdfiumError { Init, Load, Password, PageOutOfRange(u32), Text, Bitmap, Search }
pub struct Library;            // thread-confined init guard, refcounted per thread
impl Library { pub fn acquire() -> Result<Library, PdfiumError>; }
pub struct Document { /* !Send, !Sync */ }
impl Document {
    pub fn from_bytes(lib: &Library, bytes: &[u8], password: Option<&str>) -> Result<Document, PdfiumError>;
    pub fn page_count(&self) -> u32;
    pub fn page(&self, index: u32) -> Result<Page<'_>, PdfiumError>;
}
pub struct Page<'a>;
impl Page<'_> {
    pub fn width(&self) -> f32;
    pub fn height(&self) -> f32;
    pub fn text(&self) -> Result<TextPage<'_>, PdfiumError>;
    pub fn render_bgra(&self, width: u32, height: u32) -> Result<Bitmap, PdfiumError>;
}
pub struct TextPage<'a>;
impl TextPage<'_> {
    pub fn char_count(&self) -> u32;
    pub fn text(&self) -> Result<String, PdfiumError>;
    pub fn rects(&self, char_index: u32, char_count: u32) -> Result<Vec<[f64; 4]>, PdfiumError>;
    pub fn find(&self, query: &str, case_sensitive: bool) -> Result<Vec<(u32, u32)>, PdfiumError>;
}
pub struct Bitmap { pub width: u32, pub height: u32, pub stride: u32, pub bgra: Vec<u8> }
```

`Document`, `Page` and `TextPage` are deliberately neither `Send` nor `Sync`. They hold raw pointers, so they are `!Send` by default and nothing in this crate opts them in. That is the type system carrying the thread-confinement constraint from `runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77`, and Task 5 is what makes it usable from other threads.

- [ ] **Step 1: Write the failing test**

First write the shared loader, `rust-core-scaffold/crates/test-support/src/lib.rs`, because every Rust suite in Tasks 4, 5 and 6 reads the same frozen file and none of them may re-declare this:

```rust
//! The one reader of vectors/report-page4.json, which build/generate-vectors.mjs
//! wrote once from the shipping TypeScript path and which nothing regenerates.
//! A dev-dependency only: no production crate depends on this.
use std::path::{Path, PathBuf};

pub struct Vectors {
    pub page_index: u32,
    pub page_count: u32,
    pub width: f32,
    pub height: f32,
    pub char_count: u32,
    pub text: String,
    pub search_query: String,
    pub search_case_sensitive: bool,
    pub search_hit_count: usize,
    pub search_first_hits: Vec<(u32, u32)>,
    pub render_scale: f32,
    pub render_width: u32,
    pub render_height: u32,
    pub render_stride: u32,
    pub golden: Vec<u8>,
    pub tolerance: f64,
}

/// The scaffold root, from this crate's own manifest directory.
pub fn scaffold_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture_bytes() -> Vec<u8> {
    let path = scaffold_root().join("fixtures/report.pdf");
    std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

pub fn vectors() -> Vectors {
    let root = scaffold_root();
    let raw = std::fs::read_to_string(root.join("vectors/report-page4.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let size = &v["size"];
    let search = &v["search"];
    let render = &v["render"];

    Vectors {
        page_index: v["pageIndex"].as_u64().unwrap() as u32,
        page_count: v["pageCount"].as_u64().unwrap() as u32,
        width: size["width"].as_f64().unwrap() as f32,
        height: size["height"].as_f64().unwrap() as f32,
        char_count: v["charCount"].as_u64().unwrap() as u32,
        text: v["text"].as_str().unwrap().to_string(),
        search_query: search["query"].as_str().unwrap().to_string(),
        search_case_sensitive: search["caseSensitive"].as_bool().unwrap(),
        search_hit_count: search["hitCount"].as_u64().unwrap() as usize,
        search_first_hits: search["firstHits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| {
                (
                    hit["charIndex"].as_u64().unwrap() as u32,
                    hit["charCount"].as_u64().unwrap() as u32,
                )
            })
            .collect(),
        render_scale: render["scale"].as_f64().unwrap() as f32,
        render_width: render["width"].as_u64().unwrap() as u32,
        render_height: render["height"].as_u64().unwrap() as u32,
        render_stride: render["stride"].as_u64().unwrap() as u32,
        golden: std::fs::read(root.join("vectors").join(render["golden"].as_str().unwrap())).unwrap(),
        tolerance: render["meanAbsDiffTolerance"].as_f64().unwrap(),
    }
}

/// Mean absolute per-byte difference. Rendering is compared this way rather
/// than by hash because a hash pins float and rasterization behaviour across
/// four architectures and fails for reasons unrelated to this code.
pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "buffer lengths differ: {} vs {}", a.len(), b.len());
    let total: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    total as f64 / a.len() as f64
}
```

`rust-core-scaffold/crates/test-support/Cargo.toml`:

```toml
[package]
name = "test-support"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
crate-type = ["rlib"]

[dependencies]
serde_json = "1"
```

Add both `"crates/test-support"` and `"crates/pdfium"` to the `members` array in `rust-core-scaffold/Cargo.toml`.

Then create `rust-core-scaffold/crates/pdfium/tests/fixture.rs`:

```rust
//! Every expected value comes from vectors/report-page4.json through
//! test_support, the one loader in this workspace.

use test_support::{fixture_bytes, mean_abs_diff, vectors};

#[test]
fn matches_the_frozen_vectors() {
    let v = vectors();
    let bytes = fixture_bytes();

    let lib = pdfium::Library::acquire().unwrap();
    let doc = pdfium::Document::from_bytes(&lib, &bytes, None).unwrap();
    assert_eq!(doc.page_count(), v.page_count);

    let page = doc.page(v.page_index).unwrap();
    assert!((page.width() - v.width).abs() < 0.001, "{} vs {}", page.width(), v.width);
    assert!((page.height() - v.height).abs() < 0.001);

    let text_page = page.text().unwrap();
    assert_eq!(text_page.char_count(), v.char_count);
    assert_eq!(text_page.text().unwrap(), v.text);

    let hits = text_page.find("the", false).unwrap();
    assert!(!hits.is_empty(), "the fixture contains the query");

    let bitmap = page.render_bgra(v.render_width, v.render_height).unwrap();
    assert_eq!(bitmap.width, v.render_width);
    assert_eq!(bitmap.height, v.render_height);
    assert_eq!(bitmap.stride, v.render_stride);
    let diff = mean_abs_diff(&bitmap.bgra, &v.golden);
    assert!(diff <= v.tolerance, "mean abs diff {diff} exceeds {}", v.tolerance);
}

#[test]
fn rejects_a_page_past_the_end() {
    let bytes = fixture_bytes();
    let lib = pdfium::Library::acquire().unwrap();
    let doc = pdfium::Document::from_bytes(&lib, &bytes, None).unwrap();
    let index = doc.page_count();
    assert!(matches!(
        doc.page(index),
        Err(pdfium::PdfiumError::PageOutOfRange(i)) if i == index
    ));
}

#[test]
fn rejects_bytes_that_are_not_a_pdf() {
    let lib = pdfium::Library::acquire().unwrap();
    let err = pdfium::Document::from_bytes(&lib, b"not a pdf at all", None).unwrap_err();
    assert!(matches!(err, pdfium::PdfiumError::Load));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && cargo test -p pdfium`
Expected: FAIL, `failed to read .../crates/pdfium/Cargo.toml`.

- [ ] **Step 3: Write the manifest**

```toml
[package]
name = "pdfium"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
crate-type = ["rlib"]

[dependencies]
pdfium-sys = { path = "../pdfium-sys" }

[dev-dependencies]
test-support = { path = "../test-support" }
```

- [ ] **Step 4: Write the library guard**

`src/error.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfiumError {
    Init,
    Load,
    Password,
    PageOutOfRange(u32),
    Text,
    Bitmap,
    Search,
}

impl std::fmt::Display for PdfiumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init => write!(f, "FPDF_InitLibrary failed"),
            Self::Load => write!(f, "the bytes are not a document PDFium can open"),
            Self::Password => write!(f, "the document is encrypted and the password was wrong or missing"),
            Self::PageOutOfRange(i) => write!(f, "page index {i} is past the end of the document"),
            Self::Text => write!(f, "FPDFText_LoadPage failed"),
            Self::Bitmap => write!(f, "FPDFBitmap_CreateEx failed"),
            Self::Search => write!(f, "FPDFText_FindStart failed"),
        }
    }
}

impl std::error::Error for PdfiumError {}
```

`src/library.rs`:

```rust
use std::cell::Cell;

use crate::PdfiumError;

thread_local! {
    /// PDFium's globals are per-thread in every non-wasm fork build, because
    /// runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77 sets
    /// embedpdf_thread_local_globals=true for every target except wasm32. So
    /// this refcount is per-thread too: FPDF_InitLibrary has to run on each
    /// thread that touches PDFium, and FPDF_DestroyLibrary on that same thread.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// A per-thread init guard. Not `Send`: dropping it on another thread would
/// call FPDF_DestroyLibrary against globals that thread never initialized.
pub struct Library {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Library {
    pub fn acquire() -> Result<Self, PdfiumError> {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                unsafe { pdfium_sys::FPDF_InitLibrary() };
            }
            depth.set(depth.get() + 1);
        });
        Ok(Self { _not_send: std::marker::PhantomData })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let next = depth.get() - 1;
            depth.set(next);
            if next == 0 {
                unsafe { pdfium_sys::FPDF_DestroyLibrary() };
            }
        });
    }
}
```

- [ ] **Step 5: Write document, page, text and bitmap**

`src/document.rs`:

```rust
use std::ffi::{c_void, CString};
use std::marker::PhantomData;

use crate::{Library, Page, PdfiumError};

pub struct Document {
    handle: pdfium_sys::FPDF_DOCUMENT,
    /// FPDF_LoadMemDocument does NOT copy: PDFium reads from this buffer for the
    /// document's whole life, so the Document owns it. Field order matters,
    /// because fields drop after the Drop impl body runs, which is what puts
    /// FPDF_CloseDocument ahead of freeing these bytes.
    bytes: Vec<u8>,
    _not_send: PhantomData<*const ()>,
}

impl Document {
    pub fn from_bytes(_lib: &Library, bytes: &[u8], password: Option<&str>) -> Result<Self, PdfiumError> {
        // One copy, owned by the Document. Moving the Document moves the Vec's
        // three words, never the heap allocation, so the pointer PDFium holds
        // stays valid. A borrowed &[u8] would need a lifetime on Document,
        // which makes it unusable from the worker-thread slab in epdf-ops.
        let bytes = bytes.to_vec();

        let c_password = password.map(|p| CString::new(p).map_err(|_| PdfiumError::Password)).transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let handle = unsafe {
            pdfium_sys::FPDF_LoadMemDocument(
                bytes.as_ptr() as *const c_void,
                bytes.len() as i32,
                password_ptr,
            )
        };

        if handle.is_null() {
            // FPDF_ERR_PASSWORD, from fpdfview.h.
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == 4 { PdfiumError::Password } else { PdfiumError::Load });
        }

        Ok(Self { handle, bytes, _not_send: PhantomData })
    }

    pub fn page_count(&self) -> u32 {
        let count = unsafe { pdfium_sys::FPDF_GetPageCount(self.handle) };
        count.max(0) as u32
    }

    pub fn page(&self, index: u32) -> Result<Page<'_>, PdfiumError> {
        if index >= self.page_count() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Page::load(self.handle, index)
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // Closes first; `self.bytes` is freed afterwards, when the fields drop.
        unsafe { pdfium_sys::FPDF_CloseDocument(self.handle) };
    }
}
```

`src/bitmap.rs`:

```rust
/// BGRA, `stride * height` bytes, `stride == width * 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
}
```

`src/page.rs`:

```rust
use std::ffi::c_void;
use std::marker::PhantomData;

use crate::{Bitmap, PdfiumError, TextPage};

pub struct Page<'a> {
    handle: pdfium_sys::FPDF_PAGE,
    _doc: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

impl<'a> Page<'a> {
    pub(crate) fn load(doc: pdfium_sys::FPDF_DOCUMENT, index: u32) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDF_LoadPage(doc, index as i32) };
        if handle.is_null() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Ok(Self { handle, _doc: PhantomData, _not_send: PhantomData })
    }

    pub fn width(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageWidthF(self.handle) }
    }

    pub fn height(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageHeightF(self.handle) }
    }

    pub fn text(&self) -> Result<TextPage<'_>, PdfiumError> {
        TextPage::load(self.handle)
    }

    /// The same five calls, in the same order, as build/generate-vectors.mjs in
    /// Task 2 and pdfium-rust-spike/crates/ops/src/lib.rs. Changing the order or
    /// the fill colour changes the pixels and breaks the golden comparison in
    /// every host.
    pub fn render_bgra(&self, width: u32, height: u32) -> Result<Bitmap, PdfiumError> {
        let stride = width * 4;
        let mut bgra = vec![0u8; (stride as usize) * (height as usize)];

        let bitmap = unsafe {
            pdfium_sys::FPDFBitmap_CreateEx(
                width as i32,
                height as i32,
                pdfium_sys::FPDFBitmap_BGRA as i32,
                bgra.as_mut_ptr() as *mut c_void,
                stride as i32,
            )
        };
        if bitmap.is_null() {
            return Err(PdfiumError::Bitmap);
        }

        unsafe {
            // White ground, fully opaque, so a page with no background matches
            // what a viewer shows.
            pdfium_sys::FPDFBitmap_FillRect(bitmap, 0, 0, width as i32, height as i32, 0xFFFF_FFFF);
            pdfium_sys::FPDF_RenderPageBitmap(
                bitmap,
                self.handle,
                0,
                0,
                width as i32,
                height as i32,
                0,
                0,
            );
            pdfium_sys::FPDFBitmap_Destroy(bitmap);
        }

        Ok(Bitmap { width, height, stride, bgra })
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDF_ClosePage(self.handle) };
    }
}
```

`src/text.rs`:

```rust
use std::marker::PhantomData;

use crate::PdfiumError;

/// fpdf_text.h's flag for a case-sensitive search.
const FPDF_MATCHCASE: u64 = 1;

pub struct TextPage<'a> {
    handle: pdfium_sys::FPDF_TEXTPAGE,
    _page: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

impl<'a> TextPage<'a> {
    pub(crate) fn load(page: pdfium_sys::FPDF_PAGE) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDFText_LoadPage(page) };
        if handle.is_null() {
            return Err(PdfiumError::Text);
        }
        Ok(Self { handle, _page: PhantomData, _not_send: PhantomData })
    }

    pub fn char_count(&self) -> u32 {
        unsafe { pdfium_sys::FPDFText_CountChars(self.handle) }.max(0) as u32
    }

    /// EPDFText_GetTextFull is a fork extension: one call for the whole page,
    /// UTF-16, returning the code-unit count including the NUL terminator.
    pub fn text(&self) -> Result<String, PdfiumError> {
        let needed = unsafe { pdfium_sys::EPDFText_GetTextFull(self.handle, std::ptr::null_mut(), 0) };
        if needed <= 1 {
            return Ok(String::new());
        }

        let mut buf = vec![0u16; needed as usize];
        let written = unsafe {
            pdfium_sys::EPDFText_GetTextFull(self.handle, buf.as_mut_ptr(), needed)
        };
        if written <= 0 {
            return Err(PdfiumError::Text);
        }

        buf.truncate((written as usize).saturating_sub(1)); // drop the terminator
        String::from_utf16(&buf).map_err(|_| PdfiumError::Text)
    }

    /// FPDFText_CountRects has to run before FPDFText_GetRect: it is what
    /// populates the rect cache GetRect then indexes into.
    pub fn rects(&self, char_index: u32, char_count: u32) -> Result<Vec<[f64; 4]>, PdfiumError> {
        let count = unsafe {
            pdfium_sys::FPDFText_CountRects(self.handle, char_index as i32, char_count as i32)
        }
        .max(0);

        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut left = 0.0f64;
            let mut top = 0.0f64;
            let mut right = 0.0f64;
            let mut bottom = 0.0f64;
            let ok = unsafe {
                pdfium_sys::FPDFText_GetRect(
                    self.handle,
                    i,
                    &mut left,
                    &mut top,
                    &mut right,
                    &mut bottom,
                )
            };
            if ok == 0 {
                return Err(PdfiumError::Text);
            }
            out.push([left, top, right, bottom]);
        }
        Ok(out)
    }

    /// Returns `(char_index, char_count)` per hit, in document order.
    pub fn find(&self, query: &str, case_sensitive: bool) -> Result<Vec<(u32, u32)>, PdfiumError> {
        let needle: Vec<u16> = query.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if case_sensitive { FPDF_MATCHCASE } else { 0 };

        let search = unsafe {
            pdfium_sys::FPDFText_FindStart(self.handle, needle.as_ptr(), flags as _, 0)
        };
        if search.is_null() {
            return Err(PdfiumError::Search);
        }

        let mut hits = Vec::new();
        unsafe {
            while pdfium_sys::FPDFText_FindNext(search) != 0 {
                let index = pdfium_sys::FPDFText_GetSchResultIndex(search).max(0) as u32;
                let count = pdfium_sys::FPDFText_GetSchCount(search).max(0) as u32;
                hits.push((index, count));
            }
            pdfium_sys::FPDFText_FindClose(search);
        }
        Ok(hits)
    }
}

impl Drop for TextPage<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDFText_ClosePage(self.handle) };
    }
}
```

bindgen's generated integer widths for the flag and count arguments depend on the headers, so if `FPDFText_FindStart`'s third parameter is `u32` rather than `u64`, change the `FPDF_MATCHCASE` constant's type rather than adding a cast at the call site.

- [ ] **Step 6: Write src/lib.rs**

```rust
//! A safe, thread-confined wrapper over pdfium-sys. Every handle type here is
//! !Send by construction, because PDFium's globals are per-thread in the fork's
//! native builds. epdf-ops is what makes these usable from another thread.
mod bitmap;
mod document;
mod error;
mod library;
mod page;
mod text;

pub use bitmap::Bitmap;
pub use document::Document;
pub use error::PdfiumError;
pub use library::Library;
pub use page::Page;
pub use text::TextPage;
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cd rust-core-scaffold && cargo test -p pdfium`
Expected: PASS, 3 tests. If `matches_the_frozen_vectors` fails on the render diff, print the actual mean and stop: a large diff means the render sequence differs from the generator's, and the fix is in the Rust, never in the golden file.

- [ ] **Step 8: Commit**

```bash
git add rust-core-scaffold/crates/pdfium
git commit -m "feat(scaffold): add safe RAII pdfium wrapper verified against frozen vectors"
```

---

### Task 5: `epdf-ops`, the six operations on a dedicated thread

**Files:**
- Create: `rust-core-scaffold/crates/epdf-ops/Cargo.toml`
- Create: `rust-core-scaffold/crates/epdf-ops/src/lib.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/types.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/error.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/state.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/dispatch/mod.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/dispatch/threaded.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/dispatch/inline.rs`
- Create: `rust-core-scaffold/crates/epdf-ops/src/engine.rs`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/epdf-ops"` to `members`)
- Test: `rust-core-scaffold/crates/epdf-ops/tests/vectors.rs`
- Test: `rust-core-scaffold/crates/epdf-ops/tests/threading.rs`

**Interfaces:**
- Consumes: everything `pdfium` produces in Task 4.
- Produces:

```rust
#[derive(Debug, Clone, PartialEq)] pub struct Size { pub width: f32, pub height: f32 }
#[derive(Debug, Clone, PartialEq)] pub struct Rect { pub left: f64, pub top: f64, pub right: f64, pub bottom: f64 }
#[derive(Debug, Clone, PartialEq)] pub struct SearchHit { pub char_index: u32, pub char_count: u32, pub rects: Vec<Rect> }
#[derive(Debug, Clone, PartialEq)] pub struct Bitmap { pub width: u32, pub height: u32, pub stride: u32, pub bgra: Vec<u8> }

#[derive(Debug, Clone, PartialEq)]
pub enum OpsError {
    LoadFailed,
    PasswordRequired,
    PageOutOfRange { index: u32 },
    RenderFailed,
    /// The caller used a document it had closed, or the slab has no such entry.
    Closed,
    /// The worker thread is gone. Every document from this engine is dead and
    /// the engine must be recreated. Distinct from `Closed`, which is a caller
    /// mistake with an obvious remedy.
    EngineFailed { message: String },
    Internal { message: String },
}

pub struct Engine { /* Send + Sync */ }
impl Engine {
    pub fn new() -> Result<Arc<Engine>, OpsError>;
    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<Document>, OpsError>;
}

pub struct Document { /* Send + Sync */ }
impl Document {
    pub fn page_count(&self) -> Result<u32, OpsError>;
    pub fn page_size(&self, index: u32) -> Result<Size, OpsError>;
    pub fn render_page(&self, index: u32, scale: f32) -> Result<Bitmap, OpsError>;
    pub fn page_text(&self, index: u32) -> Result<String, OpsError>;
    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError>;
    pub fn close(&self);   // idempotent
}
```

`render_page`'s pixel dimensions are `max(1, round(page_points * scale))` on each axis, matching `build/generate-vectors.mjs` in Task 2 exactly. Any other rounding rule breaks the golden comparison in every host.

`search` takes a page index and returns that page's hits, with `char_index` local to the page. It is
per-page rather than whole-document because the frozen vectors record page 4 only, and because a
whole-document search is a different operation with different cancellation needs. All four host
bindings use this four-argument form.

- [ ] **Step 1: Write the failing vectors test**

Create `rust-core-scaffold/crates/epdf-ops/tests/vectors.rs`, reading the frozen file through `test_support`, the shared loader Task 4 created. Do not re-declare the loader or `mean_abs_diff`.

```rust
use test_support::{fixture_bytes, max_abs_diff, mean_abs_diff, vectors};

#[test]
fn the_six_operations_match_the_frozen_vectors() {
    let v = vectors();
    let bytes = fixture_bytes();

    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();

    assert_eq!(doc.page_count().unwrap(), v.page_count);

    let size = doc.page_size(v.page_index).unwrap();
    assert!((size.width - v.width).abs() < 0.001);
    assert!((size.height - v.height).abs() < 0.001);

    assert_eq!(doc.page_text(v.page_index).unwrap(), v.text);

    let hits = doc
        .search(v.page_index, v.search_query.clone(), v.search_case_sensitive)
        .unwrap();
    assert_eq!(hits.len(), v.search_hit_count);
    for (got, &(char_index, char_count)) in hits.iter().zip(&v.search_first_hits) {
        assert_eq!(got.char_index, char_index);
        assert_eq!(got.char_count, char_count);
    }

    let bitmap = doc.render_page(v.page_index, v.render_scale).unwrap();
    assert_eq!(bitmap.width, v.render_width);
    assert_eq!(bitmap.height, v.render_height);
    assert_eq!(bitmap.stride, v.render_stride);
    let mean = mean_abs_diff(&bitmap.bgra, &v.golden);
    assert!(mean <= v.tolerance, "mean abs diff {mean} exceeds {}", v.tolerance);
    let worst = max_abs_diff(&bitmap.bgra, &v.golden);
    assert!(worst <= v.max_tolerance, "max abs diff {worst} exceeds {}", v.max_tolerance);

    doc.close();
    doc.close(); // idempotent
    assert_eq!(doc.page_count(), Err(epdf_ops::OpsError::Closed));
}

#[test]
fn a_page_past_the_end_is_an_error_not_a_panic() {
    let bytes = fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();
    let index = doc.page_count().unwrap();
    assert_eq!(doc.page_size(index), Err(epdf_ops::OpsError::PageOutOfRange { index }));
}

#[test]
fn garbage_bytes_are_a_load_failure() {
    let engine = epdf_ops::Engine::new().unwrap();
    assert_eq!(engine.open(b"not a pdf".to_vec(), None).err(), Some(epdf_ops::OpsError::LoadFailed));
}
```

`test_support::Vectors` already carries `search_query`, `search_case_sensitive`, `search_hit_count`, `search_first_hits` and `render_scale`. Do not add fields to it for this task.

- [ ] **Step 2: Write the failing threading test**

Create `rust-core-scaffold/crates/epdf-ops/tests/threading.rs`:

```rust
//! The reason epdf_ops exists. pdfium::Document is !Send because the fork's
//! native builds set embedpdf_thread_local_globals=true, so a handle opened on
//! one thread is unusable on another. epdf_ops::Document must be usable from
//! any thread, and from several at once, by routing to the one thread that owns
//! the handle.

use std::sync::Arc;
use std::thread;

#[test]
fn a_document_opened_here_is_usable_from_other_threads() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc: Arc<epdf_ops::Document> = engine.open(bytes, None).unwrap();

    let expected = doc.page_count().unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let doc = Arc::clone(&doc);
            thread::spawn(move || doc.page_count().unwrap())
        })
        .collect();

    for handle in handles {
        assert_eq!(handle.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_renders_from_many_threads_all_succeed() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let doc = Arc::clone(&doc);
            thread::spawn(move || doc.render_page(i, 0.25).map(|b| b.bgra.len()))
        })
        .collect();

    for handle in handles {
        assert!(handle.join().unwrap().unwrap() > 0);
    }
}

#[test]
fn engine_and_document_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<epdf_ops::Engine>();
    assert_send_sync::<epdf_ops::Document>();
}
```

- [ ] **Step 3: Run both tests to verify they fail**

Run: `cd rust-core-scaffold && cargo test -p epdf-ops`
Expected: FAIL, `failed to read .../crates/epdf-ops/Cargo.toml`.

- [ ] **Step 4: Write the manifest, types and error**

`Cargo.toml`:

```toml
[package]
name = "epdf-ops"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
crate-type = ["rlib"]

[dependencies]
pdfium = { path = "../pdfium" }

[build-dependencies]
build-support = { path = "../build-support" }

[dev-dependencies]
test-support = { path = "../test-support" }
```

`crates/epdf-ops/build.rs`, three lines. Cargo does not propagate
`cargo:rustc-link-arg` from a dependency's build script to a dependent package's
own test binaries, so without this `cargo test -p epdf-ops` links but the test
binary cannot find `libembedpdf` at runtime:

```rust
fn main() {
    build_support::emit_rpath_only();
}
```

Add `"crates/epdf-ops"` to the `members` array in `rust-core-scaffold/Cargo.toml`.

`src/types.rs` holds `Size`, `Rect`, `SearchHit` and `Bitmap` exactly as declared in the Interfaces block above, each with `#[derive(Debug, Clone, PartialEq)]`.

`src/error.rs` holds `OpsError` as declared, plus a `From<pdfium::PdfiumError>` impl:

```rust
impl From<pdfium::PdfiumError> for OpsError {
    fn from(err: pdfium::PdfiumError) -> Self {
        match err {
            pdfium::PdfiumError::Load => Self::LoadFailed,
            pdfium::PdfiumError::Password => Self::PasswordRequired,
            pdfium::PdfiumError::PageOutOfRange(index) => Self::PageOutOfRange { index },
            pdfium::PdfiumError::PageLoad(index) => Self::Internal {
                message: format!("page {index} exists but PDFium could not load it"),
            },
            pdfium::PdfiumError::Bitmap => Self::RenderFailed,
            other => Self::Internal { message: other.to_string() },
        }
    }
}
```

Also give `OpsError` a `Display` impl and `impl std::error::Error`, because Task 6 needs `thiserror`-shaped behavior and Task 9's napi mapping reads `to_string()`.

- [ ] **Step 5: Write the state slab**

`src/state.rs`:

```rust
use std::collections::HashMap;
use std::rc::Rc;

use crate::OpsError;

/// Everything that must live on the PDFium thread. Held by whichever dispatch
/// implementation is compiled in, and only ever touched from that thread.
pub struct State {
    /// An `Rc` so every `pdfium::Document` in `docs` holds its own share. That
    /// is what keeps `FPDF_DestroyLibrary` from running while a document handle
    /// is still open, and it makes this struct's field order irrelevant.
    lib: Rc<pdfium::Library>,
    docs: HashMap<u64, pdfium::Document>,
    next_id: u64,
}

impl State {
    pub fn new() -> Result<Self, OpsError> {
        Ok(Self {
            lib: pdfium::Library::acquire()?,
            docs: HashMap::new(),
            next_id: 1,
        })
    }

    pub fn insert(&mut self, doc: pdfium::Document) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(id, doc);
        id
    }

    /// Clones the handle rather than lending it: `Document::from_bytes` takes
    /// `Rc<Library>` so the library outlives every document opened from it.
    pub fn library(&self) -> Rc<pdfium::Library> {
        Rc::clone(&self.lib)
    }

    pub fn get(&self, id: u64) -> Result<&pdfium::Document, OpsError> {
        self.docs.get(&id).ok_or(OpsError::Closed)
    }

    pub fn remove(&mut self, id: u64) {
        self.docs.remove(&id);
    }
}
```

- [ ] **Step 6: Write the two dispatch implementations**

`src/dispatch/mod.rs`:

```rust
//! One trait-shaped API, two implementations chosen by target.
//!
//! On every native target the fork builds with embedpdf_thread_local_globals=true
//! (see runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77), so PDFium's
//! globals are per-thread and a handle cannot cross threads. `threaded` owns one
//! OS thread and every op is a channel round-trip to it.
//!
//! On wasm32-unknown-emscripten there are no threads and each wasm instance has
//! its own linear memory, which is why that target leaves the flag off. `inline`
//! calls straight through.

#[cfg(not(target_os = "emscripten"))]
mod threaded;
#[cfg(not(target_os = "emscripten"))]
pub use threaded::Worker;

#[cfg(target_os = "emscripten")]
mod inline;
#[cfg(target_os = "emscripten")]
pub use inline::Worker;
```

`src/dispatch/threaded.rs`:

```rust
use std::sync::mpsc::{channel, Sender};
use std::thread;

use crate::{OpsError, State};

type Job = Box<dyn FnOnce(&mut State) + Send>;

pub struct Worker {
    tx: Sender<Job>,
}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        let (tx, rx) = channel::<Job>();
        let (ready_tx, ready_rx) = channel::<Result<(), OpsError>>();

        thread::Builder::new()
            .name("epdf-pdfium".to_string())
            .spawn(move || {
                let mut state = match State::new() {
                    Ok(state) => {
                        let _ = ready_tx.send(Ok(()));
                        state
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                // Ends when every Sender drops, which drops State and therefore
                // FPDF_DestroyLibrary on this thread. That is the only thread
                // allowed to call it.
                while let Ok(job) = rx.recv() {
                    job(&mut state);
                }
            })
            .map_err(|err| OpsError::Internal { message: err.to_string() })?;

        ready_rx
            .recv()
            .map_err(|err| OpsError::Internal { message: err.to_string() })??;

        Ok(Self { tx })
    }

    /// Blocks the calling thread until the PDFium thread answers. Deliberately
    /// synchronous: this scaffold does not answer the wave-0 cancellation
    /// question in docs/research/rust-core-port.md, and an async signature would
    /// imply that it does.
    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        let (tx, rx) = channel();
        self.tx
            .send(Box::new(move |state| {
                let _ = tx.send(f(state));
            }))
            .map_err(|_| OpsError::Closed)?;
        rx.recv().map_err(|_| OpsError::Closed)?
    }
}
```

`src/dispatch/inline.rs`:

```rust
use std::cell::UnsafeCell;

use crate::{OpsError, State};

pub struct Worker {
    state: UnsafeCell<State>,
}

// SAFETY: this file compiles only for wasm32-unknown-emscripten, which is
// single-threaded. There is no second thread to race against, and each wasm
// instance holds its own PDFium globals in its own linear memory, which is why
// build-target.sh leaves embedpdf_thread_local_globals off for wasm32. If this
// scaffold ever grows a pthreads wasm build, delete this impl and use `threaded`.
unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        Ok(Self { state: UnsafeCell::new(State::new()?) })
    }

    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        f(unsafe { &mut *self.state.get() })
    }
}
```

- [ ] **Step 7: Write engine.rs and lib.rs**

`src/engine.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dispatch::Worker;
use crate::{Bitmap, OpsError, Rect, SearchHit, Size};

pub struct Engine {
    worker: Arc<Worker>,
}

impl Engine {
    /// Spawns the PDFium thread eagerly, so a failure to initialize surfaces
    /// here rather than on the first `open`. Fallible on purpose: uniffi
    /// constructors may return `Result`, and Task 6 exports it that way.
    pub fn new() -> Result<Arc<Self>, OpsError> {
        Ok(Arc::new(Self { worker: Arc::new(Worker::spawn()?) }))
    }

    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<Document>, OpsError> {
        let worker = Arc::clone(&self.worker);
        let id = worker.run(move |state| {
            // The immutable borrow of the library ends before `insert` takes a
            // mutable one; `from_bytes` does not retain the reference.
            let doc = pdfium::Document::from_bytes(state.library(), &bytes, password.as_deref())?;
            Ok(state.insert(doc))
        })?;
        Ok(Arc::new(Document { worker, id, closed: AtomicBool::new(false) }))
    }
}

pub struct Document {
    worker: Arc<Worker>,
    id: u64,
    closed: AtomicBool,
}

impl Document {
    fn with<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&pdfium::Document) -> Result<T, OpsError> + Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(OpsError::Closed);
        }
        let id = self.id;
        self.worker.run(move |state| f(state.get(id)?))
    }

    pub fn page_count(&self) -> Result<u32, OpsError> {
        self.with(|doc| Ok(doc.page_count()))
    }

    pub fn page_size(&self, index: u32) -> Result<Size, OpsError> {
        self.with(move |doc| {
            let page = doc.page(index)?;
            Ok(Size { width: page.width(), height: page.height() })
        })
    }

    pub fn render_page(&self, index: u32, scale: f32) -> Result<Bitmap, OpsError> {
        self.with(move |doc| {
            let page = doc.page(index)?;
            // Same rounding as build/generate-vectors.mjs. Changing it breaks
            // the golden buffer comparison in every host.
            let width = ((page.width() * scale).round() as u32).max(1);
            let height = ((page.height() * scale).round() as u32).max(1);
            let bitmap = page.render_bgra(width, height)?;
            Ok(Bitmap {
                width: bitmap.width,
                height: bitmap.height,
                stride: bitmap.stride,
                bgra: bitmap.bgra,
            })
        })
    }

    pub fn page_text(&self, index: u32) -> Result<String, OpsError> {
        self.with(move |doc| Ok(doc.page(index)?.text()?.text()?))
    }

    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError> {
        self.with(move |doc| {
            let page = doc.page(index)?;
            let text = page.text()?;
            let mut hits = Vec::new();
            for (char_index, char_count) in text.find(&query, case_sensitive)? {
                let rects = text
                    .rects(char_index, char_count)?
                    .into_iter()
                    .map(|[left, top, right, bottom]| Rect { left, top, right, bottom })
                    .collect();
                hits.push(SearchHit { char_index, char_count, rects });
            }
            Ok(hits)
        })
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let id = self.id;
        let _ = self.worker.run(move |state| {
            state.remove(id);
            Ok(())
        });
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.close();
    }
}
```

`src/lib.rs`:

```rust
//! Six coarse operations over the EmbedPDF PDFium fork, and the only surface
//! any host language sees. Recommendation 3 of
//! docs/research/pdfium-rust-wrapper-decision.md is why it is coarse: a
//! per-item host boundary measured 2.6x worse in wasm and 7.16x worse natively
//! than a per-operation one.
mod dispatch;
mod engine;
mod error;
mod state;
mod types;

pub use engine::{Document, Engine};
pub use error::OpsError;
pub use state::State;
pub use types::{Bitmap, Rect, SearchHit, Size};
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cd rust-core-scaffold && cargo test -p epdf-ops`
Expected: PASS, 6 tests across `vectors.rs` and `threading.rs`.

- [ ] **Step 9: Commit**

```bash
git add rust-core-scaffold/crates/epdf-ops
git commit -m "feat(scaffold): add coarse ops on a thread-confined pdfium worker"
```

---

### Task 6: `epdf-uniffi` and the Swift/Kotlin binding generator

**Files:**
- Create: `rust-core-scaffold/crates/epdf-uniffi/Cargo.toml`
- Create: `rust-core-scaffold/crates/epdf-uniffi/src/lib.rs`
- Create: `rust-core-scaffold/crates/uniffi-bindgen/Cargo.toml`
- Create: `rust-core-scaffold/crates/uniffi-bindgen/src/main.rs`
- Create: `rust-core-scaffold/build/build-swift.sh`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/epdf-uniffi"` and `"crates/uniffi-bindgen"` to `members`)
- Test: `rust-core-scaffold/crates/epdf-uniffi/tests/reexport.rs`

**Interfaces:**
- Consumes: `epdf_ops::{Engine, Document, Size, Rect, SearchHit, Bitmap, OpsError}` from Task 5.
- Produces: `libepdf_uniffi.dylib` plus generated Swift at `packages/swift/EpdfOps/Sources/EpdfOps/Generated/epdf_uniffi.swift`, a C header and a modulemap at `packages/swift/EpdfOps/Sources/EpdfOpsFFI/`, and the same generator ready for Kotlin in Task 13. The uniffi-facing type names are `EpdfEngine`, `EpdfDocument`, `Size`, `Rect`, `SearchHit`, `Bitmap`, `OpsError`.

uniffi lives in its own crate rather than in `epdf-ops` on purpose. `epdf-ops` has to compile for `wasm32-unknown-emscripten` in Task 11, and whether `uniffi::setup_scaffolding!()` builds for that target is unverified. Putting it here means the question never has to be answered, at the cost of a thin wrapper per type.

- [ ] **Step 1: Write the failing test**

Create `rust-core-scaffold/crates/epdf-uniffi/tests/reexport.rs`:

```rust
//! The wrapper crate adds no behavior, so this test only proves the wrapped
//! surface still answers the same numbers. The real parity assertions live in
//! the Swift and Kotlin suites, which is where a binding can actually drift.

#[test]
fn the_wrapped_engine_opens_the_fixture() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_uniffi::EpdfEngine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();
    assert!(doc.page_count().unwrap() > 4);
    let size = doc.page_size(4).unwrap();
    assert!(size.width > 0.0 && size.height > 0.0);
    doc.close();
}

#[test]
fn errors_cross_the_wrapper_unchanged() {
    let engine = epdf_uniffi::EpdfEngine::new().unwrap();
    let err = engine.open(b"not a pdf".to_vec(), None).unwrap_err();
    assert!(matches!(err, epdf_uniffi::OpsError::LoadFailed));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && cargo test -p epdf-uniffi`
Expected: FAIL, `failed to read .../crates/epdf-uniffi/Cargo.toml`.

- [ ] **Step 3: Write the manifests**

`crates/epdf-uniffi/Cargo.toml`:

```toml
[package]
name = "epdf-uniffi"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "epdf_uniffi"
# cdylib for `uniffi-bindgen generate --library` and for JNA on the JVM;
# staticlib for the iOS slice once embedpdf/runtime ships ios-arm64.
crate-type = ["cdylib", "staticlib", "rlib"]

[dependencies]
epdf-ops = { path = "../epdf-ops" }
thiserror = "2"
uniffi = "=0.32.0"

[build-dependencies]
build-support = { path = "../build-support" }

[dev-dependencies]
test-support = { path = "../test-support" }
```

`crates/epdf-uniffi/build.rs`, for the same reason `epdf-ops` needs one:

```rust
fn main() {
    build_support::emit_rpath_only();
}
```

Add `"crates/epdf-uniffi"` and `"crates/uniffi-bindgen"` to the `members` array in `rust-core-scaffold/Cargo.toml`.

`crates/uniffi-bindgen/Cargo.toml`:

```toml
[package]
name = "uniffi-bindgen"
version = "0.0.0"
edition = "2024"
publish = false

[[bin]]
name = "uniffi-bindgen"
path = "src/main.rs"

[dependencies]
uniffi = { version = "=0.32.0", features = ["cli"] }
```

`crates/uniffi-bindgen/src/main.rs`:

```rust
//! The local bindgen binary uniffi 0.32 wants, so the generator version can
//! never drift from the runtime version: both come from the one `=0.32.0` pin.
fn main() {
    uniffi::uniffi_bindgen_main()
}
```

- [ ] **Step 4: Write the wrapper**

`crates/epdf-uniffi/src/lib.rs`:

```rust
//! uniffi's view of epdf-ops. Every type here is a thin newtype or a mirror,
//! because uniffi's derives have to sit on types this crate owns.
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SearchHit {
    pub char_index: u32,
    pub char_count: u32,
    pub rects: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// BGRA, `stride * height` bytes. uniffi copies this once on the way out:
    /// Data in Swift, ByteArray in Kotlin. At 4x zoom on a large page that copy
    /// is not free, and it is the first thing to revisit if render latency
    /// matters. See docs/superpowers/specs/2026-09-03-rust-core-scaffold-design.md.
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum OpsError {
    #[error("the bytes are not a document PDFium can open")]
    LoadFailed,
    #[error("the document is encrypted and the password was wrong or missing")]
    PasswordRequired,
    #[error("page index {index} is past the end of the document")]
    PageOutOfRange { index: u32 },
    #[error("rendering the page failed")]
    RenderFailed,
    #[error("the document is closed")]
    Closed,
    #[error("the PDF engine failed and must be recreated: {message}")]
    EngineFailed { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl From<epdf_ops::OpsError> for OpsError {
    fn from(err: epdf_ops::OpsError) -> Self {
        match err {
            epdf_ops::OpsError::LoadFailed => Self::LoadFailed,
            epdf_ops::OpsError::PasswordRequired => Self::PasswordRequired,
            epdf_ops::OpsError::PageOutOfRange { index } => Self::PageOutOfRange { index },
            epdf_ops::OpsError::RenderFailed => Self::RenderFailed,
            epdf_ops::OpsError::Closed => Self::Closed,
            epdf_ops::OpsError::EngineFailed { message } => Self::EngineFailed { message },
            epdf_ops::OpsError::Internal { message } => Self::Internal { message },
        }
    }
}

#[derive(uniffi::Object)]
pub struct EpdfEngine {
    inner: Arc<epdf_ops::Engine>,
}

#[uniffi::export]
impl EpdfEngine {
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, OpsError> {
        Ok(Arc::new(Self { inner: epdf_ops::Engine::new()? }))
    }

    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<EpdfDocument>, OpsError> {
        Ok(Arc::new(EpdfDocument { inner: self.inner.open(bytes, password)? }))
    }
}

#[derive(uniffi::Object)]
pub struct EpdfDocument {
    inner: Arc<epdf_ops::Document>,
}

#[uniffi::export]
impl EpdfDocument {
    pub fn page_count(&self) -> Result<u32, OpsError> {
        Ok(self.inner.page_count()?)
    }

    pub fn page_size(&self, index: u32) -> Result<Size, OpsError> {
        let size = self.inner.page_size(index)?;
        Ok(Size { width: size.width, height: size.height })
    }

    pub fn render_page(&self, index: u32, scale: f32) -> Result<Bitmap, OpsError> {
        let b = self.inner.render_page(index, scale)?;
        Ok(Bitmap { width: b.width, height: b.height, stride: b.stride, bgra: b.bgra })
    }

    pub fn page_text(&self, index: u32) -> Result<String, OpsError> {
        Ok(self.inner.page_text(index)?)
    }

    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError> {
        Ok(self
            .inner
            .search(index, query, case_sensitive)?
            .into_iter()
            .map(|hit| SearchHit {
                char_index: hit.char_index,
                char_count: hit.char_count,
                rects: hit
                    .rects
                    .into_iter()
                    .map(|r| Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                    .collect(),
            })
            .collect())
    }

    /// Idempotent. Swift releases this object on ARC and would call it anyway;
    /// Kotlin releases on the GC, whenever that is, which is why it exists.
    pub fn close(&self) {
        self.inner.close();
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd rust-core-scaffold && cargo test -p epdf-uniffi`
Expected: PASS, 2 tests.

- [ ] **Step 6: Write build-swift.sh**

`rust-core-scaffold/build/build-swift.sh`:

```bash
#!/usr/bin/env bash
# Builds libepdf_uniffi for the requested Apple targets, generates the Swift
# bindings, and stages both dylibs where Package.swift and the Xcode project
# expect them.
#
#   bash build/build-swift.sh            # macos-arm64 only, the default today
#   bash build/build-swift.sh ios-arm64  # exits 2 until embedpdf/runtime ships it
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICES=("${@:-macos-arm64}")

declare -A RUST_TARGET=(
  [macos-arm64]=aarch64-apple-darwin
  [ios-arm64]=aarch64-apple-ios
  [ios-sim-arm64]=aarch64-apple-ios-sim
)
declare -A FORK_TARGET=(
  [macos-arm64]=darwin-arm64
  [ios-arm64]=ios-arm64
  [ios-sim-arm64]=ios-sim-arm64
)

SWIFT_PKG="$ROOT/packages/swift/EpdfOps"
GENERATED="$SWIFT_PKG/Sources/EpdfOps/Generated"
FFI_TARGET="$SWIFT_PKG/Sources/EpdfOpsFFI"

for slice in "${SLICES[@]}"; do
  rust="${RUST_TARGET[$slice]:-}"
  fork="${FORK_TARGET[$slice]:-}"
  if [[ -z "$rust" ]]; then
    echo "unknown slice: $slice (known: ${!RUST_TARGET[*]})" >&2
    exit 1
  fi

  # Exits 2 with its own message for the pending mobile targets.
  bash "$ROOT/build/fetch-libpdfium.sh" "$fork"

  rustup target add "$rust"
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$fork" cargo build --release -p epdf-uniffi --target "$rust" )

  dest="$SWIFT_PKG/Frameworks/$slice"
  mkdir -p "$dest"
  cp "$ROOT/target/$rust/release/libepdf_uniffi.dylib" "$dest/"
  cp "$ROOT/build/libpdfium/$fork/lib/libembedpdf."* "$dest/"
  install_name_tool -id "@rpath/libepdf_uniffi.dylib" "$dest/libepdf_uniffi.dylib"
  echo "staged $slice -> $dest"
done

# Bindings are architecture-independent, so generate them once from whichever
# slice was built first.
first="${SLICES[0]}"
mkdir -p "$GENERATED" "$FFI_TARGET"
( cd "$ROOT" && cargo run --release --bin uniffi-bindgen -- generate \
    --library "$SWIFT_PKG/Frameworks/$first/libepdf_uniffi.dylib" \
    --language swift \
    --out-dir "$GENERATED" )

# uniffi emits the .swift beside a C header and a modulemap. SwiftPM needs the
# latter two in their own systemLibrary target.
mv "$GENERATED"/epdf_uniffiFFI.h "$FFI_TARGET/"
mv "$GENERATED"/epdf_uniffiFFI.modulemap "$FFI_TARGET/module.modulemap"

echo "generated Swift bindings -> $GENERATED"
```

Then `chmod +x rust-core-scaffold/build/build-swift.sh`.

- [ ] **Step 7: Run it and confirm the artifacts**

Run:
```bash
cd rust-core-scaffold
bash build/build-swift.sh
ls packages/swift/EpdfOps/Frameworks/macos-arm64
ls packages/swift/EpdfOps/Sources/EpdfOps/Generated
ls packages/swift/EpdfOps/Sources/EpdfOpsFFI
bash build/build-swift.sh ios-arm64
```
Expected: `libepdf_uniffi.dylib` and `libembedpdf.dylib` staged; `epdf_uniffi.swift` generated; `epdf_uniffiFFI.h` and `module.modulemap` in the FFI target. The last command exits 2 printing `no libembedpdf for ios-arm64: embedpdf/runtime does not build it yet.`

If uniffi's emitted filenames differ from `epdf_uniffiFFI.h` / `epdf_uniffiFFI.modulemap`, run `ls "$GENERATED"` and correct the two `mv` lines to the real names rather than renaming what uniffi produced.

- [ ] **Step 8: Commit**

```bash
git add rust-core-scaffold/crates/epdf-uniffi rust-core-scaffold/crates/uniffi-bindgen \
  rust-core-scaffold/build/build-swift.sh
git commit -m "feat(scaffold): add uniffi wrapper crate and swift binding build"
```

---

### Task 7: The Swift package

**Files:**
- Create: `rust-core-scaffold/packages/swift/EpdfOps/Package.swift`
- Create: `rust-core-scaffold/packages/swift/EpdfOps/Sources/EpdfOps/Epdf.swift`
- Create: `rust-core-scaffold/build/test-swift.sh`
- Test: `rust-core-scaffold/packages/swift/EpdfOps/Tests/EpdfOpsTests/VectorsTests.swift`

**Interfaces:**
- Consumes: the generated `epdf_uniffi.swift` (types `EpdfEngine`, `EpdfDocument`, `Size`, `Rect`, `SearchHit`, `Bitmap`, `OpsError`) and the staged dylibs from Task 6.
- Produces: a Swift module `EpdfOps` exporting the uniffi types plus one convenience: `EpdfDocument.renderPageCGImage(index:scale:) -> CGImage`, which Task 8's SwiftUI view uses.

- [ ] **Step 1: Write the failing test**

Create `Tests/EpdfOpsTests/VectorsTests.swift`:

```swift
import Foundation
import Testing
@testable import EpdfOps

/// Same frozen file the Rust tests read: rust-core-scaffold/vectors/report-page4.json.
/// A Swift binding that drifts fails here instead of looking fine on screen.
private struct Vectors {
    let pageIndex: UInt32
    let pageCount: UInt32
    let width: Float
    let height: Float
    let charCount: UInt32
    let text: String
    let searchHitCount: Int
    let renderScale: Float
    let renderWidth: UInt32
    let renderHeight: UInt32
    let renderStride: UInt32
    let golden: Data
    let tolerance: Double

    static func load() throws -> Vectors {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()   // EpdfOpsTests
            .deletingLastPathComponent()   // Tests
            .deletingLastPathComponent()   // EpdfOps
            .deletingLastPathComponent()   // swift
            .deletingLastPathComponent()   // packages
        let json = try JSONSerialization.jsonObject(
            with: try Data(contentsOf: root.appendingPathComponent("vectors/report-page4.json"))
        ) as! [String: Any]
        let size = json["size"] as! [String: Any]
        let render = json["render"] as! [String: Any]
        let search = json["search"] as! [String: Any]
        return Vectors(
            pageIndex: UInt32(json["pageIndex"] as! Int),
            pageCount: UInt32(json["pageCount"] as! Int),
            width: Float(size["width"] as! Double),
            height: Float(size["height"] as! Double),
            charCount: UInt32(json["charCount"] as! Int),
            text: json["text"] as! String,
            searchHitCount: search["hitCount"] as! Int,
            renderScale: Float(render["scale"] as! Double),
            renderWidth: UInt32(render["width"] as! Int),
            renderHeight: UInt32(render["height"] as! Int),
            renderStride: UInt32(render["stride"] as! Int),
            golden: try Data(contentsOf: root.appendingPathComponent("vectors/\(render["golden"] as! String)")),
            tolerance: render["meanAbsDiffTolerance"] as! Double
        )
    }
}

private func meanAbsDiff(_ a: Data, _ b: Data) -> Double {
    precondition(a.count == b.count, "buffer lengths differ: \(a.count) vs \(b.count)")
    var total = 0.0
    for i in a.indices { total += Double(abs(Int(a[i]) - Int(b[i]))) }
    return total / Double(a.count)
}

private func openFixture() throws -> EpdfDocument {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent()
    // uniffi maps Rust `Vec<u8>` to Swift `Data`, so no [UInt8] conversion.
    let bytes = try Data(contentsOf: root.appendingPathComponent("fixtures/report.pdf"))
    return try EpdfEngine().open(bytes: bytes, password: nil)
}

@Test func matchesTheFrozenVectors() throws {
    let v = try Vectors.load()
    let doc = try openFixture()
    defer { doc.close() }

    #expect(try doc.pageCount() == v.pageCount)

    let size = try doc.pageSize(index: v.pageIndex)
    #expect(abs(size.width - v.width) < 0.001)
    #expect(abs(size.height - v.height) < 0.001)

    #expect(try doc.pageText(index: v.pageIndex) == v.text)

    let hits = try doc.search(index: v.pageIndex, query: "the", caseSensitive: false)
    #expect(hits.count == v.searchHitCount)

    let bitmap = try doc.renderPage(index: v.pageIndex, scale: v.renderScale)
    #expect(bitmap.width == v.renderWidth)
    #expect(bitmap.height == v.renderHeight)
    #expect(bitmap.stride == v.renderStride)
    #expect(meanAbsDiff(bitmap.bgra, v.golden) <= v.tolerance)
}

@Test func aPagePastTheEndThrows() throws {
    let doc = try openFixture()
    defer { doc.close() }
    let index = try doc.pageCount()
    #expect(throws: OpsError.self) { try doc.pageSize(index: index) }
}

@Test func rendersACGImage() throws {
    let v = try Vectors.load()
    let doc = try openFixture()
    defer { doc.close() }
    let image = try doc.renderPageCGImage(index: v.pageIndex, scale: v.renderScale)
    #expect(image.width == Int(v.renderWidth))
    #expect(image.height == Int(v.renderHeight))
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && bash build/test-swift.sh`
Expected: FAIL, `build/test-swift.sh: No such file or directory`.

- [ ] **Step 3: Write Package.swift**

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "EpdfOps",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "EpdfOps", targets: ["EpdfOps"])
    ],
    targets: [
        // The C header and modulemap uniffi emits, staged here by
        // build/build-swift.sh.
        .systemLibrary(name: "EpdfOpsFFI", path: "Sources/EpdfOpsFFI"),
        .target(
            name: "EpdfOps",
            dependencies: ["EpdfOpsFFI"],
            linkerSettings: [
                // No -L here on purpose: build/test-swift.sh exports
                // LIBRARY_PATH and DYLD_LIBRARY_PATH, and the Xcode project
                // sets its own search paths. Hardcoding a relative path would
                // depend on the directory `swift build` was run from.
                .linkedLibrary("epdf_uniffi"),
                .linkedLibrary("embedpdf"),
            ]
        ),
        .testTarget(name: "EpdfOpsTests", dependencies: ["EpdfOps"]),
    ]
)
```

- [ ] **Step 4: Write the facade**

`Sources/EpdfOps/Epdf.swift`:

```swift
import CoreGraphics
import Foundation

/// The whole Swift-side addition to what uniffi generates: one bridge from a
/// BGRA buffer to a CGImage, so a SwiftUI view can show a page without knowing
/// anything about pixel layout.
extension EpdfDocument {
    public func renderPageCGImage(index: UInt32, scale: Float) throws -> CGImage {
        let bitmap = try renderPage(index: index, scale: scale)

        // bitmap.bgra is already Data: uniffi maps Vec<u8> that way. And the
        // case is UpperCamelCase: uniffi 0.32.0 does NOT lowercase Rust enum
        // variant names in Swift. Verified against the generated
        // Sources/EpdfOps/Generated/epdf_uniffi.swift.
        guard let provider = CGDataProvider(data: bitmap.bgra as CFData) else {
            throw OpsError.RenderFailed
        }

        // PDFium's FPDFBitmap_BGRA is byte order B,G,R,A in memory, which on a
        // little-endian machine is a 32-bit little-endian ARGB word. Hence
        // .byteOrder32Little with premultipliedFirst.
        guard
            let image = CGImage(
                width: Int(bitmap.width),
                height: Int(bitmap.height),
                bitsPerComponent: 8,
                bitsPerPixel: 32,
                bytesPerRow: Int(bitmap.stride),
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue)
                    .union(.byteOrder32Little),
                provider: provider,
                decode: nil,
                shouldInterpolate: false,
                intent: .defaultIntent
            )
        else {
            throw OpsError.RenderFailed
        }
        return image
    }
}
```

- [ ] **Step 5: Write the test runner**

`rust-core-scaffold/build/test-swift.sh`:

```bash
#!/usr/bin/env bash
# swift test with the staged dylibs on the linker and loader paths. Without
# this, Package.swift's .linkedLibrary("epdf_uniffi") has nowhere to look.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICE="${1:-macos-arm64}"
LIBS="$ROOT/packages/swift/EpdfOps/Frameworks/$SLICE"

if [[ ! -f "$LIBS/libepdf_uniffi.dylib" ]]; then
  echo "no staged libraries at $LIBS" >&2
  echo "Fix with: bash build/build-swift.sh $SLICE" >&2
  exit 1
fi

export LIBRARY_PATH="$LIBS${LIBRARY_PATH:+:$LIBRARY_PATH}"

cd "$ROOT/packages/swift/EpdfOps"

# LIBRARY_PATH covers link time. Load time needs a baked-in rpath, NOT
# DYLD_LIBRARY_PATH: `swift test` runs through `swiftpm-testing-helper`, which
# is Apple-signed with the hardened runtime and carries no
# allow-dyld-environment-variables entitlement, so macOS strips every DYLD_*
# variable before it reaches the test bundle. Do not "simplify" this back to an
# environment variable; it silently stops working.
swift test -Xlinker -rpath -Xlinker "$LIBS" "${@:2}"
```

Then `chmod +x rust-core-scaffold/build/test-swift.sh`.

- [ ] **Step 6: Run the tests to verify they pass**

Run:
```bash
cd rust-core-scaffold
bash build/build-swift.sh
bash build/test-swift.sh
```
Expected: PASS, 3 tests. Swift 6.3.3 and Xcode 26.6 are already installed on this machine, so no toolchain step is needed.

If `matchesTheFrozenVectors` fails only on `meanAbsDiff`, do not raise `meanAbsDiffTolerance` in the vectors file. Print the actual value, and treat anything above about 2.0 as a real difference in the render path.

- [ ] **Step 7: Commit**

```bash
git add rust-core-scaffold/packages/swift/EpdfOps/Package.swift \
  rust-core-scaffold/packages/swift/EpdfOps/Sources/EpdfOps/Epdf.swift \
  rust-core-scaffold/packages/swift/EpdfOps/Tests \
  rust-core-scaffold/build/test-swift.sh
git commit -m "feat(scaffold): add EpdfOps swift package verified against frozen vectors"
```

---

### Task 8: The iOS app

**Files:**
- Create: `rust-core-scaffold/apps/ios/project.yml` (XcodeGen spec; the `.xcodeproj` is generated and gitignored)
- Create: `rust-core-scaffold/apps/ios/EpdfScaffold/EpdfScaffoldApp.swift`
- Create: `rust-core-scaffold/apps/ios/EpdfScaffold/ContentView.swift`
- Create: `rust-core-scaffold/apps/ios/EpdfScaffold/DocumentModel.swift`
- Create: `rust-core-scaffold/apps/ios/EpdfScaffold/Info.plist`
- Modify: `rust-core-scaffold/.gitignore` (add `apps/ios/*.xcodeproj`)
- Create: `rust-core-scaffold/build/build-ios-app.sh`

**Interfaces:**
- Consumes: the `EpdfOps` Swift package by local path, and `fixtures/report.pdf` as a bundled resource.
- Produces: an app with a macOS destination that runs today and an iOS destination that builds only once `ios-arm64` exists.

The `.xcodeproj` is generated by XcodeGen from a committed `project.yml` rather than hand-written. A `project.pbxproj` is thousands of lines of unreviewable UUIDs, and this scaffold is meant to be read. The cost is one Homebrew dependency: `brew install xcodegen`.

- [ ] **Step 1: Install XcodeGen and confirm the version**

Run:
```bash
brew install xcodegen
xcodegen --version
```
Expected: a version prints. Record it in `rust-core-scaffold/README.md` in Task 15.

- [ ] **Step 2: Write the project spec**

`apps/ios/project.yml`:

```yaml
name: EpdfScaffold
options:
  bundleIdPrefix: com.embedpdf.scaffold
  deploymentTarget:
    iOS: "17.0"
    macOS: "14.0"
packages:
  EpdfOps:
    path: ../../packages/swift/EpdfOps
targets:
  EpdfScaffold:
    type: application
    supportedDestinations: [iOS, macOS]
    sources:
      - path: EpdfScaffold
    resources:
      - path: ../../fixtures/report.pdf
    dependencies:
      - package: EpdfOps
        product: EpdfOps
    info:
      path: EpdfScaffold/Info.plist
    settings:
      base:
        # Where build/build-swift.sh stages libepdf_uniffi and libembedpdf.
        LIBRARY_SEARCH_PATHS:
          - $(SRCROOT)/../../packages/swift/EpdfOps/Frameworks/macos-arm64
        LD_RUNPATH_SEARCH_PATHS:
          - $(SRCROOT)/../../packages/swift/EpdfOps/Frameworks/macos-arm64
```

- [ ] **Step 3: Write the model**

`apps/ios/EpdfScaffold/DocumentModel.swift`:

```swift
import CoreGraphics
import Foundation
import Observation
import EpdfOps

/// All six operations, driven off the main thread. epdf-ops routes every call
/// to the one thread that owns the PDFium handle, so this actor's job is only
/// to keep the UI responsive, not to protect the handle.
@Observable
@MainActor
final class DocumentModel {
    var pageCount: UInt32 = 0
    var pageSize: Size?
    var image: CGImage?
    var textPreview: String = ""
    var searchHitCount: Int = 0
    var failure: String?

    private let pageIndex: UInt32 = 4
    private var document: EpdfDocument?

    func load() async {
        do {
            guard let url = Bundle.main.url(forResource: "report", withExtension: "pdf") else {
                failure = "report.pdf is not in the app bundle"
                return
            }
            let bytes = try Data(contentsOf: url)

            let doc = try await Task.detached { try EpdfEngine().open(bytes: bytes, password: nil) }.value
            document = doc

            pageCount = try doc.pageCount()
            pageSize = try doc.pageSize(index: pageIndex)
            textPreview = String(try doc.pageText(index: pageIndex).prefix(600))
            searchHitCount = try doc.search(index: pageIndex, query: "the", caseSensitive: false).count
            image = try doc.renderPageCGImage(index: pageIndex, scale: 2.0)
        } catch {
            failure = String(describing: error)
        }
    }

    func close() {
        document?.close()
        document = nil
    }
}
```

- [ ] **Step 4: Write the views**

`apps/ios/EpdfScaffold/EpdfScaffoldApp.swift`:

```swift
import SwiftUI

@main
struct EpdfScaffoldApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
```

`apps/ios/EpdfScaffold/ContentView.swift`:

```swift
import SwiftUI

struct ContentView: View {
    @State private var model = DocumentModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let failure = model.failure {
                    Text(failure)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(.red)
                }

                LabeledContent("Pages", value: "\(model.pageCount)")
                if let size = model.pageSize {
                    LabeledContent("Page 4", value: String(format: "%.1f x %.1f pt", size.width, size.height))
                }
                LabeledContent("Hits for \"the\"", value: "\(model.searchHitCount)")

                if let image = model.image {
                    Image(decorative: image, scale: 2.0)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .border(.separator)
                }

                Text(model.textPreview)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
            }
            .padding()
        }
        .task { await model.load() }
        .onDisappear { model.close() }
    }
}
```

`apps/ios/EpdfScaffold/Info.plist` is the minimal SwiftUI app plist: `CFBundleDisplayName` `EpdfScaffold`, `UILaunchScreen` an empty dict, `CFBundleShortVersionString` `0.0.0`.

- [ ] **Step 5: Write the build script and generate the project**

`rust-core-scaffold/build/build-ios-app.sh`:

```bash
#!/usr/bin/env bash
# Generates the Xcode project and builds the app for one destination.
#
#   bash build/build-ios-app.sh                          # macOS, works today
#   bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 16'
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESTINATION="${1:-platform=macOS}"

( cd "$ROOT/apps/ios" && xcodegen generate )

xcodebuild \
  -project "$ROOT/apps/ios/EpdfScaffold.xcodeproj" \
  -scheme EpdfScaffold \
  -destination "$DESTINATION" \
  build
```

Add `apps/ios/*.xcodeproj` to `rust-core-scaffold/.gitignore`, then `chmod +x rust-core-scaffold/build/build-ios-app.sh`.

- [ ] **Step 6: Build and run on macOS**

Run:
```bash
cd rust-core-scaffold
bash build/build-swift.sh
bash build/build-ios-app.sh
open apps/ios/EpdfScaffold.xcodeproj
```
Expected: `** BUILD SUCCEEDED **`. Run the macOS destination from Xcode and confirm the window shows the page count, the page 4 size and the hit count that `vectors/report-page4.json` records, a rendered page image, and the first 600 characters of page 4. Compare against what `node apps/node-cli/main.mjs --page 4` printed in Task 10: the two must agree digit for digit.

- [ ] **Step 7: Confirm the iOS destination fails for the documented reason**

Run:
```bash
cd rust-core-scaffold
bash build/build-swift.sh ios-sim-arm64
```
Expected: exit 2, `no libembedpdf for ios-sim-arm64: embedpdf/runtime does not build it yet.` That is the gate, not a bug. Do not work around it.

- [ ] **Step 8: Commit**

```bash
git add rust-core-scaffold/apps/ios rust-core-scaffold/build/build-ios-app.sh rust-core-scaffold/.gitignore
git commit -m "feat(scaffold): add multiplatform SwiftUI demo app"
```

---

### Task 9: `epdf-node`, the napi-rs addon and the Node package

**Files:**
- Create: `rust-core-scaffold/crates/epdf-node/Cargo.toml`
- Create: `rust-core-scaffold/crates/epdf-node/build.rs`
- Create: `rust-core-scaffold/crates/epdf-node/src/lib.rs`
- Create: `rust-core-scaffold/build/build-node.sh`
- Create: `rust-core-scaffold/packages/node/package.json`
- Create: `rust-core-scaffold/packages/node/index.mjs`
- Create: `rust-core-scaffold/package.json` (the scaffold's own npm root)
- Create: `rust-core-scaffold/packages/test-support/vectors.mjs`
- Create: `rust-core-scaffold/packages/test-support/package.json`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/epdf-node"` to `members`)
- Test: `rust-core-scaffold/packages/node/vectors.test.mjs`

**Interfaces:**
- Consumes: `epdf_ops::{Engine, Document, OpsError}` from Task 5.
- Produces: `packages/node/epdf-node.<fork-target>.node`, loaded by `packages/node/index.mjs`, which exports `openDocument(bytes: Uint8Array, password?: string) -> Document` where `Document` has `pageCount()`, `pageSize(index)`, `renderPage(index, scale)`, `pageText(index)`, `search(index, query, caseSensitive)` and `close()`. `renderPage` returns `{ width, height, stride, bgra: Buffer }`.

Recommendation 4 of `docs/research/pdfium-rust-wrapper-decision.md` says to prefer `f64` pointers over BigInt on Node. It does not apply here, and that is the point: a coarse boundary exposes no pointers to JavaScript at all, so the representation question disappears rather than being answered. Note that in the crate docs.

- [ ] **Step 1: Write the failing test**

First write the shared JS loader, `rust-core-scaffold/packages/test-support/vectors.mjs`, which both this task's suite and Task 11's read. Neither suite re-declares it:

```js
/**
 * The one JS reader of vectors/report-page4.json, the frozen file
 * build/generate-vectors.mjs wrote once and nothing regenerates. The Rust
 * suites read the same file through crates/test-support.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const scaffoldRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

export async function loadVectors() {
  const vectors = JSON.parse(await readFile(resolve(scaffoldRoot, 'vectors/report-page4.json'), 'utf8'));
  return {
    vectors,
    golden: await readFile(resolve(scaffoldRoot, 'vectors', vectors.render.golden)),
    fixture: await readFile(resolve(scaffoldRoot, 'fixtures/report.pdf')),
  };
}

/**
 * Mean absolute per-byte difference. Rendering is compared this way rather than
 * by hash: a hash pins float and rasterization behaviour across four
 * architectures and fails for reasons unrelated to this code.
 */
export function meanAbsDiff(a, b) {
  if (a.length !== b.length) {
    throw new Error(`buffer lengths differ: ${a.length} vs ${b.length}`);
  }
  let total = 0;
  for (let i = 0; i < a.length; i += 1) total += Math.abs(a[i] - b[i]);
  return total / a.length;
}
```

`rust-core-scaffold/packages/test-support/package.json`:

```json
{
  "name": "@epdf-scaffold/test-support",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "main": "./vectors.mjs"
}
```

Then create `rust-core-scaffold/packages/node/vectors.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loadVectors, meanAbsDiff } from '../test-support/vectors.mjs';
import { openDocument } from './index.mjs';

const { vectors, golden, fixture } = await loadVectors();

test('the six operations match the frozen vectors', () => {
  const doc = openDocument(fixture);
  try {
    assert.equal(doc.pageCount(), vectors.pageCount);

    const size = doc.pageSize(vectors.pageIndex);
    assert.ok(Math.abs(size.width - vectors.size.width) < 0.001);
    assert.ok(Math.abs(size.height - vectors.size.height) < 0.001);

    assert.equal(doc.pageText(vectors.pageIndex), vectors.text);

    const hits = doc.search(vectors.pageIndex, vectors.search.query, vectors.search.caseSensitive);
    assert.equal(hits.length, vectors.search.hitCount);
    for (const [i, want] of vectors.search.firstHits.entries()) {
      assert.equal(hits[i].charIndex, want.charIndex);
      assert.equal(hits[i].charCount, want.charCount);
    }

    const bitmap = doc.renderPage(vectors.pageIndex, vectors.render.scale);
    assert.equal(bitmap.width, vectors.render.width);
    assert.equal(bitmap.height, vectors.render.height);
    assert.equal(bitmap.stride, vectors.render.stride);
    const diff = meanAbsDiff(bitmap.bgra, golden);
    assert.ok(diff <= vectors.render.meanAbsDiffTolerance, `mean abs diff ${diff}`);
  } finally {
    doc.close();
  }
});

test('a page past the end throws rather than crashing the process', () => {
  const doc = openDocument(fixture);
  try {
    assert.throws(() => doc.pageSize(doc.pageCount()), /past the end/);
  } finally {
    doc.close();
  }
});

test('garbage bytes throw', () => {
  assert.throws(() => openDocument(Buffer.from('not a pdf')), /PDFium can open/);
});

test('using a closed document throws', () => {
  const doc = openDocument(fixture);
  doc.close();
  doc.close();
  assert.throws(() => doc.pageCount(), /closed/);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && node --test packages/node/vectors.test.mjs`
Expected: FAIL, `ERR_MODULE_NOT_FOUND` on `./index.mjs`.

- [ ] **Step 3: Write the crate**

`crates/epdf-node/Cargo.toml`:

```toml
[package]
name = "epdf-node"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "epdf_node"
crate-type = ["cdylib"]

[dependencies]
epdf-ops = { path = "../epdf-ops" }
napi = { version = "=3.12.2", default-features = false, features = ["napi8"] }
napi-derive = "=3.6.3"

[build-dependencies]
napi-build = "=2.4.1"
build-support = { path = "../build-support" }
```

Add `"crates/epdf-node"` to the `members` array in `rust-core-scaffold/Cargo.toml`.

`crates/epdf-node/build.rs`:

```rust
fn main() {
    napi_build::setup();
    // Cargo does not propagate cargo:rustc-link-arg from a dependency's build
    // script, so the addon needs its own rpath to find libembedpdf at runtime.
    build_support::emit_rpath_only();
}
```

`crates/epdf-node/src/lib.rs`:

```rust
//! The Node adapter. Six methods, no pointers.
//!
//! Recommendation 4 of docs/research/pdfium-rust-wrapper-decision.md prefers f64
//! pointers to BigInt for a Node addon over PDFium. It is moot here: a coarse
//! per-operation boundary hands JavaScript no handles at all, so there is no
//! pointer representation to get wrong. That is a reason to prefer a coarse
//! boundary, not evidence the recommendation was unnecessary.
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;

fn to_napi(err: epdf_ops::OpsError) -> Error {
    Error::from_reason(err.to_string())
}

#[napi(object)]
pub struct JsSize {
    pub width: f64,
    pub height: f64,
}

#[napi(object)]
pub struct JsRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[napi(object)]
pub struct JsSearchHit {
    pub char_index: u32,
    pub char_count: u32,
    pub rects: Vec<JsRect>,
}

#[napi(object)]
pub struct JsBitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Buffer,
}

#[napi]
pub struct Document {
    inner: Arc<epdf_ops::Document>,
}

#[napi]
impl Document {
    #[napi]
    pub fn page_count(&self) -> Result<u32> {
        self.inner.page_count().map_err(to_napi)
    }

    #[napi]
    pub fn page_size(&self, index: u32) -> Result<JsSize> {
        let size = self.inner.page_size(index).map_err(to_napi)?;
        Ok(JsSize { width: size.width as f64, height: size.height as f64 })
    }

    #[napi]
    pub fn render_page(&self, index: u32, scale: f64) -> Result<JsBitmap> {
        let b = self.inner.render_page(index, scale as f32).map_err(to_napi)?;
        Ok(JsBitmap { width: b.width, height: b.height, stride: b.stride, bgra: b.bgra.into() })
    }

    #[napi]
    pub fn page_text(&self, index: u32) -> Result<String> {
        self.inner.page_text(index).map_err(to_napi)
    }

    #[napi]
    pub fn search(&self, index: u32, query: String, case_sensitive: bool) -> Result<Vec<JsSearchHit>> {
        Ok(self
            .inner
            .search(index, query, case_sensitive)
            .map_err(to_napi)?
            .into_iter()
            .map(|hit| JsSearchHit {
                char_index: hit.char_index,
                char_count: hit.char_count,
                rects: hit
                    .rects
                    .into_iter()
                    .map(|r| JsRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                    .collect(),
            })
            .collect())
    }

    #[napi]
    pub fn close(&self) {
        self.inner.close();
    }
}

/// One process-wide engine, and therefore one PDFium thread. A scaffold does
/// not need a pool, and a second engine would mean a second thread with its own
/// FPDF_InitLibrary, which is legal but pointless here.
fn engine() -> &'static Arc<epdf_ops::Engine> {
    static ENGINE: std::sync::OnceLock<Arc<epdf_ops::Engine>> = std::sync::OnceLock::new();
    ENGINE.get_or_init(|| epdf_ops::Engine::new().expect("PDFium engine"))
}

#[napi]
pub fn open_document(bytes: Buffer, password: Option<String>) -> Result<Document> {
    let inner = engine().open(bytes.to_vec(), password).map_err(to_napi)?;
    Ok(Document { inner })
}
```

- [ ] **Step 4: Write the build script and the package**

`rust-core-scaffold/build/build-node.sh`:

```bash
#!/usr/bin/env bash
# Builds the napi addon and drops it beside packages/node/index.mjs under the
# fork's target name, which is the same spelling packages/engine/runtime/npm
# uses. No @napi-rs/cli: a cdylib rename is the whole build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-$(node -p "process.platform === 'darwin' ? 'darwin' : process.platform === 'win32' ? 'win32' : 'linux'")-$(node -p "process.arch === 'arm64' ? 'arm64' : 'x64'")}"

bash "$ROOT/build/fetch-libpdfium.sh" "$TARGET"
( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$TARGET" cargo build --release -p epdf-node )

case "$(uname -s)" in
  Darwin) ext=dylib ;;
  Linux)  ext=so ;;
  *)      echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
esac

cp "$ROOT/target/release/libepdf_node.$ext" "$ROOT/packages/node/epdf-node.$TARGET.node"
echo "built packages/node/epdf-node.$TARGET.node"
```

`rust-core-scaffold/package.json`:

```json
{
  "name": "epdf-rust-core-scaffold",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "test": "node --test build/*.test.mjs vectors/*.test.mjs packages/node/*.test.mjs packages/web/*.test.mjs"
  }
}
```

`rust-core-scaffold/packages/node/package.json`:

```json
{
  "name": "@epdf-scaffold/node",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "main": "./index.mjs"
}
```

`rust-core-scaffold/packages/node/index.mjs`:

```js
/**
 * Loads the napi addon for this host, named the way packages/engine/runtime/npm
 * names its targets (Node's spelling: darwin-arm64, linux-x64, win32-x64).
 */
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const platform = { darwin: 'darwin', linux: 'linux', win32: 'win32' }[process.platform];
const arch = { arm64: 'arm64', x64: 'x64' }[process.arch];

if (!platform || !arch) {
  throw new Error(`unsupported host: ${process.platform}-${process.arch}`);
}

const addonPath = resolve(here, `epdf-node.${platform}-${arch}.node`);

if (!existsSync(addonPath)) {
  throw new Error(
    `no addon at ${addonPath}\nFix with: bash build/build-node.sh ${platform}-${arch}`,
  );
}

const addon = require(addonPath);

export const openDocument = addon.openDocument;
export const Document = addon.Document;
```

- [ ] **Step 5: Build and run the tests**

Run:
```bash
cd rust-core-scaffold
bash build/build-node.sh
node --test packages/node/vectors.test.mjs
```
Expected: PASS, 4 tests.

If the addon loads but every method is `undefined`, the napi export names are camelCased by `napi-derive` (`page_count` becomes `pageCount`). That is what the test expects. Do not rename the Rust methods to match; check the generated names with `node -p "Object.keys(require('./packages/node/epdf-node.darwin-arm64.node'))"`.

- [ ] **Step 6: Commit**

```bash
git add rust-core-scaffold/crates/epdf-node rust-core-scaffold/build/build-node.sh \
  rust-core-scaffold/package.json rust-core-scaffold/packages/node
git commit -m "feat(scaffold): add napi addon and node package verified against frozen vectors"
```

---

### Task 10: The Node CLI app

**Files:**
- Create: `rust-core-scaffold/apps/node-cli/package.json`
- Create: `rust-core-scaffold/apps/node-cli/main.mjs`
- Create: `rust-core-scaffold/apps/node-cli/bmp.mjs`
- Test: `rust-core-scaffold/apps/node-cli/main.test.mjs`

**Interfaces:**
- Consumes: `openDocument` from `packages/node/index.mjs`.
- Produces: `node apps/node-cli/main.mjs [--page 4] [--scale 1.0] [--out page.bmp]` printing a summary to stdout and writing a BMP.

BMP rather than PNG because `zlib` alone is not a PNG encoder and adding one is a dependency this scaffold does not need. `packages/engine/main/src/render/bmp.ts` is the same choice for the same reason.

- [ ] **Step 1: Write the failing test**

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { promisify } from 'node:util';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadVectors } from '../../packages/test-support/vectors.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const run = promisify(execFile);

const { vectors } = await loadVectors();

test('the CLI reports the frozen numbers and writes a readable BMP', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'epdf-cli-'));
  const out = join(dir, 'page.bmp');

  const { stdout } = await run('node', [
    resolve(here, 'main.mjs'),
    '--page', String(vectors.pageIndex),
    '--scale', String(vectors.render.scale),
    '--out', out,
  ]);

  assert.match(stdout, new RegExp(`pages: ${vectors.pageCount}\\b`));
  assert.match(stdout, new RegExp(`chars: ${vectors.charCount}\\b`));
  assert.match(stdout, new RegExp(`hits: ${vectors.search.hitCount}\\b`));
  assert.match(stdout, new RegExp(`${vectors.render.width}x${vectors.render.height}`));

  const bmp = await readFile(out);
  assert.equal(bmp.subarray(0, 2).toString('ascii'), 'BM');
  assert.equal(bmp.readUInt32LE(2), bmp.length);
  assert.equal(bmp.readInt32LE(18), vectors.render.width);
  assert.equal(bmp.readInt32LE(22), vectors.render.height);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && node --test apps/node-cli/main.test.mjs`
Expected: FAIL, the spawn of `main.mjs` errors with `Cannot find module`.

- [ ] **Step 3: Write the BMP encoder**

`apps/node-cli/bmp.mjs`:

```js
/**
 * A 32-bit BGRA BMP with a BITMAPINFOHEADER. PDFium's FPDFBitmap_BGRA byte
 * order is already what BMP wants, so the only work is the header and flipping
 * rows: BMP scanlines run bottom-up.
 */
export function encodeBmp({ width, height, stride, bgra }) {
  const headerSize = 14 + 40;
  const pixelBytes = width * height * 4;
  const out = Buffer.alloc(headerSize + pixelBytes);

  out.write('BM', 0, 'ascii');
  out.writeUInt32LE(out.length, 2);
  out.writeUInt32LE(headerSize, 10);

  out.writeUInt32LE(40, 14);
  out.writeInt32LE(width, 18);
  out.writeInt32LE(height, 22);
  out.writeUInt16LE(1, 26);
  out.writeUInt16LE(32, 28);
  out.writeUInt32LE(0, 30);
  out.writeUInt32LE(pixelBytes, 34);

  for (let y = 0; y < height; y += 1) {
    const source = (height - 1 - y) * stride;
    bgra.copy(out, headerSize + y * width * 4, source, source + width * 4);
  }

  return out;
}
```

- [ ] **Step 4: Write the CLI**

`apps/node-cli/main.mjs`:

```js
#!/usr/bin/env node
import { parseArgs } from 'node:util';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openDocument } from '../../packages/node/index.mjs';
import { encodeBmp } from './bmp.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const { values } = parseArgs({
  options: {
    page: { type: 'string', default: '4' },
    scale: { type: 'string', default: '1.0' },
    out: { type: 'string', default: resolve(process.cwd(), 'page.bmp') },
    pdf: { type: 'string', default: resolve(root, 'fixtures/report.pdf') },
    query: { type: 'string', default: 'the' },
  },
});

const pageIndex = Number(values.page);
const scale = Number(values.scale);

const doc = openDocument(await readFile(values.pdf));
try {
  const pageCount = doc.pageCount();
  const size = doc.pageSize(pageIndex);
  const text = doc.pageText(pageIndex);
  const hits = doc.search(pageIndex, values.query, false);
  const bitmap = doc.renderPage(pageIndex, scale);

  await writeFile(values.out, encodeBmp(bitmap));

  console.log(`pdf: ${values.pdf}`);
  console.log(`pages: ${pageCount}`);
  console.log(`page ${pageIndex}: ${size.width.toFixed(1)} x ${size.height.toFixed(1)} pt`);
  console.log(`chars: ${text.length}`);
  console.log(`hits: ${hits.length} for "${values.query}"`);
  console.log(`render: ${bitmap.width}x${bitmap.height} stride ${bitmap.stride} -> ${values.out}`);
  console.log(`text preview: ${JSON.stringify(text.slice(0, 120))}`);
} finally {
  doc.close();
}
```

`apps/node-cli/package.json`:

```json
{
  "name": "@epdf-scaffold/node-cli",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "bin": { "epdf-scaffold": "./main.mjs" }
}
```

- [ ] **Step 5: Run the test and the app**

Run:
```bash
cd rust-core-scaffold
node --test apps/node-cli/main.test.mjs
node apps/node-cli/main.mjs --page 4 --scale 1.0 --out /tmp/page4.bmp
open /tmp/page4.bmp
```
Expected: PASS, 1 test. The `open` shows page 4 of the report, right way up. An upside-down image means the row flip in `encodeBmp` is wrong, not the render.

- [ ] **Step 6: Commit**

```bash
git add rust-core-scaffold/apps/node-cli
git commit -m "feat(scaffold): add node CLI demo writing a BMP of page 4"
```

---

### Task 11: `epdf-wasm` and the web package

**Files:**
- Create: `rust-core-scaffold/crates/epdf-wasm/Cargo.toml`
- Create: `rust-core-scaffold/crates/epdf-wasm/src/lib.rs`
- Create: `rust-core-scaffold/build/link-wasm.sh`
- Create: `rust-core-scaffold/packages/web/package.json`
- Create: `rust-core-scaffold/packages/web/index.mjs`
- Modify: `rust-core-scaffold/Cargo.toml` (add `"crates/epdf-wasm"` to `members`)
- Test: `rust-core-scaffold/packages/web/vectors.test.mjs`

**Interfaces:**
- Consumes: `epdf_ops::{Engine, Document}` compiled for `wasm32-unknown-emscripten`, which uses the `inline` dispatch from Task 5.
- Produces: `build/out/web/epdf.mjs` plus `epdf.wasm` from one `em++` link, and `packages/web/index.mjs` exporting `createEpdf() -> Promise<{ openDocument(bytes) -> Document }>` with the same six methods as the Node package.

The C ABI the module exports:

```c
uint32_t epdf_open(const uint8_t* bytes, uint32_t len);         /* 0 on failure */
int32_t  epdf_page_count(uint32_t doc);                          /* -1 on failure */
uint8_t* epdf_json(uint32_t doc, uint32_t op, uint32_t index,
                   const uint8_t* arg, uint32_t arg_len,
                   uint32_t flag, uint32_t* out_len);             /* NULL on failure */
uint8_t* epdf_render(uint32_t doc, uint32_t index, float scale,
                     uint32_t* out_w, uint32_t* out_h, uint32_t* out_stride,
                     uint32_t* out_len);
void     epdf_free(uint8_t* ptr, uint32_t len);
void     epdf_close(uint32_t doc);
const char* epdf_last_error(void);
```

`epdf_json` covers `page_size` (op 0), `page_text` (op 1) and `search` (op 2), each returning UTF-8 JSON. JSON rather than a hand-packed struct because the layout would then have to agree in two languages, and `pdfium-rust-spike/crates/ops/src/record.rs:1-3` says out loud that its 112-byte record is duplicated in three places and only the fairness gate proves they agree. One crossing per operation is what recommendation 3 asks for; what rides on that crossing is free to be convenient. The render path stays raw, because a megabyte of base64 is not convenient.

- [ ] **Step 1: Write the failing test**

Create `rust-core-scaffold/packages/web/vectors.test.mjs`. It reads the frozen file through `../test-support/vectors.mjs`, the shared loader Task 9 created, and must not re-declare `loadVectors` or `meanAbsDiff`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loadVectors, meanAbsDiff } from '../test-support/vectors.mjs';
import { createEpdf } from './index.mjs';

const { vectors, golden, fixture } = await loadVectors();
const epdf = await createEpdf();
```

Then the same four tests as `packages/node/vectors.test.mjs`, asserting the same frozen numbers, with `epdf.openDocument(...)` in place of `openDocument(...)`. The four behaviours are: the six operations match the frozen vectors; a page past the end throws; garbage bytes throw; a closed document throws.

The two suites assert the same numbers through deliberately separate test bodies, because they exercise two different code paths (napi over a thread, and one em++ module over `inline` dispatch) and a shared body would hide a divergence in either.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust-core-scaffold && node --test packages/web/vectors.test.mjs`
Expected: FAIL, `ERR_MODULE_NOT_FOUND` on `./index.mjs`.

- [ ] **Step 3: Write the crate**

`crates/epdf-wasm/Cargo.toml`:

```toml
[package]
name = "epdf-wasm"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "epdf_wasm"
crate-type = ["staticlib", "rlib"]

[dependencies]
epdf-ops = { path = "../epdf-ops" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Add `"crates/epdf-wasm"` to the `members` array in `rust-core-scaffold/Cargo.toml`.

`crates/epdf-wasm/src/lib.rs`:

```rust
//! The browser adapter: one extern "C" entry point per operation, over the
//! `inline` dispatch epdf-ops compiles for emscripten.
//!
//! Everything except the render buffer crosses as UTF-8 JSON. A hand-packed
//! struct would have to agree in two languages, and
//! pdfium-rust-spike/crates/ops/src/record.rs:1-3 says out loud that its
//! 112-byte record is duplicated in three places with only a fairness gate
//! proving they match. One crossing per operation is what recommendation 3 of
//! docs/research/pdfium-rust-wrapper-decision.md asks for; what rides on that
//! crossing is free to be convenient. The render path stays raw, because a
//! megabyte of base64 is not convenient.
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

use serde::Serialize;

const OP_PAGE_SIZE: u32 = 0;
const OP_PAGE_TEXT: u32 = 1;
const OP_SEARCH: u32 = 2;

thread_local! {
    static ENGINE: RefCell<Option<Arc<epdf_ops::Engine>>> = const { RefCell::new(None) };
    static DOCS: RefCell<HashMap<u32, Arc<epdf_ops::Document>>> = RefCell::new(HashMap::new());
    static NEXT_ID: RefCell<u32> = const { RefCell::new(1) };
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("").unwrap());
}

fn set_error(message: impl std::fmt::Display) {
    let text = message.to_string().replace('\0', " ");
    LAST_ERROR.with(|cell| *cell.borrow_mut() = CString::new(text).unwrap());
}

fn engine() -> Result<Arc<epdf_ops::Engine>, epdf_ops::OpsError> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(epdf_ops::Engine::new()?);
        }
        Ok(Arc::clone(slot.as_ref().unwrap()))
    })
}

fn document(id: u32) -> Result<Arc<epdf_ops::Document>, epdf_ops::OpsError> {
    DOCS.with(|cell| cell.borrow().get(&id).cloned().ok_or(epdf_ops::OpsError::Closed))
}

/// Hands a buffer to JavaScript. `epdf_free` rebuilds the Vec with
/// `from_raw_parts(ptr, len, len)`, which is sound only when capacity equals
/// length, so shrink before leaking.
fn leak(mut bytes: Vec<u8>, out_len: *mut u32) -> *mut u8 {
    bytes.shrink_to_fit();
    debug_assert_eq!(bytes.len(), bytes.capacity());
    let len = bytes.len();
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    unsafe { *out_len = len as u32 };
    ptr
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSize {
    width: f32,
    height: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSearchHit {
    char_index: u32,
    char_count: u32,
    rects: Vec<JsonRect>,
}

#[no_mangle]
pub extern "C" fn epdf_open(bytes: *const u8, len: u32) -> u32 {
    let slice = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    let result = engine().and_then(|engine| engine.open(slice.to_vec(), None));
    match result {
        Ok(doc) => {
            let id = NEXT_ID.with(|cell| {
                let mut next = cell.borrow_mut();
                let id = *next;
                *next += 1;
                id
            });
            DOCS.with(|cell| cell.borrow_mut().insert(id, doc));
            id
        }
        Err(err) => {
            set_error(err);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn epdf_page_count(doc: u32) -> i32 {
    match document(doc).and_then(|d| d.page_count()) {
        Ok(count) => count as i32,
        Err(err) => {
            set_error(err);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn epdf_json(
    doc: u32,
    op: u32,
    index: u32,
    arg: *const u8,
    arg_len: u32,
    flag: u32,
    out_len: *mut u32,
) -> *mut u8 {
    let json = document(doc).and_then(|d| match op {
        OP_PAGE_SIZE => {
            let size = d.page_size(index)?;
            Ok(serde_json::to_vec(&JsonSize { width: size.width, height: size.height }).unwrap())
        }
        OP_PAGE_TEXT => Ok(serde_json::to_vec(&d.page_text(index)?).unwrap()),
        OP_SEARCH => {
            let query = unsafe { std::slice::from_raw_parts(arg, arg_len as usize) };
            let query = String::from_utf8_lossy(query).into_owned();
            let hits: Vec<JsonSearchHit> = d
                .search(index, query, flag != 0)?
                .into_iter()
                .map(|hit| JsonSearchHit {
                    char_index: hit.char_index,
                    char_count: hit.char_count,
                    rects: hit
                        .rects
                        .into_iter()
                        .map(|r| JsonRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                        .collect(),
                })
                .collect();
            Ok(serde_json::to_vec(&hits).unwrap())
        }
        _ => Err(epdf_ops::OpsError::Internal { message: format!("unknown op {op}") }),
    });

    match json {
        Ok(bytes) => leak(bytes, out_len),
        Err(err) => {
            set_error(err);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn epdf_render(
    doc: u32,
    index: u32,
    scale: f32,
    out_w: *mut u32,
    out_h: *mut u32,
    out_stride: *mut u32,
    out_len: *mut u32,
) -> *mut u8 {
    match document(doc).and_then(|d| d.render_page(index, scale)) {
        Ok(bitmap) => {
            unsafe {
                *out_w = bitmap.width;
                *out_h = bitmap.height;
                *out_stride = bitmap.stride;
            }
            leak(bitmap.bgra, out_len)
        }
        Err(err) => {
            set_error(err);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn epdf_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    unsafe { drop(Vec::from_raw_parts(ptr, len as usize, len as usize)) };
}

#[no_mangle]
pub extern "C" fn epdf_close(doc: u32) {
    DOCS.with(|cell| {
        if let Some(d) = cell.borrow_mut().remove(&doc) {
            d.close();
        }
    });
}

#[no_mangle]
pub extern "C" fn epdf_last_error() -> *const std::os::raw::c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}
```

`epdf_last_error` returns a pointer into a `thread_local` `CString`, so JavaScript must read it before the next failing call overwrites it. The glue in Step 5 reads it immediately, inside the same `if` that detected the failure.

- [ ] **Step 4: Write the link script**

`rust-core-scaffold/build/link-wasm.sh`, adapted from `pdfium-rust-spike/build/link-wasm.sh`:

```bash
#!/usr/bin/env bash
# One em++ invocation: libembedpdf.a plus the Rust staticlib, into one module.
# This is recommendation 5 of docs/research/pdfium-rust-wrapper-decision.md, and
# pdfium-rust-spike/build/link-wasm.sh proved it four times over.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPIKE="$ROOT/../pdfium-rust-spike"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out/web"

bash "$ROOT/build/fetch-libpdfium.sh" wasm32

if ! command -v em++ >/dev/null 2>&1; then
  if [[ -f "$SPIKE/build/emsdk.sh" ]]; then
    # shellcheck disable=SC1091
    source "$SPIKE/build/emsdk.sh"
  else
    echo "em++ not on PATH and no emsdk at $SPIKE/build/emsdk.sh" >&2
    exit 1
  fi
fi

# emsdk 3.1.72 bundles wasm-opt v119, which rejects two wasm feature names
# present in rustc 1.98.0's precompiled std. -O3 makes em++ run it, so a newer
# wasm-opt has to shadow the bundled one. pdfium-rust-spike/README.md documents
# the same substitution.
if ! wasm-opt --version 2>/dev/null | grep -qE 'version 1[2-9][0-9]'; then
  echo "warning: wasm-opt is older than v120; the -O3 link will fail." >&2
  echo "         install a newer binaryen and put it ahead of emsdk's on PATH." >&2
fi

rustup target add wasm32-unknown-emscripten
( cd "$ROOT" && cargo build --release -p epdf-wasm --target wasm32-unknown-emscripten )

EXPORTS="_malloc,_free\
,_epdf_open,_epdf_page_count,_epdf_json,_epdf_render,_epdf_free,_epdf_close,_epdf_last_error"

mkdir -p "$OUT_DIR"
em++ \
  "$LIB_DIR/lib/libembedpdf.a" \
  "$ROOT/target/wasm32-unknown-emscripten/release/libepdf_wasm.a" \
  -sENVIRONMENT=node,web,worker \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sWASM=1 \
  -sALLOW_MEMORY_GROWTH=1 \
  -sALLOW_TABLE_GROWTH=1 \
  -sEXPORT_NAME=createEpdfModule \
  -sASSERTIONS=1 \
  -O3 \
  -sEXPORTED_RUNTIME_METHODS=cwrap,getValue,setValue,UTF8ToString,HEAPU8 \
  -sEXPORTED_FUNCTIONS="$EXPORTS" \
  -I"$LIB_DIR/include" \
  -std=c++17 --no-entry \
  -o "$OUT_DIR/epdf.mjs"

echo "linked $OUT_DIR/epdf.wasm ($(wc -c < "$OUT_DIR/epdf.wasm") bytes)"
```

- [ ] **Step 5: Write the JS glue**

`packages/web/index.mjs`:

```js
/**
 * JS glue over the em++ module built by build/link-wasm.sh. Same six methods as
 * packages/node/index.mjs, so packages/web/vectors.test.mjs and
 * packages/node/vectors.test.mjs assert the same frozen numbers through the
 * same shapes.
 */
import createEpdfModule from '../../build/out/web/epdf.mjs';

const OP_PAGE_SIZE = 0;
const OP_PAGE_TEXT = 1;
const OP_SEARCH = 2;

export async function createEpdf() {
  const module = await createEpdfModule();

  const epdf_open = module.cwrap('epdf_open', 'number', ['number', 'number']);
  const epdf_page_count = module.cwrap('epdf_page_count', 'number', ['number']);
  const epdf_json = module.cwrap('epdf_json', 'number',
    ['number', 'number', 'number', 'number', 'number', 'number', 'number']);
  const epdf_render = module.cwrap('epdf_render', 'number',
    ['number', 'number', 'number', 'number', 'number', 'number', 'number']);
  const epdf_free = module.cwrap('epdf_free', null, ['number', 'number']);
  const epdf_close = module.cwrap('epdf_close', null, ['number']);
  const epdf_last_error = module.cwrap('epdf_last_error', 'number', []);

  /** Read the error string immediately: the next failing call overwrites it. */
  const fail = () => {
    throw new Error(module.UTF8ToString(epdf_last_error()));
  };

  /** Copy bytes into the module heap; the caller frees. */
  const push = (bytes) => {
    const ptr = module._malloc(bytes.length);
    module.HEAPU8.set(bytes, ptr);
    return ptr;
  };

  const readJson = (doc, op, index, argBytes, flag) => {
    const lenPtr = module._malloc(4);
    const argPtr = argBytes ? push(argBytes) : 0;
    let dataPtr = 0;
    try {
      dataPtr = epdf_json(doc, op, index, argPtr, argBytes ? argBytes.length : 0, flag, lenPtr);
      if (dataPtr === 0) fail();
      const len = module.HEAPU32[lenPtr >> 2];
      const text = new TextDecoder().decode(module.HEAPU8.subarray(dataPtr, dataPtr + len));
      return JSON.parse(text);
    } finally {
      if (dataPtr !== 0) epdf_free(dataPtr, module.HEAPU32[lenPtr >> 2]);
      if (argPtr !== 0) module._free(argPtr);
      module._free(lenPtr);
    }
  };

  class Document {
    #handle;

    constructor(handle) {
      this.#handle = handle;
    }

    pageCount() {
      const count = epdf_page_count(this.#handle);
      if (count < 0) fail();
      return count;
    }

    pageSize(index) {
      return readJson(this.#handle, OP_PAGE_SIZE, index, null, 0);
    }

    pageText(index) {
      return readJson(this.#handle, OP_PAGE_TEXT, index, null, 0);
    }

    search(index, query, caseSensitive) {
      return readJson(
        this.#handle,
        OP_SEARCH,
        index,
        new TextEncoder().encode(query),
        caseSensitive ? 1 : 0,
      );
    }

    renderPage(index, scale) {
      const meta = module._malloc(16); // w, h, stride, len
      let dataPtr = 0;
      try {
        dataPtr = epdf_render(this.#handle, index, scale, meta, meta + 4, meta + 8, meta + 12);
        if (dataPtr === 0) fail();
        const [width, height, stride, len] = [0, 1, 2, 3].map((i) => module.HEAPU32[(meta >> 2) + i]);
        // Copy out: the view would dangle after epdf_free, and would also be
        // invalidated by any heap growth.
        const bgra = new Uint8Array(module.HEAPU8.subarray(dataPtr, dataPtr + len));
        return { width, height, stride, bgra };
      } finally {
        if (dataPtr !== 0) epdf_free(dataPtr, module.HEAPU32[(meta >> 2) + 3]);
        module._free(meta);
      }
    }

    close() {
      if (this.#handle !== 0) {
        epdf_close(this.#handle);
        this.#handle = 0;
      }
    }
  }

  return {
    openDocument(bytes) {
      const ptr = push(bytes);
      try {
        const handle = epdf_open(ptr, bytes.length);
        if (handle === 0) fail();
        return new Document(handle);
      } finally {
        module._free(ptr);
      }
    },
  };
}
```

`pageText` returns a JSON string, so `readJson` gives back a JavaScript string directly. That is why `epdf_json`'s text op serializes the `String` rather than writing raw UTF-8: one decode path for all three ops.

`packages/web/package.json`:

```json
{
  "name": "@epdf-scaffold/web",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "main": "./index.mjs"
}
```

- [ ] **Step 6: Build and run the tests**

Run:
```bash
cd rust-core-scaffold
bash build/link-wasm.sh
node --test packages/web/vectors.test.mjs
```
Expected: `linked .../epdf.wasm (<n> bytes)` then PASS, 4 tests. The same frozen numbers as the Rust, Swift and Node suites.

If the link fails with `Unknown feature in feature section` or similar from `wasm-opt`, that is the emsdk v119 problem the script warns about. Install a newer binaryen; do not drop `-O3` to route around it, because that changes what is being built.

- [ ] **Step 7: Commit**

```bash
git add rust-core-scaffold/crates/epdf-wasm rust-core-scaffold/build/link-wasm.sh \
  rust-core-scaffold/packages/web
git commit -m "feat(scaffold): link rust into the emscripten module and verify in node"
```

---

### Task 12: The browser demo

**Files:**
- Create: `rust-core-scaffold/apps/web-demo/package.json`
- Create: `rust-core-scaffold/apps/web-demo/vite.config.mjs`
- Create: `rust-core-scaffold/apps/web-demo/index.html`
- Create: `rust-core-scaffold/apps/web-demo/src/main.js`
- Create: `rust-core-scaffold/apps/web-demo/public/report.pdf` (symlink or copy of `fixtures/report.pdf`)

**Interfaces:**
- Consumes: `createEpdf` from `packages/web/index.mjs` and the linked `build/out/web/epdf.wasm`.
- Produces: `npm run dev -C apps/web-demo` serving a page on port 5174 that shows the same five facts the other apps show, with the page drawn on a `<canvas>`.

Port 5174 because `ts-wasm-interop-poc` already claims 5171 through 5173.

- [ ] **Step 1: Write the page and the script**

`apps/web-demo/index.html`:

```html
<!doctype html>
<meta charset="utf-8" />
<title>EmbedPDF Rust core scaffold</title>
<style>
  body { font: 14px/1.5 system-ui, sans-serif; margin: 2rem; max-width: 60rem; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 0.25rem 1rem; }
  dt { font-weight: 600; }
  canvas { border: 1px solid #ccc; display: block; margin: 1rem 0; }
  pre { white-space: pre-wrap; font-size: 12px; background: #f6f6f6; padding: 1rem; }
</style>
<main>
  <h1>report.pdf page 4, through Rust in wasm</h1>
  <dl>
    <dt>Pages</dt><dd id="pages">…</dd>
    <dt>Page 4</dt><dd id="size">…</dd>
    <dt>Characters</dt><dd id="chars">…</dd>
    <dt>Hits for "the"</dt><dd id="hits">…</dd>
  </dl>
  <canvas id="page"></canvas>
  <pre id="text"></pre>
</main>
<script type="module" src="/src/main.js"></script>
```

`apps/web-demo/src/main.js`:

```js
import { createEpdf } from '../../../packages/web/index.mjs';

const PAGE_INDEX = 4;
const SCALE = 2;

const set = (id, value) => { document.getElementById(id).textContent = String(value); };

const epdf = await createEpdf();
const bytes = new Uint8Array(await (await fetch('/report.pdf')).arrayBuffer());
const doc = epdf.openDocument(bytes);

set('pages', doc.pageCount());
const size = doc.pageSize(PAGE_INDEX);
set('size', `${size.width.toFixed(1)} x ${size.height.toFixed(1)} pt`);
const text = doc.pageText(PAGE_INDEX);
set('chars', text.length);
set('hits', doc.search(PAGE_INDEX, 'the', false).length);
document.getElementById('text').textContent = text.slice(0, 2000);

const bitmap = doc.renderPage(PAGE_INDEX, SCALE);
const canvas = document.getElementById('page');
canvas.width = bitmap.width;
canvas.height = bitmap.height;
canvas.style.width = `${bitmap.width / SCALE}px`;

// PDFium gives BGRA; ImageData wants RGBA. Swap in place, one pass.
const rgba = new Uint8ClampedArray(bitmap.bgra);
for (let i = 0; i < rgba.length; i += 4) {
  const b = rgba[i];
  rgba[i] = rgba[i + 2];
  rgba[i + 2] = b;
}
canvas.getContext('2d').putImageData(new ImageData(rgba, bitmap.width, bitmap.height), 0, 0);
```

`apps/web-demo/vite.config.mjs`:

```js
import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 5174,
    // packages/web and build/out/web both live outside this app's root, and
    // Vite refuses to serve outside it without this.
    fs: { allow: ['../..'] },
  },
  optimizeDeps: { exclude: ['@epdf-scaffold/web'] },
});
```

`apps/web-demo/package.json`:

```json
{
  "name": "@epdf-scaffold/web-demo",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  },
  "devDependencies": {
    "vite": "^6.3.5"
  }
}
```

Vite 6.3.5 matches the root `package.json`'s pinned `vite` devDependency, so the scaffold does not introduce a second major version into the repo.

- [ ] **Step 2: Install and run**

Run:
```bash
cd rust-core-scaffold/apps/web-demo
cp ../../fixtures/report.pdf public/report.pdf
npm install
npm run dev
```
Expected: Vite serves `http://localhost:5174/`. Open it and confirm the page shows the same page count, page-4 size, character count and hit count the Node CLI printed in Task 10, and that the canvas shows page 4.

- [ ] **Step 3: Check the console**

In the browser devtools console, expect zero errors. A `SharedArrayBuffer` or cross-origin isolation error means the em++ link picked up pthreads, which it should not: `build/link-wasm.sh` passes no `-pthread`.

- [ ] **Step 4: Commit**

```bash
git add rust-core-scaffold/apps/web-demo
git commit -m "feat(scaffold): add browser demo drawing page 4 on a canvas"
```

---

### Task 13: The Kotlin package, verified on JVM desktop

**Files:**
- Create: `rust-core-scaffold/build/build-kotlin.sh`
- Create: `rust-core-scaffold/packages/kotlin/settings.gradle.kts`
- Create: `rust-core-scaffold/packages/kotlin/gradle/libs.versions.toml`
- Create: `rust-core-scaffold/packages/kotlin/epdf-ops/build.gradle.kts`
- Create: `rust-core-scaffold/packages/kotlin/epdf-ops/src/main/kotlin/com/embedpdf/scaffold/Epdf.kt`
- Test: `rust-core-scaffold/packages/kotlin/epdf-ops/src/test/kotlin/com/embedpdf/scaffold/VectorsTest.kt`

**Interfaces:**
- Consumes: the same `epdf-uniffi` crate as Task 6, and the `uniffi-bindgen` binary from Task 6, with `--language kotlin`.
- Produces: a Gradle library module `:epdf-ops` whose Kotlin API is the generated `EpdfEngine`, `EpdfDocument`, `Size`, `Rect`, `SearchHit`, `Bitmap` and `OpsException`, plus one handwritten helper `EpdfDocument.renderPageArgb(index: UInt, scale: Float): IntArray` for Compose.

This task is verifiable on this machine only after a JDK is installed. It does not need any iOS or Android artifact: the JVM desktop tests load `libepdf_uniffi.dylib` built for `aarch64-apple-darwin`, which is why the Kotlin binding can be proven before the fork ships a mobile target.

- [ ] **Step 1: Install the JDK and Gradle**

`/usr/libexec/java_home` currently fails on this machine, so:

```bash
brew install openjdk@21 gradle
sudo ln -sfn /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk /Library/Java/JavaVirtualMachines/openjdk-21.jdk
/usr/libexec/java_home -V
gradle --version
```
Expected: `java_home -V` lists a 21 JDK and `gradle --version` prints. Record both versions for the README in Task 15.

- [ ] **Step 2: Write the failing test**

`packages/kotlin/epdf-ops/src/test/kotlin/com/embedpdf/scaffold/VectorsTest.kt`:

```kotlin
package com.embedpdf.scaffold

import java.io.File
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith
import org.json.JSONObject

/**
 * Reads rust-core-scaffold/vectors/report-page4.json, the same frozen file the
 * Rust, Swift and JS suites read. `epdf.scaffold.root` is set by the Gradle
 * test task.
 */
private val root = File(System.getProperty("epdf.scaffold.root"))
private val vectors = JSONObject(File(root, "vectors/report-page4.json").readText())
private val render = vectors.getJSONObject("render")
private val golden = File(root, "vectors/${render.getString("golden")}").readBytes()
private val fixture = File(root, "fixtures/report.pdf").readBytes()

private fun meanAbsDiff(a: ByteArray, b: ByteArray): Double {
    assertEquals(a.size, b.size, "buffer lengths differ")
    var total = 0L
    for (i in a.indices) {
        total += abs((a[i].toInt() and 0xFF) - (b[i].toInt() and 0xFF)).toLong()
    }
    return total.toDouble() / a.size
}

class VectorsTest {
    private fun open(): EpdfDocument = EpdfEngine().open(fixture, null)

    @Test
    fun matchesTheFrozenVectors() {
        val pageIndex = vectors.getInt("pageIndex").toUInt()
        val doc = open()
        try {
            assertEquals(vectors.getInt("pageCount").toUInt(), doc.pageCount())

            val size = doc.pageSize(pageIndex)
            val want = vectors.getJSONObject("size")
            assertTrue(abs(size.width - want.getDouble("width").toFloat()) < 0.001f)
            assertTrue(abs(size.height - want.getDouble("height").toFloat()) < 0.001f)

            assertEquals(vectors.getString("text"), doc.pageText(pageIndex))

            val search = vectors.getJSONObject("search")
            val hits = doc.search(pageIndex, search.getString("query"), search.getBoolean("caseSensitive"))
            assertEquals(search.getInt("hitCount"), hits.size)

            val bitmap = doc.renderPage(pageIndex, render.getDouble("scale").toFloat())
            assertEquals(render.getInt("width").toUInt(), bitmap.width)
            assertEquals(render.getInt("height").toUInt(), bitmap.height)
            assertEquals(render.getInt("stride").toUInt(), bitmap.stride)
            val diff = meanAbsDiff(bitmap.bgra, golden)
            assertTrue(diff <= render.getDouble("meanAbsDiffTolerance"), "mean abs diff $diff")
        } finally {
            doc.close()
        }
    }

    @Test
    fun aPagePastTheEndThrows() {
        val doc = open()
        try {
            assertFailsWith<OpsException.PageOutOfRange> { doc.pageSize(doc.pageCount()) }
        } finally {
            doc.close()
        }
    }

    @Test
    fun garbageBytesThrow() {
        assertFailsWith<OpsException.LoadFailed> {
            EpdfEngine().open("not a pdf".toByteArray(), null)
        }
    }

    @Test
    fun rendersAnArgbIntArray() {
        val pageIndex = vectors.getInt("pageIndex").toUInt()
        val doc = open()
        try {
            val image = doc.renderPageArgb(pageIndex, render.getDouble("scale").toFloat())
            assertEquals(render.getInt("width"), image.width)
            assertEquals(render.getInt("height"), image.height)
            assertEquals(image.width * image.height, image.pixels.size)
        } finally {
            doc.close()
        }
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd rust-core-scaffold/packages/kotlin && gradle :epdf-ops:test`
Expected: FAIL, `Project directory ... is not part of the build` or a missing `settings.gradle.kts`.

- [ ] **Step 4: Write the Gradle build**

`packages/kotlin/settings.gradle.kts`:

```kotlin
rootProject.name = "epdf-scaffold-kotlin"
include(":epdf-ops")
```

`packages/kotlin/gradle/libs.versions.toml`:

```toml
[versions]
kotlin = "2.1.0"
jna = "5.14.0"
json = "20240303"

[libraries]
jna = { module = "net.java.dev.jna:jna", version.ref = "jna" }
json = { module = "org.json:json", version.ref = "json" }
```

Before writing these, run `gradle --version` and pick the Kotlin plugin version that Gradle release supports; do not keep `2.1.0` if it is incompatible. uniffi's generated Kotlin needs JNA at runtime, which is why it is an `api` dependency rather than `implementation`.

`packages/kotlin/epdf-ops/build.gradle.kts`:

```kotlin
plugins {
    kotlin("jvm") version "2.1.0"
}

val scaffoldRoot = file("../../..").canonicalPath
val hostLibs = file("$scaffoldRoot/packages/kotlin/epdf-ops/libs/darwin-aarch64")

kotlin { jvmToolchain(21) }

sourceSets {
    main {
        kotlin.srcDirs("src/main/kotlin", "src/main/kotlin/generated")
    }
}

dependencies {
    api(libs.jna)
    testImplementation(kotlin("test"))
    testImplementation(libs.json)
}

tasks.test {
    useJUnitPlatform()
    // JNA finds libepdf_uniffi here; libembedpdf resolves beside it through the
    // @loader_path rpath build/build-kotlin.sh installs.
    systemProperty("jna.library.path", hostLibs.absolutePath)
    systemProperty("epdf.scaffold.root", scaffoldRoot)
    doFirst {
        require(File(hostLibs, "libepdf_uniffi.dylib").exists()) {
            "no host library at $hostLibs. Fix with: bash build/build-kotlin.sh"
        }
    }
}
```

- [ ] **Step 5: Write the Kotlin helper**

`packages/kotlin/epdf-ops/src/main/kotlin/com/embedpdf/scaffold/Epdf.kt`:

```kotlin
package com.embedpdf.scaffold

/** ARGB pixels with the dimensions epdf-ops actually rendered at. */
data class ArgbImage(val pixels: IntArray, val width: Int, val height: Int)

/**
 * The whole Kotlin-side addition to what uniffi generates: BGRA bytes to an
 * ARGB IntArray, which is what android.graphics.Bitmap.createBitmap and
 * Compose's ImageBitmap both want.
 *
 * It returns the width and height rather than letting the caller recompute
 * them from `page_size * scale`. A second rounding rule is the one thing Task 5
 * warns against: epdf-ops uses max(1, round(points * scale)) and nothing else
 * may guess at it.
 */
fun EpdfDocument.renderPageArgb(index: UInt, scale: Float): ArgbImage {
    val bitmap = renderPage(index, scale)
    val width = bitmap.width.toInt()
    val height = bitmap.height.toInt()
    val stride = bitmap.stride.toInt()
    val out = IntArray(width * height)

    for (y in 0 until height) {
        var source = y * stride
        var target = y * width
        for (x in 0 until width) {
            val b = bitmap.bgra[source].toInt() and 0xFF
            val g = bitmap.bgra[source + 1].toInt() and 0xFF
            val r = bitmap.bgra[source + 2].toInt() and 0xFF
            val a = bitmap.bgra[source + 3].toInt() and 0xFF
            out[target] = (a shl 24) or (r shl 16) or (g shl 8) or b
            source += 4
            target += 1
        }
    }
    return ArgbImage(out, width, height)
}
```

- [ ] **Step 6: Write build-kotlin.sh**

```bash
#!/usr/bin/env bash
# Builds libepdf_uniffi for the JVM host and, when the fork ships them, for the
# Android ABIs, then generates the Kotlin bindings.
#
#   bash build/build-kotlin.sh                 # host JVM (darwin-aarch64)
#   bash build/build-kotlin.sh android-arm64   # exits 2 until the fork builds it
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICE="${1:-host}"
MODULE="$ROOT/packages/kotlin/epdf-ops"
GENERATED="$MODULE/src/main/kotlin/generated"

case "$SLICE" in
  host)
    RUST_TARGET=aarch64-apple-darwin
    FORK_TARGET=darwin-arm64
    DEST="$MODULE/libs/darwin-aarch64"
    ;;
  android-arm64)
    RUST_TARGET=aarch64-linux-android
    FORK_TARGET=android-arm64
    DEST="$MODULE/src/main/jniLibs/arm64-v8a"
    ;;
  android-x64)
    RUST_TARGET=x86_64-linux-android
    FORK_TARGET=android-x64
    DEST="$MODULE/src/main/jniLibs/x86_64"
    ;;
  *)
    echo "unknown slice: $SLICE (known: host, android-arm64, android-x64)" >&2
    exit 1
    ;;
esac

# Exits 2 with its own message for the pending Android targets.
bash "$ROOT/build/fetch-libpdfium.sh" "$FORK_TARGET"

rustup target add "$RUST_TARGET"
mkdir -p "$DEST"

if [[ "$SLICE" == host ]]; then
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$FORK_TARGET" cargo build --release -p epdf-uniffi --target "$RUST_TARGET" )
  cp "$ROOT/target/$RUST_TARGET/release/libepdf_uniffi.dylib" "$DEST/"
  cp "$ROOT/build/libpdfium/$FORK_TARGET/lib/libembedpdf."* "$DEST/"
  # JNA dlopens libepdf_uniffi from an absolute path, so @loader_path resolves
  # to this directory and libembedpdf is found beside it. Check the actual
  # install name first: if it is absolute rather than @rpath-relative, add an
  # install_name_tool -change line for it.
  otool -L "$DEST/libepdf_uniffi.dylib"
  install_name_tool -add_rpath "@loader_path" "$DEST/libepdf_uniffi.dylib"
else
  cargo install cargo-ndk --version 4.1.2 --locked
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$FORK_TARGET" cargo ndk -t "$RUST_TARGET" build --release -p epdf-uniffi )
  cp "$ROOT/target/$RUST_TARGET/release/libepdf_uniffi.so" "$DEST/"
  cp "$ROOT/build/libpdfium/$FORK_TARGET/lib/libembedpdf.so" "$DEST/"
fi

mkdir -p "$GENERATED"
LIBRARY="$DEST/libepdf_uniffi.dylib"
[[ -f "$LIBRARY" ]] || LIBRARY="$DEST/libepdf_uniffi.so"
( cd "$ROOT" && cargo run --release --bin uniffi-bindgen -- generate \
    --library "$LIBRARY" --language kotlin --out-dir "$GENERATED" )

echo "staged $SLICE -> $DEST; bindings -> $GENERATED"
```

Then `chmod +x rust-core-scaffold/build/build-kotlin.sh`.

- [ ] **Step 7: Generate the Gradle wrapper, build, and run the tests**

Run:
```bash
cd rust-core-scaffold/packages/kotlin
gradle wrapper --gradle-version 8.14
cd ../..
bash build/build-kotlin.sh
cd packages/kotlin && ./gradlew :epdf-ops:test
```
Expected: PASS, 4 tests. Commit the generated `gradlew`, `gradlew.bat` and `gradle/wrapper/` so the build does not need a system Gradle again.

If JNA cannot find the library, print `java.library.path` and `jna.library.path` from a scratch test before changing anything: the two are different properties and only the second one this build sets.

- [ ] **Step 8: Confirm the Android slice fails for the documented reason**

Run: `cd rust-core-scaffold && bash build/build-kotlin.sh android-arm64`
Expected: exit 2, `no libembedpdf for android-arm64: embedpdf/runtime does not build it yet.`

- [ ] **Step 9: Commit**

```bash
git add rust-core-scaffold/build/build-kotlin.sh rust-core-scaffold/packages/kotlin
git commit -m "feat(scaffold): add kotlin package verified against frozen vectors on the jvm"
```

---

### Task 14: The Android app

**Files:**
- Create: `rust-core-scaffold/apps/android/settings.gradle.kts`
- Create: `rust-core-scaffold/apps/android/app/build.gradle.kts`
- Create: `rust-core-scaffold/apps/android/app/src/main/AndroidManifest.xml`
- Create: `rust-core-scaffold/apps/android/app/src/main/assets/report.pdf` (copy of `fixtures/report.pdf`)
- Create: `rust-core-scaffold/apps/android/app/src/main/kotlin/com/embedpdf/scaffold/app/MainActivity.kt`
- Create: `rust-core-scaffold/apps/android/app/src/main/kotlin/com/embedpdf/scaffold/app/DocumentViewModel.kt`

**Interfaces:**
- Consumes: the `:epdf-ops` Gradle module from Task 13 by composite build, and `report.pdf` from the app's assets.
- Produces: a Compose app showing the same five facts. It compiles today and cannot run until `android-arm64` exists.

This is the one task that cannot be finished on this machine. There is no Android SDK, no NDK, no `adb`. The deliverable is therefore split: Kotlin compiles, and the packaging step fails with the pending-artifact message rather than silently producing an APK that crashes on launch.

- [ ] **Step 1: Install the Android SDK and NDK**

Run:
```bash
brew install --cask android-commandlinetools
sdkmanager --install "platforms;android-35" "build-tools;35.0.0" "platform-tools" "ndk;27.2.12479018"
sdkmanager --licenses
```
Expected: `sdkmanager --list_installed` shows all four. Record the exact versions for the README in Task 15. If a newer NDK or platform is current when you run this, use it and record what you used.

- [ ] **Step 2: Write the Gradle build with an explicit native-library gate**

`apps/android/settings.gradle.kts`:

```kotlin
rootProject.name = "epdf-scaffold-android"
include(":app")
includeBuild("../../packages/kotlin")
```

`apps/android/app/build.gradle.kts`:

```kotlin
plugins {
    id("com.android.application") version "8.7.3"
    kotlin("android") version "2.1.0"
}

android {
    namespace = "com.embedpdf.scaffold.app"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.embedpdf.scaffold.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }
    buildFeatures { compose = true }
    sourceSets["main"].java.srcDirs("src/main/kotlin")
}

dependencies {
    implementation("com.embedpdf.scaffold:epdf-ops")
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
}

/**
 * Fails the build with the same message build/fetch-libpdfium.sh prints, rather
 * than packaging an APK with no native library that would crash on launch. This
 * task goes away when embedpdf/runtime ships android-arm64.
 */
val requireNativeLibraries by tasks.registering {
    doFirst {
        val so = file("../../../packages/kotlin/epdf-ops/src/main/jniLibs/arm64-v8a/libepdf_uniffi.so")
        require(so.exists()) {
            """
            no libepdf_uniffi.so for arm64-v8a at ${so.path}

            embedpdf/runtime does not build android-arm64 yet. That is
            recommendation 6 of docs/research/pdfium-rust-wrapper-decision.md and
            has its own design doc. Until it lands this app cannot be packaged.

            Everything else in the scaffold still works:
              bash build/test-swift.sh
              node --test packages/node/vectors.test.mjs
              node --test packages/web/vectors.test.mjs
              cd packages/kotlin && ./gradlew :epdf-ops:test
            """.trimIndent()
        }
    }
}

tasks.named("preBuild") { dependsOn(requireNativeLibraries) }
```

Pin the Android Gradle Plugin and Compose BOM to whatever is current when you run Step 1; the versions above are a starting point, not a claim about what is latest.

- [ ] **Step 3: Write the view model and the activity**

`app/src/main/kotlin/com/embedpdf/scaffold/app/DocumentViewModel.kt`:

```kotlin
package com.embedpdf.scaffold.app

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.embedpdf.scaffold.EpdfDocument
import com.embedpdf.scaffold.EpdfEngine
import com.embedpdf.scaffold.renderPageArgb
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Field for field the same state as apps/ios/EpdfScaffold/DocumentModel.swift. */
class DocumentViewModel(app: Application) : AndroidViewModel(app) {
    var pageCount by mutableStateOf(0u)
        private set
    var sizeLabel by mutableStateOf("")
        private set
    var bitmap by mutableStateOf<Bitmap?>(null)
        private set
    var textPreview by mutableStateOf("")
        private set
    var searchHitCount by mutableStateOf(0)
        private set
    var failure by mutableStateOf<String?>(null)
        private set

    private val pageIndex = 4u
    private var document: EpdfDocument? = null

    init {
        load()
    }

    private fun load() = viewModelScope.launch {
        try {
            withContext(Dispatchers.IO) {
                val bytes = getApplication<Application>().assets.open("report.pdf").use { it.readBytes() }
                val doc = EpdfEngine().open(bytes, null)
                document = doc

                val count = doc.pageCount()
                val size = doc.pageSize(pageIndex)
                val text = doc.pageText(pageIndex)
                val hits = doc.search(pageIndex, "the", false).size
                val image = doc.renderPageArgb(pageIndex, 2.0f)
                val rendered = Bitmap.createBitmap(
                    image.pixels,
                    image.width,
                    image.height,
                    Bitmap.Config.ARGB_8888,
                )

                withContext(Dispatchers.Main) {
                    pageCount = count
                    sizeLabel = "%.1f x %.1f pt".format(size.width, size.height)
                    textPreview = text.take(600)
                    searchHitCount = hits
                    bitmap = rendered
                }
            }
        } catch (error: Throwable) {
            failure = error.toString()
        }
    }

    override fun onCleared() {
        document?.close()
        document = null
    }
}
```

`renderPageArgb` returns `ArgbImage(pixels, width, height)` from Task 13, so nothing here recomputes the render size from `page_size * scale`. That second rounding rule is exactly what Task 5 warns against.

`app/src/main/kotlin/com/embedpdf/scaffold/app/MainActivity.kt`:

```kotlin
package com.embedpdf.scaffold.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    private val model: DocumentViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Column(
                    Modifier.verticalScroll(rememberScrollState()).padding(16.dp)
                ) {
                    model.failure?.let {
                        Text(it, color = MaterialTheme.colorScheme.error, fontFamily = FontFamily.Monospace)
                    }
                    Text("Pages: ${model.pageCount}")
                    Text("Page 4: ${model.sizeLabel}")
                    Text("Hits for \"the\": ${model.searchHitCount}")
                    model.bitmap?.let {
                        Image(it.asImageBitmap(), contentDescription = "page 4", modifier = Modifier.fillMaxWidth())
                    }
                    Text(model.textPreview, fontFamily = FontFamily.Monospace)
                }
            }
        }
    }
}
```

`app/src/main/AndroidManifest.xml` declares one `<activity android:name=".MainActivity" android:exported="true">` with the `MAIN`/`LAUNCHER` intent filter, and no permissions: the fixture is an asset, so the app needs neither network nor storage access.

- [ ] **Step 4: Confirm Kotlin compiles**

Run:
```bash
cd rust-core-scaffold/apps/android
cp ../../fixtures/report.pdf app/src/main/assets/report.pdf
gradle wrapper --gradle-version 8.14
./gradlew :app:compileDebugKotlin
```
Expected: `BUILD SUCCESSFUL`. Compiling Kotlin needs the generated bindings from Task 13 but no `.so`.

- [ ] **Step 5: Confirm packaging fails for the documented reason**

Run: `cd rust-core-scaffold/apps/android && ./gradlew :app:assembleDebug`
Expected: `BUILD FAILED` with the `no libepdf_uniffi.so for arm64-v8a` message and the four commands that do work. This is the deliverable. Do not delete the gate to get a green build.

- [ ] **Step 6: Commit**

```bash
git add rust-core-scaffold/apps/android
git commit -m "feat(scaffold): add compose demo app gated on the pending android artifact"
```

---

### Task 15: README and the verification sweep

**Files:**
- Create: `rust-core-scaffold/README.md`
- Modify: `docs/superpowers/specs/2026-09-03-rust-core-scaffold-design.md` (correct the crate count in the layout tree)
- Modify: `docs/research/rust-core-port.md` (one sentence in the wave-3 paragraph pointing at the scaffold)

**Interfaces:**
- Consumes: every artifact and every test command from Tasks 1 through 14.
- Produces: a README whose first paragraph states the gate, and a single command that runs every suite that can run.

- [ ] **Step 1: Add the aggregate test script**

Modify `rust-core-scaffold/package.json`, replacing the `scripts` block:

```json
  "scripts": {
    "test": "node --test build/*.test.mjs vectors/*.test.mjs packages/node/*.test.mjs packages/web/*.test.mjs apps/node-cli/*.test.mjs",
    "_note": "packages/test-support holds no tests of its own; it is the shared loader the four suites above read.",
    "test:all": "bash build/test-all.sh"
  }
```

Create `rust-core-scaffold/build/test-all.sh`:

```bash
#!/usr/bin/env bash
# Every suite that can run without an iOS or Android artifact. Fails on the
# first one that fails, and prints what it skipped.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== cargo =="
cargo test --workspace

echo "== node and web =="
npm test

echo "== swift (macOS) =="
bash build/test-swift.sh

echo "== kotlin (jvm desktop) =="
if command -v java >/dev/null 2>&1; then
  ( cd packages/kotlin && ./gradlew :epdf-ops:test )
else
  echo "skipped: no JDK. Fix with: brew install openjdk@21" >&2
fi

cat <<'MSG'

Not run, and why:
  apps/ios on an iOS destination   embedpdf/runtime does not build ios-arm64
  apps/android packaging           embedpdf/runtime does not build android-arm64
MSG
```

- [ ] **Step 2: Write the README**

`rust-core-scaffold/README.md` must open by stating the gate, then cover, in this order: what the directory is and that deleting it costs nothing; the six operations and why they are coarse, linking `docs/research/pdfium-rust-wrapper-decision.md`; the crate layering; the thread-confinement constraint and why `Engine` owns a thread, citing `runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77`; the frozen vectors mechanism and that `build/generate-vectors.mjs` is never run by a test; a table of every host with its build command, its test command, and whether it runs today; and the exact tool versions recorded in Tasks 8, 13 and 14 (XcodeGen, JDK, Gradle, Android SDK, NDK).

House voice: no em dashes, exact versions and paths, receipts for every claim, prose over bullets except where the list is genuinely enumerable. The one table above is enumerable and earns its place.

- [ ] **Step 3: Correct the spec's layout tree**

The spec at `docs/superpowers/specs/2026-09-03-rust-core-scaffold-design.md` lists five crates and puts uniffi inside `epdf-ops`. The implementation has seven: `pdfium-sys`, `pdfium`, `epdf-ops`, `epdf-uniffi`, `epdf-node`, `epdf-wasm`, `uniffi-bindgen`. Update the tree and add one sentence saying uniffi got its own crate so `epdf-ops` never has to compile uniffi's scaffolding for `wasm32-unknown-emscripten`.

- [ ] **Step 4: Add the cross-reference to the port plan**

In `docs/research/rust-core-port.md`, in the paragraph beginning "Wave 3 is the Swift and Kotlin bindings", add one sentence naming `rust-core-scaffold/` as the place where the binding surface was scaffolded and the two mobile targets it is waiting on. One sentence, in that document's own voice, no restructuring.

- [ ] **Step 5: Run the full sweep**

Run:
```bash
cd rust-core-scaffold
bash build/test-all.sh
```
Expected, in order: `cargo test --workspace` passing across `pdfium-sys`, `pdfium`, `epdf-ops` and `epdf-uniffi`; `npm test` passing the vectors, node, web and CLI suites; `bash build/test-swift.sh` passing 3 tests; `./gradlew :epdf-ops:test` passing 4 tests; then the two-line list of what was not run.

Record the actual counts. Do not write "all tests pass" anywhere: write the numbers.

- [ ] **Step 6: Commit**

```bash
git add rust-core-scaffold/README.md rust-core-scaffold/package.json rust-core-scaffold/build/test-all.sh \
  docs/superpowers/specs/2026-09-03-rust-core-scaffold-design.md docs/research/rust-core-port.md
git commit -m "docs(scaffold): add README and wire the scaffold into the port plan"
```
