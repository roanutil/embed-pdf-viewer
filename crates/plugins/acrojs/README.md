# epdf-acrojs

`packages/core/acrojs` in Rust: the host side of Acrobat JavaScript. All 75 cases in
`vectors/core/acrojs.json` and all 20 in `vectors/core/acrojs-host.json` pass
(`cargo test -p epdf-acrojs`). One passes through a named exception:
`dispose-pending-factory` releases the second realm at the repeated `dispose()`, where this crate
releases it when the queue drains (M13, `22ca510e6`). The entry is in
`crates/plugin-conformance/outcome-exceptions.json`.

What ports is `host.ts`, `input.ts`, `color.ts`, `actions.ts` and the contract and constants in
`types.ts`. What doesn't is `prelude.ts`, the JavaScript that runs inside the sandbox. We ship its
text as data instead: `tooling/plugin-conformance/src/suites/acrojs.ts` writes `PRELUDE_SOURCE` to
`prelude.js`, `vectors:check` fails when that file is stale, and the crate exposes it as
`PRELUDE_SOURCE` through `include_str!`. The text comes from `prelude.ts` type-stripped by the
TypeScript compiler, not from tsx. tsx's esbuild wraps every arrow in a `__name` helper that exists
only in the loading module, so its `installAcroJs.toString()` is dead in a sandbox realm. The
published build prints a different but behaviourally identical text, which
`packages/core/acrojs/test/built-parity.test.ts` pins.

The sandbox is a capability the host supplies, not a port. `ScriptSandbox` is a trait (JSON in,
JSON out, `Err` where a JavaScript implementation would throw) and `ScriptSandboxFactory` returns a
`Promise` of one. The TypeScript defaults to a lazily imported QuickJS sandbox; Rust has no default.
The vectors drive the host with a scripted sandbox (`harness.rs`, twin of `scriptedSandboxFactory`
in the suite) that logs every factory call, `boot`, `run` and `dispose` with its canonical input and
answers from the vector. That pins what the host asks and what it does with each answer, not what a
VM computes.

Two things differ from the obvious Rust spelling and are worth knowing before you touch them:

- `js.rs` carries the JavaScript semantics the TypeScript leans on: `Number#toString`,
  `Math.round` (half toward positive infinity), `Math.max`/`Math.min` with NaN, `String#trim`
  (which keeps U+0085 and removes U+FEFF, unlike `str::trim`), UTF-16 lengths, `String(x)` and
  `encodeURIComponent`. The actions crate uses it too.
- `ScriptHost::transaction` runs on `epdf_kernel::SerialQueue`, whose adoption of a returned
  promise was fixed to match `then()` for this port (commit `3dbd19149`).

The generators reach every statement of the five ported files. One branch they can't:
`host.ts:184`, the `String(error)` arm for a boot-source rejection that isn't an `Error`. A scripted
callback always rejects with one, and Rust has only `Error`.

`vectors/coverage.json` lists `prelude.ts` at 11/1217 statements because the actions plugin imports
the package index, which loads it. That file doesn't port, so its number is not a gap.

These counts are from the port before Task 13.8, and the line numbers point into it: the field
view in `input.rs` and the identity members in `host.rs` have changed since. The list is stale
and the run has not been repeated; Task 14 did not regenerate it (not re-run since 172f5369d).

`cargo mutants -p epdf-acrojs --test-package epdf-acrojs` catches 177 of 185 viable mutants (33
unviable). Each survivor builds the same observable result:

- `host.rs:110`, `resolve_script_identity` returning `Default::default()`. This wrapper over a
  `DocumentHandle` is only called by the actions crate, whose vectors pin it (the
  `environment-defaults` case logs the engine identity); the acrojs vectors drive
  `resolve_script_identity_from`.
- `js.rs:22` (`>` to `>=`) and `js.rs:24` (`<` to `<=`) in `number_to_string`: zero has already
  returned, so the boundary is never reached.
- `js.rs:40`, three mutants of the exponent sign `n - 1 < 0`: that branch only sees `n > 21` or
  `n <= -6`, where `n - 1`, `n / 1` and `n + 1` share a sign and `< 0` and `<= 0` agree.
- `types.rs:183`, the capacity hint passed to `serialize_seq`.
- `types.rs:198`, the `expecting` text of the color deserializer, which only a malformed color reads.
