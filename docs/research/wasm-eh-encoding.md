# Which Wasm EH encoding does emsdk 3.1.72 emit?

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

Firefox: not installed on this machine (absent from `/Applications`, `mdfind`, and `which firefox`). The browser confirmation step in the task brief could not be run. This is recorded as unverified, not as a passing or failing result — the opcode histogram above is the evidence actually collected.

## What this means for wasm32-eh

emsdk 3.1.72 emits the legacy exception-handling encoding, not the final one. Firefox never shipped legacy Wasm EH in a release build and only gained support for the final encoding in Firefox 131, so a module built this way would fail to load in Firefox with a `CompileError` — consistent with what the task brief predicted for this outcome, though that specific failure was not directly observed here because Firefox is not installed on this machine.

3.1.72 is the version pinned at `.github/workflows/release-libpdfium.yml:74` for the rest of the build matrix, and it predates Emscripten's switch to the final encoding (which lands with a newer LLVM/Binaryen toolchain). Given that, `wasm32-eh` as planned is not viable at 3.1.72: raising emsdk to a version that emits the final encoding is a toolchain change scoped to one target, separate from the version already pinned and validated for the other four targets. That raise, and whatever compatibility fallout it brings for a PDFium build that has otherwise standardized on 3.1.72, is Task 4's decision to make now that the encoding question is settled. Building `wasm32-eh` against 3.1.72 as-is would ship a module most browsers cannot load, so it is not worth doing at this pinned version.
