# Catch-up execution ledger

Plan: docs/research/upstream-catchup/2026-11-06/scope-and-plan.md

## Authorization and inputs

User authorized implementation after planning commit4289d3f86, allowed subagents, requires version-controlled progress and permits pushes only to their fork. Viewer target8297bad8d388eb9c70324c22cb001c59248f95f4 and runtime4346c5131cd009c4086fdce13bde7fd43fbd4ce0 remain fixed. Public baseline refs remain impl5a2f25f06 and plugins8fa01cc1e until final acceptance.

## Current jobs

| Job | State | Evidence / next operation |
|---|---|---|
| 1 Runtime | Complete for initial Linux ARM64 and WASM inputs | Final fork 1e214deb2 contains fixed upstream 4346c5131 and preserved fork histories. Native 498 passed / 6 skipped; layer gate has zero unexpected failures. Other hosts and TSAN remain platform gates. |
| 2 Viewer integration | Exact upstream source merged | Merge bdebee29c contains fixed viewer 8297bad8 and both starting histories. Matching fork runtime gitlink is f8bd8110a. Full product acceptance remains open. |
| 3 Engine and shared contracts | Initial ports verified; broader migration in progress | Clean text/charmap, XYZ, attachment records, LF/CR, stroke/dash, error vocabulary and ABI fixes committed. See latest checkpoint below. |
| 4 Plugins and bindings | Pending | Remaining contracts, generated bindings and all existing platform package consumers must be updated together. |
| 5 Artifacts and both refs | Pending | Final combined Rust WASM payload, packaged consumers, platform matrix and both research-ref acceptance required. No release publication authorized. |

## Preflight interfaces and rulings

1. Job1→2/3: runtime headers/symbols/build identity must match the fork. Ruling: source integration and dependency provisioning may overlap runtime builds; no dependent behavior acceptance until matching runtime exists. Cost if wrong: duplicated source work, caught by final graph checks.
2. Job2→3/4: merged TS schemas define final keys/tags/nulls and generated vector outputs. Ruling: preserve generated baseline evidence and regenerate with explicit reviewed diffs, never restore stale fixtures to turn Rust green. Cost if wrong: missed feature cases; paired replay/coverage gates catch them.
3. Job3→4/5: engine DTO vocabulary is shared with router/native bindings. One owner changes each vocabulary; platform packages follow the accepted records and artifacts.
4. Ruling: use manual Git worktrees inside Docker because native app worktree tools operate on the host checkout; retain a versioned ledger here per user request instead of relying on disposable skill scratch. The five plan headings remain Jobs rather than rewriting them for skill script parsing.
5. Platform/parity requirements were reaffirmed by the user: see PARITY-AND-PLATFORMS.md. Both are completion gates, not optional follow-up work.

## Stop/resume

Inspect this ledger, git status, git log and agent worktrees before repeating a task. Logs go under .superpowers/sdd/upstream-catchup-2026-11-06; summaries, source identities, tests/results and decisions are committed here. Preserve running builds and all prior artifacts. Do not reset/clean a worktree to resume.

## Source and first parity checkpoint

- Viewer mergebdebee29c contains exact8297bad8 and both starting histories; conventions conflict retained private lifecycle/isolation rules plus upstreamAPI-evolution row. Runtime gitlink196331edea7566e3a77a8f338822b052d339d345 is public in the runtime fork and contains exact4346 plus all preserved fork work. This is source integration, not full acceptance.
- Runtime targetargs/emsdk/CAPI-listing checks pass; native release compile is active with-j2 after successfulDEPS sync. Exact source/args/compiler provenance is in runtime-release-inputs.json under the persistent build logs. Runtime source ledger lives in its dedicated branch.
- Merged pnpm frozen-lockfile install succeeded. Before setup, plugin-boundary check could not load TypeScript. After setup it exposed22factory-type import failures. Ruling: consumer native bridges may import only plugin factory types without loading implementation; regular types still require contract entries. Focused RED→GREEN and full boundary suite8/8; repository boundary check passes. The Rust frame host keeps its own scheduler type, because it is a native host seam rather than an upstream stage option.
- Canonical43 Linux and WASM archives downloaded with checksum checks as provisional oracle inputs. NAPI addon compiled; canonical inputs do not prove the final retained-fork build. New content-addressed fork WASM payload is unpublished, so build locally; no pin downgrade or fake published hash.
- LinuxLP64 typedef resolution misclassified explicituint64_t/int64_t as32-bit numbers. Added executable generator regression, observed number-versus-bigint RED, preserved fixed-width names before resolution, GREEN1/1. Generic unsignedlong/PDF flags remain numbers. This fix is required to regenerate matching platform bindings safely.
- Text commit8f7054241ed6f22521befe75be989604f9a720c3 expands12TS cases and adds normalization/reader wiring. Provisionalcanonical43 charmap30/30passes; native reader hook mutation RED→GREEN. text_corpus4pass/1fail: page_text_agrees_with_the_typescript_engine, embedded-orientations still expects rawCRLF69units versus cleaned65. Existing page-text vectors were not transformed to hide the mismatch; full targetTS regeneration remains pending.
- XYZ has eight deterministic fixtures and semantic RED→GREEN at raw reader; TS32entrypoint vector replay waits for engine worker bootstrap. No full Job3 completion claim.

- Independent review caught that capitalized interface names endingPlugin could bypass the new type-factory guard. Added negative test, observed RED0-versus1violation, narrowed to lowercamel factory export names, final boundary+ABI suites10/10pass. Preserved per-instance Rust scheduler option as well as global host scheduler; the private host interface replaces an absent TS exportedScheduler type without removing existing Rust-only behavior.
- Merged TypeScript engine-core suite1080/1080passes (85files), engine and inlineworkers build successfully. Canonical TS-only WASM linked in /workspaces/catchup-oracle-canonical43;1056/1056boundfunctions matchABI. The provisional package symlink has a canonical-only payload-source marker; it must be replaced by matching retained-fork payload before finaldefault-consumer acceptance.
- XYZ commitcccf2da64be7403d3f7283d0f958ef7561396ce0 has8PDFs/32entrypointcomparisons, identical native+WASM TSvectors and fullRust action_reader15/15pass, all provisionalcanonical43. Native zeronormalization mutation control failed as expected and source restored.

## Checkpoint after first code ports

Commits: viewermergebdebee29c, cleantext8f7054241, XYZcccf2da64, factorybridgedd9fd4a2b and ABI0e3b1e870. None is a fullcatch-up acceptance claim. Runtime source196331ede is available in the user runtimefork; release native compilation is active. Both original publicresearchrefs remain at their recordedbaselines until finalacceptance.

Runtime provisional suite:5passed/1skipped; skipped wasm above2GiB test reports missing fixture/dist prerequisites. TypeScript engine-core1080/1080, boundary+ABI10/10, repository boundarycheckpass; native/WASM XYZ oracle parity and Rustaction15/15pass. Fullpage-text regeneration is underway to resolve named staleCRLF vectors. Exact retained-fork native/WASM reruns remain mandatory.

Committed execution-state.json and execution-evidence/ capture source identities, resource/log paths, tests and remaining gates. Original verification.json describes planning-only proof at its timestamp; it does not certify these implementation tasks. SHA256SUMS is refreshed when this execution checkpoint is finalized.

## Verified retained-fork checkpoint (2026-11-06)

This section supersedes the provisional build/test status above; those entries retain the decision history. Source checkpoint 0a6fa59e23368886d2623cebac10283b0759942e, runtime fork 1e214deb2869d9233e573666d239bc32ce73f186. Machine-readable results: [checkpoint receipt](execution-evidence/checkpoint-results.json), [runtime acceptance](execution-evidence/runtime-final-acceptance-result.json) and [runtime artifacts](execution-evidence/runtime-final-artifacts.json).

1. Runtime: native release and WASM static archives are built, checksummed and repackaged against final source 1e214deb2. Final source differs from prior build only in reviewed layer registry/docs; GN/Ninja no-op and library byte equivalence verified. Native unit/embedder total 498 passed, 6 skipped (five unavailable external encrypted fixtures; one opt-in Acrobat output). Layer gate: 361 passed, one opt-in skip, three unchanged expected failures, zero unexpected failures. Nine checker tests and 49 saved PDFs passed independent validation without recovery.
2. Final matched viewer checks: Rust engine library 509/509 plus 85 selected integrations = 594 passed. Boundary/ABI tests 10/10, attachment coverage guard 3/3, runtime suite 6/6 with zero skips including actual above-2GiB read/render/write. All 13 required checkpoint command groups pass; broader tooling typecheck still reports ten contract migration diagnostics. Earlier TS engine-core suite passed 1080 tests across 85 files.
3. Actual TS oracle vectors were regenerated and checked for cleaned page text/charmaps, attachments (sizeBytes/md5), XYZ, public LF/native PDF CR, and no-stroke/dash scenarios. New PDF/scenario coverage includes joined hyphens, eight XYZ fixtures, 22 stroke/dash cases, and mixed-line-break rich-text agreement. Independent review found contents normalization occurred after agreement checks; commit 8766530d0 fixes shared/create/patch normalization with ten passing regression tests. Three new generator runtime-type inference errors were fixed in 0a6fa59e2; ten broader migration diagnostics remain.
4. ABI generation preserves uint64_t/int64_t and size_t/ptrdiff_t before platform typedef erasure; native Node bindings and matching TS-only WASM oracle were rebuilt, with 1056/1056 exported functions matching. Exact final archives are extracted under /workspaces/catchup-final-runtime-artifacts. Use its runtime-build.local.json via PDF_RUNTIME_BUILD_FILE for local builds. Tracked release/local configuration is unchanged. WASM lib symlink points to /workspaces/catchup-oracle-fork1e214; this is the TS oracle, not the final combined Rust browser payload.
5. All pushes target user forks only. Runtime result is public on catchup/2026-11-06-runtime. Viewer checkpoint is on research/catchup-2026-11-06. User viewer fork Actions are globally disabled and have no repository secrets; no workflow dispatch, release publication or upstream write occurred. Original research refs stay at their recorded baselines until full acceptance.

## Remaining acceptance work

| Owner/job | Required next work | Current evidence/limitation |
|---|---|---|
| Engine/contracts, Job3 | Subtype conversion, markup RC/body defaults, ink geometry, font display and remaining DTO/cache/tolerant-read changes | Broader annotation tests expose old ink bbox, missing richText, absent-C expectations and attachment keys. Do not relax tolerances or restore stale expectations. |
| Oracle tooling, Job3 | Migrate removed mutation changed field, FormField.family, scope PdfBits, security advisory and SignatureDTO APIs; regenerate affected real TS vectors and replay Rust | Ten typecheck diagnostics; focused generators above pass. |
| Plugins/bindings, Job4 | Complete P1–P5; regenerate WASM/UniFFI/C metadata and update all exposed existing Swift/Kotlin/.NET/browser package surfaces | Platform matrix in PARITY-AND-PLATFORMS.md is mandatory. |
| Artifacts/consumers, Job5 | Build final combined Rust WASM, current native packages and supported consumers; record host/device gaps and immutable graph checksums | Initial runtime inputs are ready. Apple hosts, Android NDK and dotnet are unavailable in this Linux image; these checks remain incomplete. |
| Both research refs, Job5 | Final reviewed acceptance, merge correct result into each requested fork branch, push and verify ancestry | Neither research ref has been advanced for catch-up. Preserve distinct branch scope and all private behavior. |

Resume with Job3 contract rows and their TS-oracle generators. Check existing commits and receipts first; do not rebuild completed runtime inputs or repeat the accepted ports. Whole catch-up is incomplete.
