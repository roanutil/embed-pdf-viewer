# Which Wasm EH encoding does emsdk 3.1.72 emit?

> Status update, 2026-09-08: the measurements below are historical toolchain probes.
> The [platform audit](./platform-gap-audit.md) rebuilt the real module with
> Emscripten 3.1.72 and Binaryen 132 via `EM_BINARYEN_ROOT`, and confirmed legacy
> EH instructions. It opened, searched, and rendered in Chrome 152 and cached
> Playwright Firefox 132 Nightly. Firefox was therefore available for testing
> through the Playwright cache, although no `/Applications/Firefox.app` exists.
> Nightly success does not verify release Firefox; release Firefox and Safari
> remain untested. The final-EH 4.0.0 probe below is not a full Rust/PDFium build.


`em++ --version`:
```
emcc (Emscripten gcc/clang-like replacement + linker emulating GNU ld) 3.1.72 (437140d149d9c977ffc8b09dbaf9b0f5a02db190)
Copyright (C) 2014 the Emscripten authors (see AUTHORS.txt)
This is free and open source software under the MIT license.
There is NO warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
```

Probe: `probe.cc` compiled with `em++ probe.cc -fwasm-exceptions -O1 -sEXPORTED_FUNCTIONS=_probe -sMODULARIZE=1 -sEXPORT_NAME=Probe -o probe.js`.

Disassembler command:
```
"$(dirname "$(command -v em++)")/../bin/llvm-objdump" -d --section=CODE probe.wasm 2>/dev/null \
  | grep -oE '\b(try_table|throw_ref|catch_all_ref|try|catch|catch_all|delegate|rethrow)\b' \
  | sort | uniq -c | sort -rn
```

Opcode histogram from `llvm-objdump -d --section=CODE probe.wasm`:
```
  12 try
   9 catch_all
   2 rethrow
   2 catch
   1 delegate
```

Verdict: **legacy**

No `try_table`, `throw_ref`, or `catch_all_ref` appear anywhere in the histogram. `try` / `catch` / `catch_all` / `delegate` / `rethrow` are the legacy exception-handling opcodes.

Firefox at the time of this probe: not found on this machine (absent from `/Applications`, `mdfind`, and `which firefox`). The browser confirmation step in the task brief could not be run. This is recorded as unverified, not as a passing or failing result, the opcode histogram above is the evidence actually collected.

## What this means for wasm32-eh

emsdk 3.1.72 emits the legacy exception-handling encoding, not the final one. Firefox never shipped legacy Wasm EH in a release build and only gained support for the final encoding in Firefox 131, so a module built this way would fail to load in Firefox with a `CompileError`, consistent with what the task brief predicted for this outcome, though that failure was not observed in the original probes. The later Nightly result is recorded above; release Firefox remains unverified.

3.1.72 is the version pinned at `.github/workflows/release-libpdfium.yml:74` for the rest of the build matrix, and it predates Emscripten's switch to the final encoding. Given that, `wasm32-eh` as planned is not viable at 3.1.72: raising emsdk to a version that emits the final encoding is a toolchain change scoped to one target: none of the other four new targets (ios-arm64, ios-sim-arm64, android-arm64, android-x64) use emscripten at all, and the only other consumer of the 3.1.72 pin is the existing wasm32 target. That raise, and whatever compatibility fallout it brings for a PDFium build that has otherwise standardized on 3.1.72, is Task 4's decision to make now that the encoding question is settled. Building `wasm32-eh` against 3.1.72 as-is would ship a module that browsers supporting only the final encoding cannot load, and Firefox is the browser that matters here, so it is not worth doing at this pinned version.

## emsdk 4.0.0

Installed alongside the pinned 3.1.72 rather than replacing it: `third_party/emsdk/emsdk install 4.0.0 && third_party/emsdk/emsdk activate 4.0.0`.

`em++ --version`:
```
emcc (Emscripten gcc/clang-like replacement + linker emulating GNU ld) 4.0.0 (97c7c2adab1791b9487d1f376934a3bdc28f8a67)
Copyright (C) 2014 the Emscripten authors (see AUTHORS.txt)
This is free and open source software under the MIT license.
There is NO warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
```

Same probe, same disassembler command, reused from the 3.1.72 run (`/tmp/eh-probe/probe.cc`), recompiled with the 4.0.0 `em++`.

Opcode histogram from `llvm-objdump -d --section=CODE probe.wasm`:
```
  12 try
   9 catch_all
   2 rethrow
   2 catch
   1 delegate
```

Verdict: **legacy**

Byte-for-byte the same histogram as 3.1.72: 12 `try`, 9 `catch_all`, 2 `rethrow`, 2 `catch`, 1 `delegate`, zero `try_table` / `throw_ref` / `catch_all_ref`. `-fwasm-exceptions` still lowers to the legacy encoding by default in 4.0.0.

The toolchain confirms this itself. 4.0.0's `src/settings.js:785` reads `var WASM_LEGACY_EXCEPTIONS = true;`, a setting that does not exist in 3.1.72 at all. Its `ChangeLog.md` entry for 4.0.0 introduces the flag and states the default directly: "This option defaults to true, given that major web browsers do not support the new proposal by default yet." Emscripten added the ability to opt into the final encoding in 4.0.0; it did not make it the default.

Firefox at the time of this probe: not found on this machine, same as the 3.1.72 run. Unobserved, not failing; the histogram is the evidence collected here.

## Task 4's premise does not hold at 4.0.0 either

The brief for Task 4 assumed 4.0.0 is "the first release whose `-fwasm-exceptions` defaults to the final encoding." It is not: the flag exists starting in 4.0.0, but its default is still legacy, by Emscripten's own stated choice, because Firefox and other browsers did not yet support the final encoding by default at the time of that release. Getting the final encoding out of 4.0.0 would require passing `-sWASM_LEGACY_EXCEPTIONS=0` explicitly, which is a different, larger decision than "use a newer pinned emsdk": it means diverging `wasm32-eh` from every other emscripten default `wasm32` relies on, on a setting Emscripten itself does not yet trust as a default.

This is the stop condition the brief anticipated for a legacy result, and it applies here too even though the version was raised. Whether to build `wasm32-eh` against 4.0.0 with `-sWASM_LEGACY_EXCEPTIONS=0` forced on, to try a later emsdk release in case that default has since changed, or to drop the target's browser-compatibility goal and accept legacy encoding, is a decision this research cannot make on its own.

## Forcing the final encoding on 4.0.0

4.0.0 defaults to legacy, but unlike 3.1.72 it has the setting to override,
`WASM_LEGACY_EXCEPTIONS` at `src/settings.js:785`. Measured on this machine
with emsdk 4.0.0, compiling the same probe with
`-fwasm-exceptions -sWASM_LEGACY_EXCEPTIONS=0 -sSUPPORT_LONGJMP=wasm`:

```
  12 try_table
   8 catch_all
   3 throw_ref
   2 catch_all_ref
   1 catch
```

That is the final encoding. The link succeeded with `SUPPORT_LONGJMP=wasm`
set at the same time, which is the combination PDFium needs, and the module
ran under Node 24 returning the expected value 42.

So the toolchain side of `wasm32-eh` is settled: emsdk 4.0.0 plus one flag, not a version hunt. Whether the result is viable in a browser is a separate question, and it is still open. See below.

What this does NOT establish. No browser was tested in these original probes; see the dated update above
for the later real-module browser tests. Forcing the final encoding
trades one compatibility risk for another: legacy is what older Chrome and
Safari understand, and 4.0.0's own ChangeLog says it kept the legacy default
because major browsers lacked final support at its release in early 2025.
Final-encoding browser support for the complete Rust/PDFium module remains unmeasured. That risk is
contained for now, because `wasm32-eh` is a second artifact consumed only by
the Rust scaffold's web demo, and the published `wasm32` target is unchanged.
Anyone promoting `wasm32-eh` to a shipping artifact needs to settle the
browser question first.
