# Decisions: upstream catch-up 2026-11-12

Every decision that resolved an ambiguity in this catch-up, grouped by topic. Manager decisions are
`D-nn`; a job's decisions are `CU-Cn-Dn`, with the full text (reason, cost, how to reverse, commit)
in [`decisions/`](decisions/). This file states each decision and its cost in a line or two and
links the job's own record for the rest.

Targets: upstream viewer `ed064e0c867e7688234ad36c0350377544ef93f9`, upstream runtime
`aa59ce554aab55879b728dbb86714e13e5cb327f`. The ledger is
[`multiplatform-upstream-catchup-2026-11-12.md`](../../../multiplatform-upstream-catchup-2026-11-12.md).

## NEW DIVERGENCE

None. No job made a Rust-only divergence or changed TypeScript under `packages/**` to support Rust.
CU-C4, CU-C5 and CU-C6 each state this in their decision files, and CU-C7 matched upstream for the
one candidate it owned (CU-C7-D3, the `labelParams` NaN case), so it isn't one either.

The existing accepted divergences stand as before: `docs/research/rust-plugins-branch-review.md`
("Status"), `multiplatform-upstream-catchup-2026-11-impl.md` and
`docs/research/upstream-catchup-2026-11-07/decisions.md`.

## Scope, authority and branches

- D-01. The method is the catch-up guide on `fork/research/catchup-2026-11-06` and the 2026-11-07
  ledger, brief and scripts.
- D-02. Work happens on `research/catchup-2026-11-12-impl` and `research/catchup-2026-11-12-plugins`.
  The canonical branches move only by fast-forward. The plugins checkout at
  `/Users/andrew/repos/embed-pdf-viewer` holds uncommitted work that isn't part of this catch-up, so
  its fast-forward is left to the owner (the command is in the ledger).
- D-03. As in both earlier catch-ups, impl gets the upstream merge, generator fixes and engine
  vectors; the Rust port lands on plugins. Cost: impl's `cargo test` stays red on vector shape.
- D-04. No push, PR or release. The runtime-src pointer names a runtime commit that exists only in
  the local `~/repos/embed-pdf-runtime` until it's pushed.
- D-05. Rust matches the TypeScript. A new divergence is allowed only when the port can't match, and
  is logged under NEW DIVERGENCE.
- D-06. No Mac-only or Android-emulator steps. Swift is checked on Linux, Kotlin on a desktop JVM.
- D-08. The port split into three jobs by crate area (engine; annotation and form family; kernel and
  the rest), integrated by CU-C7.
- D-09. From CU-C4 on, commit messages name neither job nor decision IDs. Earlier commits
  (`eabb14bc8..0879a6d22`, and `063eb2285`) aren't rewritten.
- D-10. CU-C5's four kernel commits were taken by CU-C6 as cherry-picks, so CU-C7 merged one copy.
- D-11. The annotation and form-field duplicate port stayed in CU-C5.
- D-12. Two `epdf-viewer-host` measurement-scale failures were routed to CU-C4 and checked at
  integration (they pass on the integrated branch).
- CU-C2-D1. The CU-C1 record branch is merged into impl with `--no-ff` (`12256e882`).
- CU-C3-D1. Plugins' `AGENT-BRIEF.md` wins the add/add conflict (it is impl's copy plus Phase 2).
- CU-C3-D2. Plugins' `.gitignore` kept; impl's Swift and Xcode block was a duplicate.
- CU-C4-D12. CU-C4 amended and folded only commits that hadn't left its container.

## Environment and tooling

- D-07. One dev container, `embed-pdf-catchup1112`, with volume `embedpdf-catchup1112`. Heavy steps
  go through `heavy-build.sh`; Cargo runs without incremental builds or debug info, and every job
  deletes its `target/`.
- CU-C1-D2. The container's viewer clone comes from a bundle of the host repository; its
  remote-tracking refs are as fresh as the host's last fetch.
- CU-C1-D4. The helper scripts start from impl's newer `prep.sh` and use the brief's Cargo settings;
  `suite.sh` runs `cargo test` with 4 jobs, not 12.
- CU-C1-D5. The `wasm32-eh` PDFium build needed `patch` and a depot_tools bootstrap; both are steps in
  `scripts/build-wasm32-eh.sh`.
- CU-C1-D8. `wasm-bindgen-cli` 0.2.127, the version plugins' `Cargo.lock` pins.
- CU-C2-D2. Impl was prepared without the `@mfb/embedpdf-rust-viewer` filter, since impl has no such
  package.
- CU-C2-D3. A stale local `wasm32` payload was rebuilt at runtime `aa59ce554`; without it 13 engine
  groups failed on `EPDFPage_CountViewportsRaw`.
- CU-C2-D7. The TS viewer and vue browser timeouts at `--concurrency=6` count as load, not
  regressions (each package passes alone).
- CU-C6-D1. CU-C6 edited on the host and mirrored files into its container worktree for builds;
  its container branch holds no commits of its own.

## Runtime and PDFium

- CU-C1-D1. The fork keeps its Android secure-random branch; upstream `aa59ce554` still has none.
- CU-C1-D3. linux-arm64 PDFium comes from upstream's `runtime-aa59ce554` release, not a fork build.
  Only the Android branch and one test differ in C++.
- CU-C1-D6. The runtime merge adds no fork sync note or ledger edit.
- CU-C1-D7. The two untagged Android blocks `check-fork-tags.py` reports stay untagged.
- CU-C2-D4. Upstream's `engine-runtime-build.json` is taken unchanged.

## Engine conformance vectors and the engine port

- CU-C2-D5. `wire:check` and `operations:check` stay failing on impl; plugins carries their fixes.
- CU-C2-D6. Impl got no generator or vector commit: all 30 regenerable engine groups were
  byte-identical. The new upstream engine behaviour got cases later, in CU-C4.
- CU-C4-D1. New engine group `changes`, recorded at the worker boundary, with its own generator and
  Rust runner (`tests/change_ops.rs`).
- CU-C4-D2. New fixture `viewports.pdf` with `/VP` pages.
- CU-C4-D3. UUIDv7 names and post-2025 timestamps are masked as `<uuid>` and `<now>`.
- CU-C4-D4. Two upstream refusals (a reversed rect, a value only a left-out widget has) are cases of
  their own.
- CU-C4-D5. The replay opens with 64 reserved object numbers and drops the `Host`'s `events` and
  `mutationSequence` from `document.apply`.
- CU-C4-D6. `RawViewport` closes on drop; measure reads and writes go through the raw functions.
- CU-C4-D7. `API_VERSION` is a constant (`"3.2"`) checked against the vector.
- CU-C4-D8. The page-box check also applies to annotation imports.
- CU-C4-D9. Design-import widgets carry `dashArray` and `checkStyle` (upstream `a390171cc`).
- CU-C4-D10. Copy limits travel on the change services; `document.applyChanges` reads `limits`.
- CU-C4-D11. An annotation copy counts its resources as `AnnotationCopier` does. Known gap: covered by
  a unit test, not a vector, since no fixture carries stamp drawings or attached files.
- CU-C4-D13. Each apply step records the events its change publishes.
- CU-C4-D14. Copies are checked by counting distinct names, not numbering them.
- CU-C4-D15. Copies onto another page go onto a page the same change inserts.
- CU-C4-D16. `kinds.json` records and checks the order it asks in.

## Plugin conformance vectors and generator

- CU-C3-D3. The measurement suite is rewritten for upstream's layout-borne scales (`c7f8d75ff`,
  `8bc6546e5`); read-failure and read-epoch cases went with the behaviour.
- CU-C3-D4. The generator follows `stageRemeasure`, the stamp tool remover and the commands asking
  order rather than working around them.
- CU-C3-D7. CU-C3 listed coverage gaps and added no cases; the port jobs did.
- CU-C6-D3. The three wheel and coasting cases are in `stage-turns.json`.
- CU-C7-D1, CU-C7-D3, CU-C7-D4. See [CU-C7](#cu-c7-integration).

## Evidence archives

- CU-C3-D5. Archives re-baselined by re-capture and re-pinning only; pins that name older commits or
  removed files stay.
- CU-C3-D6. The measurement B3 evidence was left to CU-C5.
- CU-C5-D6. Measurement cases are re-accounted in a layer of their own,
  `historical/catchup-2026-11-12-measurement/accounting.json`; the B3 archives stay byte for byte.
- CU-C5-D7. Rewritten measurement evidence follows upstream, each change named in the test.
- CU-C5-D12. New cases are listed as catch-up additions in the live evidence; pins moved with
  `chainpins.py`.
- CU-C7-D5. See [CU-C7](#cu-c7-integration).

## Annotation, form and measurement port

- CU-C5-D1. CU-C5 made four kernel changes its crates needed, each in its own commit.
- CU-C5-D2. An engine verb's `send` gets no capability-scoped handle; both plugins pass their own
  `doc`.
- CU-C5-D3. Viewports are told to the annotation plugin when they differ by value, not identity.
- CU-C5-D4. A page's scale is memoized on the whole page list. Cost: a new `PageScale` whenever any
  page's layout changes.
- CU-C5-D5. The measurement crate's Rust-only tests follow one change per scale change.
- CU-C5-D8. Form-field duplicate was ported in CU-C5. Cost: `planFormCopies` has two Rust ports to
  keep in step (`crates/plugins/form/src/engine/copy.rs` and `epdf-engine`).
- CU-C5-D9. Duplicate fixtures answer with the plugin's own copy rule.
- CU-C5-D10. The two viewer-host scale failures belonged to CU-C4 (see D-12).
- CU-C5-D11. Reachable new lines got cases; the unreachable ones are listed with reasons.

## Kernel, stage, commands and the rest

- CU-C6-D2. CU-C5's kernel commits were cherry-picked; CU-C6's own ports of them were dropped.
- CU-C6-D4. The commands context doesn't cache capabilities per token; no vector observes the
  difference.
- CU-C6-D5. `labelParams` are compared as JSON. The NaN rendering this left behind is resolved by
  CU-C7-D3.
- CU-C6-D6. `omittedCommandsOf`'s `menuTarget` isn't ported; the Rust `ui-core` has no port of that
  function.
- CU-C6-D8. Rust-only tests follow the once-per-store-change derivations; `page_cell_value_copies`
  is removed.
- CU-C6-D9. Remaining coverage gaps are listed with reasons; CU-C7-D4 closes the commands sentence
  gap.
- CU-C6-D10. JavaScript number edges (`Math.max` with NaN) are kept.

## Platform bindings

- CU-C6-D7. The stage uniffi surface grows: `scroll_chaining`, `StageSettingKey::ScrollChaining` and
  `wheel_by`. No rename.
- CU-C7-D2, CU-C7-D7 to CU-C7-D10. See [CU-C7](#cu-c7-integration).

## CU-C7 (integration)

- CU-C7-D1. The three port branches merge `--no-ff` in the order c4, c5, c6; `coverage.json`, the
  only conflict, is regenerated (72,838 statements, more than either side).
- CU-C7-D2. The commands FFI test and the Kotlin and Swift command tests follow
  once-per-store-change asking (`7af98f4df`).
- CU-C7-D3. `labelParams` that aren't finite reach a resolved label as `null`, as upstream's JSON
  round trip does; this resolves CU-C6 F1 without a divergence.
- CU-C7-D4. A commands case for the translated refusal sentences (CU-C6 F2); the suite decodes
  `{"$number": ...}` so both sides read the same non-finite input.
- CU-C7-D5. The commands evidence is re-captured and the live evidence counts the five catch-up
  cases (two commands, three stage-turns).
- CU-C7-D6. The payload script uses emsdk 4's own config and installs its sysroot first (CU-C1 R1).
  The payloads built before this used the image's 3.1.72 LLVM and binaryen under emcc 4.0.0.
- CU-C7-D7. Platform baselines run on `7990560d6` with the head's PDFium and runtime manifest.
- CU-C7-D8. A new Swift-on-Linux harness with stub Apple modules; head and base are compared with
  the same harness. Cost: 38 of 504 tests fail on both sides by design of the stubs.
- CU-C7-D9. Kotlin on the JVM is `test-command-context.sh`; the AAR's instrumented smoke test isn't
  run.
- CU-C7-D10. No binding renames to apply; CU-C6's stage additions are additive.
- CU-C7-D11. `core-wasm` and `epdf-web` are compared with the 2026-11-07 final state, not rerun on
  the start tip.
- CU-C7-D12. TS package tests count turbo's cache hits (41 of 98 in the final run); nothing under
  `packages/**` changed.
- CU-C7-D13. The rust-viewer-bundle `#inline-transport` typecheck error stays, as in 2026-11-07.
- CU-C7-D14. CU-C5 R6 (form `predict.ts` 272-273) stays a remaining advisory.
- CU-C7-D15. The platform toolchains are installed on the volume under `/workspaces/tools`.
- CU-C7-D16. The local impl branch is fast-forwarded `e4c17f48d..a7c75f5ad`.
