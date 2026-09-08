# Fork Build Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `ios-arm64`, `ios-sim-arm64`, `android-arm64`, `android-x64` and `wasm32-eh` to the embedpdf/runtime build matrix, and consume them from `pdfium-multiplatform-prototype`.

**Architecture:** Every target flows through the same five scripts in `scripts/embedpdf-runtime/`: `ensure-deps.sh` syncs, `apply-patches.sh` patches, `build-target.sh` writes `args.gn` and runs ninja, `package-target.sh` tars, `write-manifest.sh` publishes. Adding a target means a case in each place that needs one, plus a matrix row in `.github/workflows/release-libpdfium.yml`. Task 2 first extracts the target-to-gn-args mapping into a pure script so every later target can be verified in milliseconds instead of hours.

**Tech Stack:** GN + ninja, depot_tools/gclient, emsdk 3.1.72, Android NDK (via gclient `target_os = [ "android" ]`), Xcode for iOS, bash.

## Global Constraints

- Two repos. Fork changes commit to the submodule at `packages/engine/runtime/runtime-src` (remote `https://github.com/embedpdf/runtime.git`), on a new branch `embedpdf/feature/mobile-and-wasm-eh` cut from `fce2b000cc7a9e2cabc92db41cd57ca03233f07d`. Consumer changes commit to `embed-pdf-viewer` on `research/runtime-fork-build-matrix`. Never mix the two in one commit.
- All fourteen artifacts (nine existing plus five new) are cut from ONE fork sha. Task 9 re-pins all fourteen entries in `pdfium-multiplatform-prototype/build/runtime-build.json`, not just the four currently marked `"status": "pending"`.
- `wasm32` output must stay byte-identical. Any change that alters the existing `wasm32` `args.gn` is a defect.
- New target names, exactly: `wasm32-eh`, `ios-arm64`, `ios-sim-arm64`, `android-arm64`, `android-x64`. These are the strings already used by `pdfium-multiplatform-prototype/build/build-swift.sh:20-21`, `build-kotlin.sh:25-33` and the four `"status": "pending"` keys in `pdfium-multiplatform-prototype/build/runtime-build.json`.
- iOS AND Android artifacts are both static (`pdf_is_complete_lib=true`, staged as `lib/libembedpdf.a`). Android was specified as shared and that was wrong: Chromium's Android config exports nothing from a `.so`, so `--gc-sections` strips all of PDFium and the build silently produces an empty library that still passes a `file` check. Static also means `jniLibs` ships one `.so` (the Rust `cdylib`) instead of two.
- `embedpdf_thread_local_globals` must be `false` for every wasm target and `true` for every other target.
- Every fork script keeps its existing header: `# Copyright 2026 CloudPDF LTD` then `# SPDX-License-Identifier: Apache-2.0`.
- Do not run `git clean` in the submodule. `apply-patches.sh` mutates gclient-fetched trees in place and `unapply-patches.sh` is how they are reverted.

---

### Task 1: Bootstrap the checkout and prove the baseline

Nothing in this plan can be verified until `gclient sync` has run once. This task also proves the environment builds an EXISTING target correctly, so that a later failure is attributable to a new target rather than to the machine.

**Files:**
- Create: `packages/engine/runtime/runtime-src/../.gclient` (written by `sync-deps.sh`, not by hand)
- Modify: none

**Interfaces:**
- Consumes: nothing
- Produces: a synced checkout where `build/config/BUILDCONFIG.gn`, `build/toolchain/` and `third_party/llvm-build/` all exist, and a stamp at `packages/engine/runtime/runtime-src/.embedpdf-runtime/deps-sync.stamp`

- [ ] **Step 1: Confirm the checkout is currently unsynced**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
ls -d build third_party/llvm-build 2>&1
```

Expected: both reported missing. `testing/` and `tools/` DO exist and are not evidence of a sync.

- [ ] **Step 2: Install depot_tools and put it on PATH**

```bash
git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git "$HOME/depot_tools"
export PATH="$HOME/depot_tools:$PATH"
gclient --version
```

Expected: a version string. Every later step in this plan assumes this `export PATH` is still in effect.

- [ ] **Step 3: Create the fork branch before anything mutates the tree**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
git rev-parse HEAD
git checkout -b embedpdf/feature/mobile-and-wasm-eh
```

Expected: `git rev-parse HEAD` prints `fce2b000cc7a9e2cabc92db41cd57ca03233f07d` before the checkout. If it prints anything else, STOP and report: the plan's Global Constraints pin this sha.

- [ ] **Step 4: Sync dependencies for the host platform**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
PDF_RUNTIME_TARGET_OS_LIST=mac bash scripts/embedpdf-runtime/ensure-deps.sh
```

Expected: a long `gclient sync` (gigabytes, tens of minutes). It writes `packages/engine/runtime/.gclient` containing `target_os = [ "mac" ]`.

- [ ] **Step 5: Verify the sync satisfied what ensure-deps.sh requires**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
ls -d build/config/BUILDCONFIG.gn build/toolchain third_party/llvm-build
cat .embedpdf-runtime/deps-sync.stamp
```

Expected: all three paths listed, and the stamp contains `target_os_list=mac`.

- [ ] **Step 6: Build an existing target end to end**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/build-target.sh darwin-arm64
```

Expected: prints the out dir `out/embedpdf-runtime/darwin-arm64` after ninja completes. Budget an hour. If this fails, the problem is the environment, not this plan: report it and stop.

- [ ] **Step 7: Package it and confirm the artifact shape**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/package-target.sh darwin-arm64
tar -tzf out/embedpdf-runtime-artifacts/libembedpdf-runtime-darwin-arm64-*.tar.gz | sort | head -20
```

Expected: the listing contains `./lib/libembedpdf.dylib`, `./args.gn`, `./BUILD-METADATA.json`, `./LICENSES/PDFIUM_LICENSE` and an `./include/` tree.

- [ ] **Step 8: Record the baseline**

Write `packages/engine/runtime/runtime-src/.embedpdf-runtime/BASELINE.md` (this directory is already used for the sync stamp and is not tracked by git; the file is a scratch note for later tasks, not a deliverable):

```markdown
# Baseline
- fork sha: fce2b000cc7a9e2cabc92db41cd57ca03233f07d
- synced with: PDF_RUNTIME_TARGET_OS_LIST=mac
- darwin-arm64 build: OK
- darwin-arm64 artifact: lib/libembedpdf.dylib
```

- [ ] **Step 9: Commit nothing, and say so**

There is no commit in this task. `gclient sync` writes only gitignored paths and `.gclient` lives outside the repo. Confirm the fork tree is clean apart from patch application:

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
git status --short
```

Expected: empty, or only paths under `third_party/skia` (from `apply-patches.sh` applying `geometry-only-stroke.patch`). Report which.

---

### Task 2: Extract the gn-args mapping so targets are testable

`build-target.sh` computes `args.gn` and immediately runs `gn gen` and `ninja`, so the only way to check a target's arguments today is to do a multi-hour build. Every later task in this plan adds a target. Extract the mapping first.

The cost of this task, stated plainly: it restructures a file the fork's maintainers own, and a future upstream merge could conflict on it. It is worth it because the alternative is five targets whose only test is a CI build.

**Files:**
- Create: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh`
- Create: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/build-target.sh:8-112`

**Interfaces:**
- Consumes: nothing
- Produces: `target-args.sh <target>` prints the complete `args.gn` body to stdout and exits 0, or prints `unknown target: <target>` to stderr and exits 1. `build-target.sh` calls it and is otherwise unchanged.

- [ ] **Step 1: Capture today's args.gn for all nine targets as golden files**

Run this BEFORE changing anything. It is the only proof that the extraction preserves behavior.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
mkdir -p /tmp/args-golden
for t in wasm32 darwin-arm64 darwin-x64 linux-x64 linux-arm64 \
         linuxmusl-x64 linuxmusl-arm64 win32-x64 win32-arm64; do
  sed -n '/^case "\$TARGET" in/,/^esac/p' scripts/embedpdf-runtime/build-target.sh > /dev/null
  ( set -euo pipefail
    TARGET="$t"
    PDF_IS_COMPLETE_LIB=true
    EMBEDPDF_TLS_GLOBALS=false
    eval "$(sed -n '/^case "\$TARGET" in/,/^esac/p' scripts/embedpdf-runtime/build-target.sh)"
    if [[ "$TARGET" != "wasm32" ]]; then EMBEDPDF_TLS_GLOBALS=true; fi
    cat <<EOF
is_debug=false
treat_warnings_as_errors=false
pdf_use_skia=false
pdf_enable_xfa=false
pdf_enable_v8=false
is_component_build=false
clang_use_chrome_plugins=false
pdf_is_standalone=true
use_debug_fission=false
pdf_is_complete_lib=$PDF_IS_COMPLETE_LIB
pdf_use_partition_alloc=false
embedpdf_thread_local_globals=$EMBEDPDF_TLS_GLOBALS
symbol_level=0
target_os="$GN_TARGET_OS"
target_cpu="$GN_TARGET_CPU"${EXTRA_ARGS:-}
EOF
  ) > "/tmp/args-golden/$t.gn"
done
wc -l /tmp/args-golden/*.gn
```

Expected: nine files. Lengths vary with how many lines each target's `EXTRA_ARGS` adds to the 15-line base: 15 for the targets that set none, 16 for `linux-arm64`, 17 for `wasm32`, and 21 for the two `linuxmusl` targets, which set six. Do not treat a length outside that set as fine; it means the capture did not pick up a target's `EXTRA_ARGS`.

- [ ] **Step 2: Write the failing test**

Create `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`:

```bash
#!/usr/bin/env bash
# Copyright 2026 CloudPDF LTD
# SPDX-License-Identifier: Apache-2.0

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../target-args.sh"
GOLDEN="${ARGS_GOLDEN_DIR:-/tmp/args-golden}"
FAILURES=0

fail() { echo "FAIL: $*" >&2; FAILURES=$((FAILURES + 1)); }
pass() { echo "ok: $*"; }

# Every pre-existing target must produce byte-identical args to what
# build-target.sh produced before target-args.sh existed.
for t in wasm32 darwin-arm64 darwin-x64 linux-x64 linux-arm64 \
         linuxmusl-x64 linuxmusl-arm64 win32-x64 win32-arm64; do
  if [[ ! -f "$GOLDEN/$t.gn" ]]; then
    fail "$t: no golden file at $GOLDEN/$t.gn; re-run Task 2 Step 1"
    continue
  fi
  if diff -u "$GOLDEN/$t.gn" <(bash "$SCRIPT" "$t") >/dev/null; then
    pass "$t matches golden"
  else
    fail "$t differs from golden"
    diff -u "$GOLDEN/$t.gn" <(bash "$SCRIPT" "$t") >&2
  fi
done

# An unknown target must be rejected, not silently defaulted.
if bash "$SCRIPT" no-such-target >/dev/null 2>&1; then
  fail "unknown target exited 0"
else
  pass "unknown target rejected"
fi

# Thread-local globals: off for every wasm target, on for everything else.
# wasm32-eh is added in Task 4 and is skipped until it exists.
for t in wasm32 wasm32-eh; do
  out="$(bash "$SCRIPT" "$t" 2>/dev/null)" || continue
  if grep -qx 'embedpdf_thread_local_globals=false' <<<"$out"; then
    pass "$t has thread-local globals off"
  else
    fail "$t must set embedpdf_thread_local_globals=false"
  fi
done
for t in darwin-arm64 linux-x64 win32-x64; do
  if bash "$SCRIPT" "$t" | grep -qx 'embedpdf_thread_local_globals=true'; then
    pass "$t has thread-local globals on"
  else
    fail "$t must set embedpdf_thread_local_globals=true"
  fi
done

if (( FAILURES > 0 )); then
  echo "$FAILURES failure(s)" >&2
  exit 1
fi
echo "all target-args assertions passed"
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: FAIL, every target reporting a diff, because `target-args.sh` does not exist yet.

- [ ] **Step 4: Write target-args.sh**

Create `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh`. The `case` body is copied verbatim from `build-target.sh:20-71` so the golden comparison is meaningful:

```bash
#!/usr/bin/env bash
# Copyright 2026 CloudPDF LTD
# SPDX-License-Identifier: Apache-2.0
#
# Prints the args.gn body for one target and exits. No side effects: this is
# the pure part of build-target.sh, split out so a target's gn arguments can
# be asserted without running gn or ninja.

set -euo pipefail

TARGET="${1:-}"
PDF_IS_COMPLETE_LIB=true
# EmbedPDF: thread-confined runtime. Off by default; enabled below for every
# non-wasm target. wasm isolates globals per-instance, so it stays off there.
EMBEDPDF_TLS_GLOBALS=false

if [[ -z "$TARGET" ]]; then
  echo "usage: $0 <target>" >&2
  exit 1
fi

case "$TARGET" in
  wasm32)
    GN_TARGET_OS="emscripten"
    GN_TARGET_CPU="wasm"
    EXTRA_ARGS=$'\nis_clang=false\nuse_custom_libcxx=false'
    ;;
  darwin-arm64)
    GN_TARGET_OS="mac"
    GN_TARGET_CPU="arm64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  darwin-x64)
    GN_TARGET_OS="mac"
    GN_TARGET_CPU="x64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  linux-x64)
    GN_TARGET_OS="linux"
    GN_TARGET_CPU="x64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  linux-arm64)
    GN_TARGET_OS="linux"
    GN_TARGET_CPU="arm64"
    PDF_IS_COMPLETE_LIB=false
    EXTRA_ARGS=$'\narm_control_flow_integrity="none"'
    ;;
  linuxmusl-x64)
    GN_TARGET_OS="linux"
    GN_TARGET_CPU="x64"
    EXTRA_ARGS=$'\nis_musl=true\nis_clang=false\nuse_sysroot=false\nuse_custom_libcxx=false\nuse_custom_libcxx_for_host=false\nuse_glib=false'
    ;;
  linuxmusl-arm64)
    GN_TARGET_OS="linux"
    GN_TARGET_CPU="arm64"
    EXTRA_ARGS=$'\nis_musl=true\nis_clang=false\nuse_sysroot=false\nuse_custom_libcxx=false\nuse_custom_libcxx_for_host=false\nuse_glib=false'
    ;;
  win32-x64)
    GN_TARGET_OS="win"
    GN_TARGET_CPU="x64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  win32-arm64)
    GN_TARGET_OS="win"
    GN_TARGET_CPU="arm64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  *)
    echo "unknown target: $TARGET" >&2
    exit 1
    ;;
esac

# EmbedPDF: enable per-thread PDFium globals everywhere except wasm, where each
# instance already isolates globals via its own linear memory. This is a prefix
# match, not an equality test, so that every wasm variant is covered.
if [[ "$TARGET" != wasm32* ]]; then
  EMBEDPDF_TLS_GLOBALS=true
fi

cat <<EOF
is_debug=false
treat_warnings_as_errors=false
pdf_use_skia=false
pdf_enable_xfa=false
pdf_enable_v8=false
is_component_build=false
clang_use_chrome_plugins=false
pdf_is_standalone=true
use_debug_fission=false
pdf_is_complete_lib=$PDF_IS_COMPLETE_LIB
pdf_use_partition_alloc=false
embedpdf_thread_local_globals=$EMBEDPDF_TLS_GLOBALS
symbol_level=0
target_os="$GN_TARGET_OS"
target_cpu="$GN_TARGET_CPU"${EXTRA_ARGS:-}
EOF
```

Note the one deliberate behavior change: `!= wasm32*` where `build-target.sh:76` had `!= "wasm32"`. With only `wasm32` existing today this is identical, which the golden test proves; it becomes load-bearing in Task 4.

- [ ] **Step 5: Run the test to verify it passes**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
chmod +x scripts/embedpdf-runtime/target-args.sh scripts/embedpdf-runtime/tests/target-args.test.sh
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: `all target-args assertions passed`, with nine `ok: <target> matches golden` lines.

- [ ] **Step 6: Make build-target.sh call it**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/build-target.sh`, delete the `PDF_IS_COMPLETE_LIB`/`EMBEDPDF_TLS_GLOBALS` initialization (lines 9-13), the whole `case "$TARGET" in ... esac` block (lines 20-71), the TLS-globals `if` (lines 76-78), and the `cat > "$OUT/args.gn"` heredoc (lines 96-112). Replace the heredoc with:

```bash
if ! GN_ARGS="$("$SOURCE_DIR/scripts/embedpdf-runtime/target-args.sh" "$TARGET")"; then
  exit 1
fi
```

placed immediately after the `TARGET` emptiness check, and write it out where the heredoc was:

```bash
printf '%s\n' "$GN_ARGS" > "$OUT/args.gn"
```

`GN_TARGET_OS` is still needed at line 80 for `PDF_RUNTIME_TARGET_OS_LIST` and at line 85 for the `linux-*` sysroot branch, so derive it from the emitted args rather than reintroducing the case:

```bash
GN_TARGET_OS="$(sed -n 's/^target_os="\(.*\)"$/\1/p' <<<"$GN_ARGS")"
```

Place that immediately after the `target-args.sh` call, before line 80's use of it.

- [ ] **Step 7: Verify build-target.sh still produces the golden args**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
PDF_RUNTIME_SYNC=skip bash scripts/embedpdf-runtime/build-target.sh darwin-arm64 >/dev/null 2>&1 || true
diff -u /tmp/args-golden/darwin-arm64.gn out/embedpdf-runtime/darwin-arm64/args.gn && echo "args.gn unchanged"
```

Expected: `args.gn unchanged`. The build itself may re-run or fail past that point; only the `args.gn` comparison matters here.

- [ ] **Step 8: Commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
git add scripts/embedpdf-runtime/target-args.sh \
        scripts/embedpdf-runtime/tests/target-args.test.sh \
        scripts/embedpdf-runtime/build-target.sh
git commit -m "build: split target gn args out of build-target.sh

Adding a target previously could only be verified by a multi-hour build.
target-args.sh prints one target's args.gn and does nothing else, so
tests/target-args.test.sh can assert every target's arguments in
milliseconds against goldens captured from the previous implementation.

The thread-local-globals guard becomes a wasm32* prefix match so future
wasm variants stay correctly excluded."
```

---

### Task 3: Settle whether emsdk 3.1.72 emits legacy or final Wasm EH

`wasm32-eh` exists to let Rust panics unwind. That only helps if browsers can load the result. Emscripten has emitted two incompatible encodings of the exception-handling proposal, and which one 3.1.72 produces decides whether this target is viable at all. Nobody has checked. Find out before building PDFium with it.

If the answer is "legacy", Firefox cannot load the module: Firefox shipped the final encoding in 131 and never shipped legacy in release. That outcome does not fail this task. It changes Task 4, and the finding is the deliverable either way.

**Files:**
- Create: `docs/research/wasm-eh-encoding.md` (in `embed-pdf-viewer`, on `research/runtime-fork-build-matrix`)
- Modify: none

**Interfaces:**
- Consumes: nothing
- Produces: a documented verdict, `legacy` or `final`, that Task 4 Step 1 reads

- [ ] **Step 1: Install the pinned emsdk**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
mkdir -p third_party
git clone https://github.com/emscripten-core/emsdk.git third_party/emsdk 2>/dev/null || true
third_party/emsdk/emsdk install 3.1.72
third_party/emsdk/emsdk activate 3.1.72
source third_party/emsdk/emsdk_env.sh
em++ --version
```

Expected: the version line reports 3.1.72. This is the version pinned at `.github/workflows/release-libpdfium.yml:74`.

- [ ] **Step 2: Compile a module that throws and catches**

```bash
mkdir -p /tmp/eh-probe && cd /tmp/eh-probe
cat > probe.cc <<'EOF'
#include <emscripten.h>
extern "C" EMSCRIPTEN_KEEPALIVE int probe() {
  try {
    throw 42;
  } catch (int v) {
    return v;
  }
  return 0;
}
EOF
em++ probe.cc -fwasm-exceptions -O1 \
  -sEXPORTED_FUNCTIONS=_probe -sMODULARIZE=1 -sEXPORT_NAME=Probe \
  -o probe.js
ls -l probe.wasm
```

Expected: `probe.wasm` exists.

- [ ] **Step 3: Read the encoding out of the binary**

```bash
cd /tmp/eh-probe
"$(dirname "$(command -v em++)")/../bin/llvm-objdump" -d --section=CODE probe.wasm 2>/dev/null \
  | grep -oE '\b(try_table|throw_ref|catch_all_ref|try|catch|catch_all|delegate|rethrow)\b' \
  | sort | uniq -c | sort -rn
```

Expected: a histogram of opcodes. `try_table` / `throw_ref` / `catch_all_ref` means the FINAL encoding. `try` / `catch` / `delegate` / `rethrow` means LEGACY.

If `llvm-objdump` is not on that path, use `wasm-objdump -d probe.wasm` from a wabt install, or `npx -y wabt wasm-objdump -d probe.wasm`. The opcode names are the same either way.

- [ ] **Step 4: Confirm the verdict against a real Firefox**

Opcode names are the evidence; a browser is the proof. Serve the probe and load it.

```bash
cd /tmp/eh-probe
cat > index.html <<'EOF'
<script src="probe.js"></script>
<script>
Probe().then(m => {
  document.title = 'probe=' + m._probe();
  document.body.textContent = 'probe=' + m._probe();
}).catch(e => {
  document.body.textContent = 'FAILED: ' + e;
});
</script>
EOF
python3 -m http.server 8931
```

Open `http://localhost:8931/` in Firefox and note the Firefox version from `about:support`. Expected on success: the page shows `probe=42`. Expected on failure: `FAILED: CompileError: ...`, which is the legacy-encoding outcome.

- [ ] **Step 5: Write the finding**

Create `docs/research/wasm-eh-encoding.md` in `embed-pdf-viewer`. Record the actual observed values, not the expected ones:

```markdown
# Which Wasm EH encoding does emsdk 3.1.72 emit?

`em++ --version`: <paste>
Opcode histogram from `llvm-objdump -d --section=CODE probe.wasm`: <paste>

Verdict: <legacy|final>

Firefox <version> loading the probe: <probe=42 | the exact CompileError text>

## What this means for wasm32-eh
<If final: wasm32-eh is viable as specced; the browser floor is Firefox 131,
Chrome 95, Safari 15.2.>
<If legacy: Firefox cannot load a wasm32-eh module. Either the EH target
raises its emsdk above 3.1.72 for that one target, or wasm32-eh is not worth
building. Say which, and why.>
```

- [ ] **Step 6: Commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer
git add docs/research/wasm-eh-encoding.md
git commit -m "research: record which Wasm EH encoding emsdk 3.1.72 emits

wasm32-eh is only worth building if browsers can load the result.
Firefox shipped the final exception-handling encoding in 131 and never
shipped the legacy one, so the encoding decides the target's viability."
```

---

### Task 4: The wasm32-eh target

A second wasm target compiled so that PDFium's setjmp/longjmp uses the native Wasm mechanism, letting `em++` accept `-fwasm-exceptions` on the same link as Rust's unwinding std. `wasm32` is untouched.

**Files:**
- Create: `packages/engine/runtime/runtime-src/patches/embedpdf-runtime/wasm/config-eh.gn`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh` (add a case)
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh:80-83`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh` (add assertions)

**Interfaces:**
- Consumes: `target-args.sh <target>` from Task 2; the verdict in `docs/research/wasm-eh-encoding.md` from Task 3
- Produces: target name `wasm32-eh`, artifact `lib/libembedpdf.a`, staged by `package-target.sh`'s existing `*)` fallback

- [ ] **Step 1: Read Task 3's verdict and pick the emsdk version**

```bash
grep -A1 '^Verdict:' /Users/andrew/repos/embed-pdf-viewer/docs/research/wasm-eh-encoding.md
```

If the verdict is `final`: use emsdk 3.1.72 for `wasm32-eh`, same as `wasm32`. Nothing in this step changes.

If the verdict is `legacy`: `wasm32-eh` needs both a newer emsdk AND a flag. I measured this rather than assuming it, and my original claim here was wrong: 4.0.0 does NOT default to the final encoding either. Its `src/settings.js:785` sets `WASM_LEGACY_EXCEPTIONS = true`, and its ChangeLog says plainly that this is deliberate because major browsers did not support the final encoding by default at that release. What 4.0.0 does have, and 3.1.72 does not have at all, is the setting itself, so the encoding is forceable. Verified on this machine with 4.0.0: `-fwasm-exceptions -sWASM_LEGACY_EXCEPTIONS=0 -sSUPPORT_LONGJMP=wasm` compiles, links, disassembles to 12 `try_table`, 3 `throw_ref` and 2 `catch_all_ref`, and the resulting module runs under Node returning the expected value. Install 4.0.0:

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
third_party/emsdk/emsdk install 4.0.0
third_party/emsdk/emsdk activate 4.0.0
source third_party/emsdk/emsdk_env.sh
```

Then re-run Task 3 Steps 2 through 4 against 4.0.0 and append the result to `docs/research/wasm-eh-encoding.md` under a heading `## emsdk 4.0.0`. If 4.0.0 also reports legacy, STOP and report: the premise of this target is broken and the plan needs a decision it cannot make for itself.

Whichever version you land on, record it. Task 8 needs it for the CI matrix.

- [ ] **Step 2: Write the failing test**

Append to `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`, immediately before the final `if (( FAILURES > 0 ))` block:

```bash
# wasm32-eh is wasm32 plus nothing in gn: the exception flags live in the
# emscripten toolchain config, not in args.gn. The two targets must therefore
# produce identical arguments, which also proves wasm32 was not disturbed.
if diff -u <(bash "$SCRIPT" wasm32) <(bash "$SCRIPT" wasm32-eh) >/dev/null; then
  pass "wasm32-eh args match wasm32"
else
  fail "wasm32-eh args differ from wasm32"
  diff -u <(bash "$SCRIPT" wasm32) <(bash "$SCRIPT" wasm32-eh) >&2
fi
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: FAIL with `wasm32-eh args differ from wasm32`, because `target-args.sh` rejects the unknown target and prints nothing.

- [ ] **Step 4: Add the target-args case**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh`, change the `wasm32)` case label to cover both:

```bash
  wasm32 | wasm32-eh)
    GN_TARGET_OS="emscripten"
    GN_TARGET_CPU="wasm"
    EXTRA_ARGS=$'\nis_clang=false\nuse_custom_libcxx=false'
    ;;
```

The `wasm32*` prefix match added in Task 2 already keeps `embedpdf_thread_local_globals=false` for the new name.

- [ ] **Step 5: Run the test to verify it passes**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: `all target-args assertions passed`, including `ok: wasm32 matches golden`, `ok: wasm32-eh args match wasm32` and `ok: wasm32-eh has thread-local globals off`.

- [ ] **Step 6: Write the EH toolchain config**

Create `packages/engine/runtime/runtime-src/patches/embedpdf-runtime/wasm/config-eh.gn`:

```gn
# Copyright 2026 CloudPDF LTD
# SPDX-License-Identifier: Apache-2.0
#
# The wasm32-eh variant of wasm/config.gn. Identical except that PDFium's
# setjmp/longjmp is lowered through the native WebAssembly exception-handling
# mechanism instead of emscripten's JS emulation.
#
# Why: rustc's precompiled std for wasm32-unknown-emscripten imports
# __cpp_exception as a wasm tag, which needs em++ -fwasm-exceptions. PDFium
# genuinely uses setjmp/longjmp for libpng and libjpeg error handling
# (core/fxcodec/png/pngmodule.cpp, core/fxcodec/jpeg/jpeg_common.c), which
# with the default settings needs -sSUPPORT_LONGJMP=emscripten. em++ refuses
# both on one link: "SUPPORT_LONGJMP=emscripten is not compatible with
# -fwasm-exceptions". Compiling PDFium with SUPPORT_LONGJMP=wasm puts both
# sides on the same mechanism.

config("compiler") {
  defines = [
    # Enable fseeko() and ftello() (required by libopenjpeg20)
    # https://github.com/emscripten-core/emscripten/issues/4932
    "_POSIX_C_SOURCE=200112",
  ]

  cflags = [
    "-fwasm-exceptions",
    "-sSUPPORT_LONGJMP=wasm",

    # emsdk 4.0.0 still defaults -fwasm-exceptions to the LEGACY encoding
    # (settings.js:785, WASM_LEGACY_EXCEPTIONS = true). Firefox never shipped
    # legacy in a release build, so the legacy default is the one thing that
    # makes this target pointless. Forcing 0 emits try_table / throw_ref /
    # catch_all_ref instead, measured on this toolchain.
    "-sWASM_LEGACY_EXCEPTIONS=0",
  ]

  ldflags = [
    "-fwasm-exceptions",
    "-sSUPPORT_LONGJMP=wasm",
    "-sWASM_LEGACY_EXCEPTIONS=0",
  ]
}
```

- [ ] **Step 7: Route the patch**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh`, add a case beside the existing `wasm32)` one (which is at lines 80-83 and must keep pointing at `wasm/config.gn`):

```bash
    wasm32-eh)
      apply_patch_file "$SOURCE_DIR/build" "$PATCH_DIR/wasm/build.patch"
      copy_patch_file "$PATCH_DIR/wasm/config-eh.gn" "$SOURCE_DIR/build/config/wasm/BUILD.gn"
      ;;
```

Both wasm targets share `wasm/build.patch`, which only teaches `build/` that `emscripten` is a real `target_os`. Only the config file differs.

- [ ] **Step 8: Teach unapply-patches.sh about the second config**

Without this the next build after a `wasm32-eh` build fails, and not obviously. `unapply-patches.sh:60` calls `remove_copied_file "$PATCH_DIR/wasm/config.gn" "$SOURCE_DIR/build/config/wasm/BUILD.gn"`, and that function refuses to delete the destination unless it byte-matches the source, printing `Refusing to remove copied patch output with local changes` and exiting 1. After a `wasm32-eh` build the destination is a copy of `config-eh.gn`, so it will not match, and `apply-patches.sh` calls `unapply-patches.sh` first for EVERY target. One `wasm32-eh` build would wedge all nine other targets.

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/unapply-patches.sh`, add this function after the existing `remove_copied_file` definition (which stays as it is, it still has three other call sites):

```bash
# build/config/wasm/BUILD.gn is copied from one of two sources depending on the
# wasm target that was last built. Accept either, and keep refusing to delete a
# destination that matches neither, which is how a local edit is protected.
remove_copied_wasm_config() {
  local dst="$SOURCE_DIR/build/config/wasm/BUILD.gn"
  local src

  if [[ ! -f "$dst" ]]; then
    return
  fi

  for src in "$PATCH_DIR/wasm/config.gn" "$PATCH_DIR/wasm/config-eh.gn"; do
    if [[ -f "$src" ]] && cmp -s "$src" "$dst"; then
      echo "Removing copied patch output $dst"
      rm "$dst"
      rmdir "$(dirname "$dst")" 2>/dev/null || true
      return
    fi
  done

  echo "Refusing to remove copied patch output with local changes: $dst" >&2
  exit 1
}
```

Then replace the call at line 60:

```bash
remove_copied_wasm_config
```

- [ ] **Step 9: Prove switching wasm targets both ways works**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/apply-patches.sh wasm32-eh
cmp patches/embedpdf-runtime/wasm/config-eh.gn build/config/wasm/BUILD.gn && echo "eh config in place"
bash scripts/embedpdf-runtime/apply-patches.sh wasm32
cmp patches/embedpdf-runtime/wasm/config.gn build/config/wasm/BUILD.gn && echo "plain config in place"
bash scripts/embedpdf-runtime/apply-patches.sh darwin-arm64
echo "switched away cleanly, exit $?"
```

Expected: `eh config in place`, then `plain config in place`, then `switched away cleanly, exit 0`. If the second command exits 1 with `Refusing to remove copied patch output`, Step 8 was not applied.

- [ ] **Step 10: Build it**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
source third_party/emsdk/emsdk_env.sh
bash scripts/embedpdf-runtime/build-target.sh wasm32-eh
```

Expected: prints `out/embedpdf-runtime/wasm32-eh`. Budget an hour.

- [ ] **Step 11: Prove the exception model actually changed**

This is the assertion the whole target exists for. The old archive has undefined references to `emscripten_longjmp`; the new one must not.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
source third_party/emsdk/emsdk_env.sh
echo "--- wasm32-eh ---"
llvm-nm out/embedpdf-runtime/wasm32-eh/obj/libembedpdf.a 2>/dev/null \
  | grep -c "U emscripten_longjmp" || echo 0
```

Expected: `0`. If it prints 8, the flags did not reach the compile: check that `build/config/wasm/BUILD.gn` is the `config-eh.gn` copy and not `config.gn`.

- [ ] **Step 12: Package and commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/package-target.sh wasm32-eh
tar -tzf out/embedpdf-runtime-artifacts/libembedpdf-runtime-wasm32-eh-*.tar.gz | grep libembedpdf
git add scripts/embedpdf-runtime/target-args.sh \
        scripts/embedpdf-runtime/tests/target-args.test.sh \
        scripts/embedpdf-runtime/apply-patches.sh \
        scripts/embedpdf-runtime/unapply-patches.sh \
        patches/embedpdf-runtime/wasm/config-eh.gn
git commit -m "build: add wasm32-eh, a wasm target using native Wasm EH

PDFium's setjmp/longjmp moves to SUPPORT_LONGJMP=wasm so that a module
linking both libembedpdf.a and Rust's unwinding std can be built with
-fwasm-exceptions. em++ refuses -fwasm-exceptions alongside
SUPPORT_LONGJMP=emscripten, which is what forced the Rust side to
panic=abort.

wasm32 is unchanged and still ships the JS longjmp emulation."
```

Expected from the `tar -tzf`: `./lib/libembedpdf.a`.

---

### Task 5: The Android targets

Two targets, `android-arm64` and `android-x64`, mapping to the `arm64-v8a` and `x86_64` ABIs that `pdfium-multiplatform-prototype/build/build-kotlin.sh:25-33` expects. Upstream PDFium already supports Android, so this is additive.

**Files:**
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh` (two cases)
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/package-target.sh:22-41`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`

**Interfaces:**
- Consumes: `target-args.sh` from Task 2
- Produces: target names `android-arm64` and `android-x64`, each staged as `lib/libembedpdf.so`

- [ ] **Step 1: Write the failing test**

Append to `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`, before the final `if (( FAILURES > 0 ))` block:

```bash
# Android is a shared library, like linux and darwin, so pdf_is_complete_lib
# must be false. Getting this wrong produces an .a that package-target.sh then
# cannot find, an hour into the build.
for t in android-arm64 android-x64; do
  out="$(bash "$SCRIPT" "$t" 2>/dev/null)" || { fail "$t: target-args rejected it"; continue; }
  grep -qx 'target_os="android"' <<<"$out" \
    && pass "$t targets android" || fail "$t must set target_os=\"android\""
  grep -qx 'pdf_is_complete_lib=false' <<<"$out" \
    && pass "$t is a shared library" || fail "$t must set pdf_is_complete_lib=false"
  grep -qx 'embedpdf_thread_local_globals=true' <<<"$out" \
    && pass "$t has thread-local globals on" || fail "$t must set embedpdf_thread_local_globals=true"
done
bash "$SCRIPT" android-arm64 | grep -qx 'target_cpu="arm64"' \
  && pass "android-arm64 is arm64" || fail "android-arm64 must set target_cpu=\"arm64\""
bash "$SCRIPT" android-x64 | grep -qx 'target_cpu="x64"' \
  && pass "android-x64 is x64" || fail "android-x64 must set target_cpu=\"x64\""
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: FAIL with `android-arm64: target-args rejected it` and `android-x64: target-args rejected it`.

- [ ] **Step 3: Add the target-args cases**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh`, add before the `*)` case:

```bash
  android-arm64)
    GN_TARGET_OS="android"
    GN_TARGET_CPU="arm64"
    PDF_IS_COMPLETE_LIB=false
    ;;
  android-x64)
    GN_TARGET_OS="android"
    GN_TARGET_CPU="x64"
    PDF_IS_COMPLETE_LIB=false
    ;;
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: `all target-args assertions passed`, including all eight new Android lines.

- [ ] **Step 5: Route the shared-library patch**

Android builds a `.so`, which is what `shared-library.patch` sets up, the same patch `linux-*` and `darwin-*` already use. In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh`, add beside the `linux-*)` case:

```bash
    android-*)
      apply_patch_file "$SOURCE_DIR" "$PATCH_DIR/shared-library.patch"
      ;;
```

- [ ] **Step 6: Teach package-target.sh the Android artifact**

`package-target.sh`'s `*)` fallback stages `obj/libembedpdf.a`, which is wrong for a shared build. Add a case before it, beside `linux-*)`:

```bash
  android-*)
    LIB_SOURCE="$OUT/libembedpdf.so"
    LIB_DEST="$STAGING/lib/libembedpdf.so"
    ;;
```

Do not add Android to the existing `linux-*)` case: the glob would not match `android-arm64` anyway, and keeping them separate leaves room for Android-specific staging later.

- [ ] **Step 7: Sync for Android and build**

The sync writes `target_os = [ "android" ]` into `.gclient`, which is what turns on `checkout_android` and fetches the NDK. `build-target.sh:80` passes `$GN_TARGET_OS` through automatically, and the stamp in `ensure-deps.sh` forces a re-sync because `target_os_list` changed.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/build-target.sh android-arm64
```

Expected: a re-sync fetching the Android NDK and SDK, then a ninja build, then `out/embedpdf-runtime/android-arm64`. Budget two hours for the first one.

- [ ] **Step 8: Verify the artifact is a real Android arm64 shared object**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
file out/embedpdf-runtime/android-arm64/libembedpdf.so
```

Expected: `ELF 64-bit LSB shared object, ARM aarch64`. A mention of `GNU/Linux` rather than Android is normal; the architecture and the `shared object` are what matter.

- [ ] **Step 9: Build and verify the second ABI**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/build-target.sh android-x64
file out/embedpdf-runtime/android-x64/libembedpdf.so
```

Expected: `ELF 64-bit LSB shared object, x86-64`.

- [ ] **Step 10: Package both and commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/package-target.sh android-arm64
bash scripts/embedpdf-runtime/package-target.sh android-x64
tar -tzf out/embedpdf-runtime-artifacts/libembedpdf-runtime-android-arm64-*.tar.gz | grep libembedpdf
git add scripts/embedpdf-runtime/target-args.sh \
        scripts/embedpdf-runtime/tests/target-args.test.sh \
        scripts/embedpdf-runtime/apply-patches.sh \
        scripts/embedpdf-runtime/package-target.sh
git commit -m "build: add android-arm64 and android-x64 targets

Upstream PDFium already builds for Android: core/fxge/BUILD.gn compiles a
complete Android font backend and DEPS carries checkout_android with NDK
entries. sync-deps.sh already forwards target_os to .gclient, so the sync
needed no change.

Both are shared libraries, matching the arm64-v8a and x86_64 ABIs the
Kotlin package consumes."
```

Expected from the `tar -tzf`: `./lib/libembedpdf.so`.

---

### Task 6: Make gn accept target_os="ios"

The risk gate for iOS. PDFium's own build files contain no `is_ios` and no `is_apple`, and `DEPS` has no `checkout_ios`. The rendering sources are nevertheless Apple-family already, so the source change is small. What is genuinely unknown is whether Chromium's `build/config/ios` works in a standalone PDFium checkout with `build_with_chromium=False`.

This task's deliverable is `gn gen` succeeding, or a written finding explaining exactly why it cannot. Do not start Task 7 until one of those exists.

**Files:**
- Create: `packages/engine/runtime/runtime-src/patches/embedpdf-runtime/apple/pdfium.patch`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh` (two cases)
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/unapply-patches.sh`
- Modify: `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`

**Interfaces:**
- Consumes: `target-args.sh` from Task 2
- Produces: target names `ios-arm64` and `ios-sim-arm64`, both static (`pdf_is_complete_lib=true`), staged by `package-target.sh`'s `*)` fallback as `lib/libembedpdf.a`

- [ ] **Step 1: Write the failing test**

Append to `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`, before the final `if (( FAILURES > 0 ))` block:

```bash
# iOS is static, unlike darwin. The device and simulator slices share a cpu and
# are told apart only by target_environment, so asserting that pair matters more
# than it looks: swapping them produces two artifacts that link nowhere useful.
for t in ios-arm64 ios-sim-arm64; do
  out="$(bash "$SCRIPT" "$t" 2>/dev/null)" || { fail "$t: target-args rejected it"; continue; }
  grep -qx 'target_os="ios"' <<<"$out" \
    && pass "$t targets ios" || fail "$t must set target_os=\"ios\""
  grep -qx 'target_cpu="arm64"' <<<"$out" \
    && pass "$t is arm64" || fail "$t must set target_cpu=\"arm64\""
  grep -qx 'pdf_is_complete_lib=true' <<<"$out" \
    && pass "$t is a static library" || fail "$t must set pdf_is_complete_lib=true"
  grep -qx 'embedpdf_thread_local_globals=true' <<<"$out" \
    && pass "$t has thread-local globals on" || fail "$t must set embedpdf_thread_local_globals=true"
done
bash "$SCRIPT" ios-arm64 | grep -qx 'target_environment="device"' \
  && pass "ios-arm64 is a device slice" || fail "ios-arm64 must set target_environment=\"device\""
bash "$SCRIPT" ios-sim-arm64 | grep -qx 'target_environment="simulator"' \
  && pass "ios-sim-arm64 is a simulator slice" || fail "ios-sim-arm64 must set target_environment=\"simulator\""
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: FAIL with `ios-arm64: target-args rejected it` and `ios-sim-arm64: target-args rejected it`.

- [ ] **Step 3: Add the target-args cases**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/target-args.sh`, add before the `*)` case. Note `PDF_IS_COMPLETE_LIB` is deliberately left at its `true` default, unlike `darwin-*`:

```bash
  ios-arm64)
    GN_TARGET_OS="ios"
    GN_TARGET_CPU="arm64"
    EXTRA_ARGS=$'\ntarget_environment="device"\nios_enable_code_signing=false'
    ;;
  ios-sim-arm64)
    GN_TARGET_OS="ios"
    GN_TARGET_CPU="arm64"
    EXTRA_ARGS=$'\ntarget_environment="simulator"\nios_enable_code_signing=false'
    ;;
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/tests/target-args.test.sh
```

Expected: `all target-args assertions passed`, including ten new iOS lines.

- [ ] **Step 5: Start from a clean tree, or the patch you generate will be wrong**

`apply-patches.sh` applies `shared-library.patch` for `darwin-*`, `linux-*` and `win32-*` targets, and it edits `BUILD.gn`: it rewrites `component("pdfium")` to `shared_library("pdfium")` at line 247 and adds a `resources.rc` sources block at line 285. If any of those targets was built last, `BUILD.gn` is still modified right now.

That matters because the next two steps edit `BUILD.gn` and then capture the result with `git diff`. On a dirty tree that diff silently swallows the shared-library changes too, and `apple/pdfium.patch` would end up turning iOS into a shared-library build. The plan's Global Constraints require iOS to be static, and nothing later would catch the contradiction: the patch would apply cleanly and produce the wrong artifact.

The line numbers in the next step also assume a clean tree. With `shared-library.patch` applied, everything below line 285 shifts by three.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/unapply-patches.sh
git status --short
```

Expected: `git status --short` shows no modified tracked files. Untracked entries are fine and expected: `third_party/emsdk/` was cloned by another task, and `BUILD.gn.orig` is a GNU patch backup that `unapply-patches.sh` removes on its own. If `BUILD.gn` or `core/fxge/BUILD.gn` still show as modified, STOP: something applied a patch that this script does not know how to reverse, and generating a patch on top of it would bake the mystery in.

- [ ] **Step 6: Make the two gn edits by hand**

Edit `packages/engine/runtime/runtime-src/BUILD.gn`. Replace the block at lines 287-292:

```gn
    if (is_mac) {
      frameworks = [
        "AppKit.framework",
        "CoreFoundation.framework",
      ]
    }
```

with:

```gn
    if (is_apple) {
      frameworks = [ "CoreFoundation.framework" ]
    }

    if (is_mac) {
      frameworks += [ "AppKit.framework" ]
    }
```

AppKit does not exist on iOS and is referenced exactly once in the whole fork. CoreFoundation exists on both.

Then edit `packages/engine/runtime/runtime-src/core/fxge/BUILD.gn`. At line 173, change only the condition:

```gn
    if (is_apple) {
      sources += [
        "apple/fx_apple_impl.cpp",
        "apple/fx_apple_platform.cpp",
        "apple/fx_apple_platform.h",
        "apple/fx_quartz_device.cpp",
        "apple/fx_quartz_device.h",
      ]
      frameworks = [ "CoreGraphics.framework" ]
    }
```

Those five files include only `<CoreGraphics/CoreGraphics.h>` plus standard headers, and CoreGraphics exists on iOS. Do not touch the `is_linux || is_chromeos || is_wasm` branch above it; iOS must not take that path.

- [ ] **Step 7: Turn the edits into a patch file and revert the working tree**

The patch set is applied with `patch -p1` from `$SOURCE_DIR`, so generate it from git and then restore the originals:

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
mkdir -p patches/embedpdf-runtime/apple
git diff -- BUILD.gn core/fxge/BUILD.gn > patches/embedpdf-runtime/apple/pdfium.patch
git checkout -- BUILD.gn core/fxge/BUILD.gn
head -5 patches/embedpdf-runtime/apple/pdfium.patch
git status --short -- BUILD.gn core/fxge/BUILD.gn
```

Expected: the patch begins `diff --git a/BUILD.gn b/BUILD.gn`, and `git status --short` prints nothing for those two paths.

- [ ] **Step 8: Route the patch both ways**

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/apply-patches.sh`, add a case:

```bash
    ios-*)
      apply_patch_file "$SOURCE_DIR" "$PATCH_DIR/apple/pdfium.patch"
      ;;
```

iOS does NOT get `shared-library.patch`: it is a static build.

In `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/unapply-patches.sh`, add to the block of `reverse_patch_file` calls, immediately before the `shared-library.patch` line:

```bash
reverse_patch_file "$SOURCE_DIR" "$PATCH_DIR/apple/pdfium.patch"
```

and add to the backup-removal block at the end:

```bash
remove_patch_backup "$SOURCE_DIR/core/fxge/BUILD.gn.orig"
```

(`$SOURCE_DIR/BUILD.gn.orig` is already listed there.)

- [ ] **Step 9: Verify apply and unapply round-trip**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/apply-patches.sh ios-arm64
grep -n "is_apple" BUILD.gn core/fxge/BUILD.gn
bash scripts/embedpdf-runtime/apply-patches.sh darwin-arm64
git status --short -- BUILD.gn core/fxge/BUILD.gn
```

Expected: after the first `apply-patches.sh`, `is_apple` appears in both files. After switching to `darwin-arm64`, `git status --short` prints nothing for those two paths, proving the reverse worked.

- [ ] **Step 10: Sync for iOS and run gn gen**

This is the unknown. Run it and record exactly what happens.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
PDF_RUNTIME_TARGET_OS_LIST=ios bash scripts/embedpdf-runtime/ensure-deps.sh
bash scripts/embedpdf-runtime/apply-patches.sh ios-sim-arm64
mkdir -p out/embedpdf-runtime/ios-sim-arm64
bash scripts/embedpdf-runtime/target-args.sh ios-sim-arm64 > out/embedpdf-runtime/ios-sim-arm64/args.gn
gn gen out/embedpdf-runtime/ios-sim-arm64
```

Expected on success: `Done. Made N targets from M files in Xms`.

Expected failure modes, and what each means. `Unsupported target_os: ios` from `build/config/BUILDCONFIG.gn` means Chromium's build does not accept iOS in this configuration, which is the outcome that sinks the target. An error naming a missing `//build/config/ios/...` file means the sync did not fetch the iOS toolchain, so try `PDF_RUNTIME_TARGET_OS_LIST="mac ios"` and re-run. An error about an undefined `target_environment` or `ios_enable_code_signing` means the arg names are wrong for this Chromium revision: run `gn args out/embedpdf-runtime/ios-sim-arm64 --list | grep -E "^(target_environment|ios_)"` and correct Step 3 to match.

- [ ] **Step 11: Record the outcome and commit**

Create `docs/research/pdfium-ios-toolchain.md` in `embed-pdf-viewer` on `research/runtime-fork-build-matrix`, recording the actual command output rather than a summary:

```markdown
# Can PDFium's standalone build target iOS?

Fork sha: fce2b000cc7a9e2cabc92db41cd57ca03233f07d
Sync: PDF_RUNTIME_TARGET_OS_LIST=<what you used>

`gn gen out/embedpdf-runtime/ios-sim-arm64` output:

<paste verbatim>

Verdict: <gn gen succeeds | blocked, and why>

## Source changes needed
BUILD.gn:287 split AppKit (is_mac) from CoreFoundation (is_apple).
core/fxge/BUILD.gn:173 widened from is_mac to is_apple; the five apple/*
sources include only <CoreGraphics/CoreGraphics.h> and standard headers.

## What is still unproven
<Anything gn gen does not tell you. gn gen succeeding is not a compile.>
```

Then commit both repos separately:

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
git add scripts/embedpdf-runtime/target-args.sh \
        scripts/embedpdf-runtime/tests/target-args.test.sh \
        scripts/embedpdf-runtime/apply-patches.sh \
        scripts/embedpdf-runtime/unapply-patches.sh \
        patches/embedpdf-runtime/apple/pdfium.patch
git commit -m "build: teach the build about the Apple family, not just macOS

core/fxge/apple/ is already iOS-clean: its five sources include only
CoreGraphics plus standard headers. It was gated on is_mac. AppKit, which
really is macOS-only, is referenced exactly once and stays behind is_mac.

Adds ios-arm64 and ios-sim-arm64 target arguments. Both are static."

cd /Users/andrew/repos/embed-pdf-viewer
git add docs/research/pdfium-ios-toolchain.md
git commit -m "research: record whether PDFium's standalone build accepts iOS"
```

- [ ] **Step 12: Stop here if gn gen failed**

If Step 9 did not succeed after trying the documented failure modes, report BLOCKED with the verbatim error. Task 7 is not attemptable, and Task 8 ships three new targets instead of five. That is an acceptable outcome of this plan and the spec says so.

---

### Task 7: Build and package both iOS slices

**Files:**
- Modify: none (Task 6 made every script change iOS needs)

**Interfaces:**
- Consumes: `gn gen` succeeding from Task 6; `package-target.sh`'s `*)` fallback
- Produces: `libembedpdf-runtime-ios-arm64-<sha>.tar.gz` and `libembedpdf-runtime-ios-sim-arm64-<sha>.tar.gz`, each containing `lib/libembedpdf.a`

- [ ] **Step 1: Build the simulator slice**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/build-target.sh ios-sim-arm64
```

Expected: `out/embedpdf-runtime/ios-sim-arm64`. Budget an hour.

- [ ] **Step 2: Verify the slice's platform, not just its architecture**

This is the exact check that would have caught the bug that started this whole spec: an artifact can be the right architecture and the wrong platform.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
OBJ=$(ar t out/embedpdf-runtime/ios-sim-arm64/obj/libembedpdf.a | head -1)
ar x out/embedpdf-runtime/ios-sim-arm64/obj/libembedpdf.a "$OBJ" --output=/tmp
vtool -show-build "/tmp/$OBJ" | grep -E "platform|minos|sdk"
```

Expected: `platform IOSSIMULATOR`. If it says `platform IOS` the simulator slice was built as a device slice, which means `target_environment` did not take effect: re-check Task 6 Step 3.

- [ ] **Step 3: Build the device slice**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/build-target.sh ios-arm64
```

Expected: `out/embedpdf-runtime/ios-arm64`.

- [ ] **Step 4: Verify the device slice's platform**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
OBJ=$(ar t out/embedpdf-runtime/ios-arm64/obj/libembedpdf.a | head -1)
ar x out/embedpdf-runtime/ios-arm64/obj/libembedpdf.a "$OBJ" --output=/tmp
vtool -show-build "/tmp/$OBJ" | grep -E "platform|minos|sdk"
```

Expected: `platform IOS`, and specifically NOT `IOSSIMULATOR` and NOT `MACOS`.

- [ ] **Step 5: Package both**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
bash scripts/embedpdf-runtime/package-target.sh ios-arm64
bash scripts/embedpdf-runtime/package-target.sh ios-sim-arm64
for t in ios-arm64 ios-sim-arm64; do
  echo "--- $t"
  tar -tzf out/embedpdf-runtime-artifacts/libembedpdf-runtime-$t-*.tar.gz | grep -E "lib/|args.gn"
done
```

Expected: each listing shows `./lib/libembedpdf.a` and `./args.gn`, and neither shows a `.dylib`.

- [ ] **Step 6: Commit**

No files changed in this task; the deliverable is two artifacts and the verification above. Record the `vtool` output in the task report rather than creating a commit. If anything in `scripts/` or `patches/` did need changing to get the build through, commit that here with a message naming what was missing.

---

### Task 8: Add the five targets to CI

**Files:**
- Modify: `packages/engine/runtime/runtime-src/.github/workflows/release-libpdfium.yml:55-167`

**Interfaces:**
- Consumes: every target name from Tasks 4 through 7
- Produces: a `workflow_dispatch` run that uploads fourteen artifacts

- [ ] **Step 1: Add the matrix rows**

In `.github/workflows/release-libpdfium.yml`, extend the `include` list at lines 63-71. Keep the existing nine rows untouched and append:

```yaml
          - { target: wasm32-eh,        runner: ubuntu-24.04 }
          - { target: android-arm64,    runner: ubuntu-24.04 }
          - { target: android-x64,      runner: ubuntu-24.04 }
          - { target: ios-arm64,        runner: macos-15 }
          - { target: ios-sim-arm64,    runner: macos-15 }
```

If Task 6 reported BLOCKED, omit the two `ios-` rows and note it in the commit message.

- [ ] **Step 2: Make the emsdk version per-target**

`wasm32-eh` may need a different emsdk than `wasm32`, decided in Task 4 Step 1. Replace the fixed `EMSDK_VERSION: 3.1.72` at line 74 with a matrix-driven value. Add `emsdk` to the two wasm rows:

```yaml
          - { target: wasm32,           runner: ubuntu-24.04, emsdk: '3.1.72' }
          - { target: wasm32-eh,        runner: ubuntu-24.04, emsdk: '<the version Task 4 Step 1 settled on>' }
```

and change line 74 to:

```yaml
      EMSDK_VERSION: ${{ matrix.emsdk || '3.1.72' }}
```

Every non-wasm row leaves `emsdk` unset and falls back to 3.1.72, which no non-wasm target reads anyway.

One thing the runner setup must handle, found the hard way in Task 4: emsdk does NOT support two mainline releases installed side by side in one checkout. Running `emsdk activate 4.0.0` in a tree that had 3.1.72 silently removes the 3.1.72 install. Each matrix row is a fresh runner so they do not collide with each other, but the install step must not assume a pre-existing SDK survives an activate. If a future local workflow needs both, use two separate emsdk checkouts rather than two versions in one.

- [ ] **Step 3: Widen the Emscripten install condition**

The install step at line 122 is conditioned `if: matrix.target == 'wasm32'`. Change it to cover both:

```yaml
        if: startsWith(matrix.target, 'wasm32')
```

- [ ] **Step 4: Give the Android rows an NDK**

`gclient sync` fetches the NDK when `target_os = [ "android" ]`, which `build-target.sh` arranges automatically, so no `setup-ndk` action is needed. Android does need the Linux base deps that are already installed for `runner.os == 'Linux'` at line 86. Verify by reading the workflow that no Android-specific step is required, and add a comment above the Android matrix rows recording why:

```yaml
          # Android needs no toolchain step: build-target.sh passes target_os
          # to sync-deps.sh, which writes target_os = [ "android" ] into
          # .gclient, and gclient fetches the NDK from DEPS's checkout_android
          # entries.
```

- [ ] **Step 5: Confirm the iOS rows need only the existing Xcode step**

The `Select Xcode` step at line 137 runs for `runner.os == 'macOS'` and already covers the two new `macos-15` rows. No change. Confirm by reading, and do not add a redundant step.

- [ ] **Step 6: Validate the workflow parses**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
python3 -c "
import yaml, sys
d = yaml.safe_load(open('.github/workflows/release-libpdfium.yml'))
rows = d['jobs']['build']['strategy']['matrix']['include']
print('matrix rows:', len(rows))
for r in rows:
    print(' ', r['target'], '->', r['runner'], r.get('emsdk', ''))
"
```

Expected: `matrix rows: 14` (or 12 if iOS was blocked), with each new target listed against its runner.

- [ ] **Step 7: Commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/packages/engine/runtime/runtime-src
git add .github/workflows/release-libpdfium.yml
git commit -m "ci: build the five new targets

wasm32-eh, android-arm64, android-x64, ios-arm64, ios-sim-arm64.

The emsdk version becomes matrix-driven because wasm32-eh may need a
newer one than wasm32's pinned 3.1.72; every other row falls back to it.
Android needs no toolchain step, because sync-deps.sh already writes
target_os into .gclient and gclient fetches the NDK from DEPS."
```

- [ ] **Step 8: Hand off the run**

Pushing this branch and dispatching the workflow needs write access to `embedpdf/runtime`, which this plan does not assume. Report to the human: the branch name, the commits on it, and that `Release CloudPDF Runtime Libraries` must be dispatched with `ref: embedpdf/feature/mobile-and-wasm-eh` and `release: true` before Task 9 can pin real URLs.

---

### Task 9: Consume the new targets from the scaffold

**Files:**
- Modify: `pdfium-multiplatform-prototype/build/runtime-build.json` (all fourteen entries)
- Modify: `pdfium-multiplatform-prototype/build/link-wasm.sh:9,12,100-140,163-164,171-188`
- Modify: `pdfium-multiplatform-prototype/crates/build-support/src/lib.rs:63`
- Modify: `pdfium-multiplatform-prototype/crates/epdf-wasm/src/lib.rs` (add the panic probe)
- Modify: `pdfium-multiplatform-prototype/build/test-all.sh:77-79`
- Modify: `pdfium-multiplatform-prototype/README.md:227-236`

**Interfaces:**
- Consumes: a published release containing fourteen artifacts, from Task 8
- Produces: a scaffold whose iOS, Android and web gates all pass

- [ ] **Step 1: Re-pin all fourteen artifacts from one release**

The Global Constraints require every artifact to come from one sha. The nine existing entries currently point at `d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`; they must move to the new release too, not just the five new ones.

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
SHA=<the sha the release was cut from>
for t in wasm32 wasm32-eh darwin-arm64 darwin-x64 linux-x64 linux-arm64 \
         linuxmusl-x64 linuxmusl-arm64 win32-x64 win32-arm64 \
         ios-arm64 ios-sim-arm64 android-arm64 android-x64; do
  url="https://github.com/embedpdf/runtime/releases/download/runtime-$SHA/libembedpdf-runtime-$t-${SHA:0:9}.tar.gz"
  printf '%s\t%s\n' "$t" "$(curl -sL "$url" | shasum -a 256 | awk '{print $1}')"
done
```

Write the resulting URL and sha256 into each entry of `build/runtime-build.json`, set the top-level `"sha"` to `$SHA`, and remove every `"status": "pending"` field. Add a `wasm32-eh` entry, taking the fourteenth slot.

- [ ] **Step 2: Verify every artifact fetches and verifies**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
for t in wasm32 wasm32-eh ios-arm64 ios-sim-arm64 android-arm64 android-x64; do
  bash build/fetch-libpdfium.sh "$t" >/dev/null && echo "ok: $t" || echo "FAILED: $t"
done
```

Expected: six `ok:` lines. A sha256 mismatch here means Step 1 pasted a hash for the wrong target.

- [ ] **Step 3: Add a panic probe to the wasm crate**

The whole point of `wasm32-eh` is that a Rust panic becomes a recoverable error instead of killing the instance. That needs something that panics on purpose. In `pdfium-multiplatform-prototype/crates/epdf-wasm/src/lib.rs`, add:

```rust
/// Panics on purpose, so a test can prove a panic crossing the wasm boundary
/// surfaces as an error rather than aborting the instance. Behind a feature so
/// it is absent from a normal build.
#[cfg(feature = "panic-probe")]
#[unsafe(no_mangle)]
pub extern "C" fn epdf_test_panic() -> i32 {
    panic!("epdf_test_panic: deliberate panic for the wasm EH test");
}
```

and in `pdfium-multiplatform-prototype/crates/epdf-wasm/Cargo.toml`:

```toml
[features]
panic-probe = []
```

- [ ] **Step 4: Point the web build at wasm32-eh and drop panic=abort**

First, `pdfium-multiplatform-prototype/crates/build-support/src/lib.rs:63`. `fork_target()` returns the directory name under `build/libpdfium/` that a crate's build script reads, and `pdfium-sys` uses it to find the fork's headers for bindgen. It currently hardcodes the old name:

```rust
    if os == "emscripten" {
        return "wasm32".to_string();
    }
```

Change the string to `"wasm32-eh"`. Miss this and `cargo build --target wasm32-unknown-emscripten` looks in `build/libpdfium/wasm32/`, which `link-wasm.sh` no longer fetches, so bindgen fails on missing headers rather than on anything related to exceptions. Do not instead set `EPDF_SCAFFOLD_TARGET` on the cargo command: the override at line 58 exists for cross-compiles, and relying on it would leave a bare `cargo build` for that target silently wrong.

Then four edits in `pdfium-multiplatform-prototype/build/link-wasm.sh`:

Line 9, change `LIB_DIR="$ROOT/build/libpdfium/wasm32"` to:

```bash
LIB_DIR="$ROOT/build/libpdfium/wasm32-eh"
```

Line 12, change `bash "$ROOT/build/fetch-libpdfium.sh" wasm32` to:

```bash
bash "$ROOT/build/fetch-libpdfium.sh" wasm32-eh
```

Lines 163-164, drop `RUSTFLAGS="-C panic=abort"` from the cargo command, keeping `BINDGEN_EXTRA_CLANG_ARGS` exactly as it is and keeping it on the command rather than exporting it:

```bash
( cd "$ROOT" \
  && BINDGEN_EXTRA_CLANG_ARGS="--target=wasm32-unknown-emscripten -isystem${EM_SYSROOT_INCLUDE} -fvisibility=default" \
     cargo build --release -p epdf-wasm --features panic-probe --target wasm32-unknown-emscripten )
```

In the `em++` invocation at lines 171-188, add two flags after `-sWASM=1`:

```bash
  -fwasm-exceptions \
  -sSUPPORT_LONGJMP=wasm \
```

- [ ] **Step 5: Replace the obsolete comment block**

Lines 100-140 of `pdfium-multiplatform-prototype/build/link-wasm.sh` explain at length why `panic=abort` was necessary. That reasoning is now history and leaving it would mislead. Replace the whole block with:

```bash
# rustc's precompiled std for wasm32-unknown-emscripten lowers a Rust panic's
# unwinding through the native WebAssembly exception-handling proposal: the
# compiled object imports `__cpp_exception` as a WASM TAG, which needs em++'s
# -fwasm-exceptions.
#
# This used to be impossible to satisfy. The fork's wasm32 libembedpdf.a is
# compiled with emscripten's JS setjmp/longjmp emulation (eight undefined
# references to `emscripten_longjmp`), needing -sSUPPORT_LONGJMP=emscripten,
# and em++ refuses that alongside -fwasm-exceptions: "SUPPORT_LONGJMP=
# emscripten is not compatible with -fwasm-exceptions". The workaround was to
# build epdf-wasm with `-C panic=abort`, which cost recoverability: a panic
# killed the whole instance.
#
# The fork now also builds wasm32-eh, the same source compiled with
# -sSUPPORT_LONGJMP=wasm, so both sides use the native mechanism and both
# flags agree. That is the artifact fetched above, and it is why there is no
# RUSTFLAGS on the cargo command below. PDFium's setjmp/longjmp use is real,
# not incidental: core/fxcodec/png/pngmodule.cpp and
# core/fxcodec/jpeg/jpeg_common.c use it for libpng and libjpeg error handling.
```

- [ ] **Step 6: Build the web target and prove a panic is recoverable**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
bash build/link-wasm.sh
node -e "
const { default: create } = await import('./build/out/web/epdf.mjs');
const m = await create();
try {
  m._epdf_test_panic();
  console.log('FAILED: the panic did not propagate');
  process.exit(1);
} catch (e) {
  console.log('ok: panic surfaced as', e.constructor.name);
}
try {
  m._epdf_test_panic();
  console.log('ok: the instance survived and threw again');
} catch (e) {
  console.log('ok: the instance survived and threw again');
}
" --input-type=module
```

Expected: `ok: panic surfaced as ...` followed by `ok: the instance survived and threw again`. Under the old `panic=abort` build the first call aborts the instance and the second cannot run at all, so the second line is the one that proves the change.

- [ ] **Step 7: Run the iOS gate that started this**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
bash build/build-swift.sh ios-sim-arm64
cd apps/ios && xcodebuild -project EpdfScaffold.xcodeproj -scheme EpdfScaffold \
  -destination "platform=iOS Simulator,name=iPhone 17" build 2>&1 | tail -5
```

Expected: `build-swift.sh` no longer exits 2, and `xcodebuild` prints `** BUILD SUCCEEDED **` instead of the pre-build gate's `cannot link against ios-sim-arm64` message.

- [ ] **Step 8: Run the Android gate**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
bash build/build-kotlin.sh android-arm64
ls -l packages/kotlin/epdf-ops/src/main/jniLibs/arm64-v8a/libepdf_uniffi.so
cd apps/android && ./gradlew assembleDebug 2>&1 | tail -5
```

Expected: the `.so` exists and `BUILD SUCCESSFUL`. The `requireNativeLibraries` gate at `apps/android/app/build.gradle.kts:80` passes rather than failing.

- [ ] **Step 9: Run the whole suite**

```bash
cd /Users/andrew/repos/embed-pdf-viewer/pdfium-multiplatform-prototype
bash build/test-all.sh
```

Expected: every previously-skipped section now runs. Report the counts verbatim, including anything that still skips and why.

- [ ] **Step 10: Correct the two documents that now lie**

`pdfium-multiplatform-prototype/README.md:227-236` states that `epdf-wasm` builds with `-C panic=abort` and that a panic kills the engine. That is no longer true. Rewrite that entry to say the web build links the fork's `wasm32-eh` artifact, that both sides use native Wasm EH, and that a panic surfaces as `OpsError::Internal`. Also update the passage naming the four unproven platforms: iOS, Android and web are now proven, so say what actually remains.

`pdfium-multiplatform-prototype/build/test-all.sh:77-79` prints skip messages saying `embedpdf/runtime does not build ios-arm64 or ios-sim-arm64` and the same for Android, pointing at `"status": "pending"` in a file that no longer has any. Those branches are now dead. Delete them rather than rewording, and let the sections they guarded run unconditionally.

- [ ] **Step 11: Commit**

```bash
cd /Users/andrew/repos/embed-pdf-viewer
git add pdfium-multiplatform-prototype/build/runtime-build.json \
        pdfium-multiplatform-prototype/build/link-wasm.sh \
        pdfium-multiplatform-prototype/build/test-all.sh \
        pdfium-multiplatform-prototype/crates/build-support/src/lib.rs \
        pdfium-multiplatform-prototype/crates/epdf-wasm/src/lib.rs \
        pdfium-multiplatform-prototype/crates/epdf-wasm/Cargo.toml \
        pdfium-multiplatform-prototype/README.md
git commit -m "feat(scaffold): consume the fork's mobile and wasm32-eh artifacts

All fourteen artifacts are re-pinned to one fork sha. The web build links
wasm32-eh, so epdf-wasm no longer needs -C panic=abort and a Rust panic
surfaces as OpsError::Internal instead of aborting the instance.

The iOS simulator destination and the Android app now build against real
libembedpdf slices rather than hitting their pending-artifact gates."
```
