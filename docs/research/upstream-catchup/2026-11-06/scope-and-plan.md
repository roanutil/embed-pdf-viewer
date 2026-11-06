# Both Rust branches: frozen upstream catch-up implementation plan

> **For future implementers:** use the repository's implementation workflow only after implementation is authorized. Checkbox steps below are planned work. **This refresh authorizes no upstream merge, Rust port, runtime rebuild, artifact publication, or release/pin update.**

**Goal:** catch both `research/multi-platform-rust-core-impl` and `research/multi-platform-rust-core-impl-plugins` in the public fork up to canonical upstream `8297bad8d388eb9c70324c22cb001c59248f95f4`, preserving their existing work and proving portable/native parity against that fixed source.

**Architecture:** retain both branch histories, integrate canonical TypeScript once where practical, merge the target runtime into the retained runtime fork, port decisions at the existing Rust owners, and generate/validate one coherent source/artifact graph. Branch tips must be checked individually; C7's equal-tip premise no longer holds.

**Tech stack:** TypeScript oracle, Rust/PDFium, C ABI/UniFFI/WASM router, Swift/iOS/macOS, Kotlin/Android and existing .NET consumer contracts.

**Spec:** the user's request to update C7 for current canonical main in a fresh public-fork devcontainer. [Pinned targets](pinned-targets.json), [core/runtime worklist](core-runtime-inventory.md), and [plugin/binding worklist](plugin-binding-inventory.md) travel with this plan. C7 remains available under [historical/c7](historical/c7/scope-and-plan.md).

## Frozen identities and target policy

| Item | Immutable identity |
|---|---|
| Push destination, exclusively | `https://github.com/roanutil/embed-pdf-viewer.git` |
| Canonical viewer main | `https://github.com/embedpdf/embed-pdf-viewer.git` — `8297bad8d388eb9c70324c22cb001c59248f95f4` |
| Target commit time/subject | 2026-11-06T21:18:06+02:00 Merge pull request #1015 from embedpdf/fix/footer-copyright |
| Observation | 2026-11-06T19:40:15.081503+00:00 |
| `research/multi-platform-rust-core-impl` | `5a2f25f063054c344ea514c2f6da788ad45052f3` |
| `research/multi-platform-rust-core-impl-plugins` | `8fa01cc1ec4d0739af535feec6928e6a81163fd9` |
| Viewer-selected runtime, also observed canonical runtime HEAD | `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` in `https://github.com/embedpdf/runtime.git`, `embedpdf/main` |
| Branch runtime baselines | See each branch's `runtimeGitlink` in [pinned-targets.json](pinned-targets.json); do not substitute C7's single baseline. |

The target was main when observed, not a promise to follow main continuously. Use `refs/catchup/2026-11-06/viewer-target` locally, or the full SHA. Any later target change requires a revised inventory and explicit scope change. Fork `main` is never the canonical target. **All pushes go to the user's fork; never to canonical upstream.**

Runtime choice is now straightforward: integrate the viewer's exact runtime `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` into a fork containing the retained runtime histories. This includes `eeac787c02fac7b40c74c91bbff0ffb5f59ff97e` (C7's proposed rich-comment reader-default fix) and `0b3eb09394120f878f327815ff32181cbfee2bf9` (signature-field/orphan-widget box generation). C7's A/B decision is superseded; no supplemental runtime HEAD is needed. The final gitlink must identify the reviewed fork integration, with `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` in its ancestry, rather than replacing the fork with raw upstream.

## What changed since C7

C7 compared actual upstream baseline `a40d63a199f2d483fd27c9ad439f0d5efbc195aa` to `4ad8986099314b9587df423c802db103a21d665d`. The refreshed target adds **92 upstream history commits, 50 nonmerges and 2,664 changed paths** beyond C7. Both branch merge bases with the frozen target remain `a40d63a…`; the full upstream interval has **174 history commits and 3,062 changed paths**. These counts include generated APIs/docs and are inventory dimensions, not port estimates.

1. **C7 behavior is still required:** sibling annotation kinds, no stroke/dash distinctions, ink curves, rich comments/fonts, final XYZ semantics, caret styles, spread/turn navigation and bounded snapshots.
2. **Public contracts changed broadly:** units/hashes/flags, names, union tags, nullability, ISO timestamps, return values, strict requests and tolerant responses. The worker protocol file now changes too; an unchanged operation name is not unchanged payload compatibility.
3. **Engine behavior advanced:** normalized line breaks and cleaned page text with character-map preservation, signature widgets that retain an installed mark, stored `contentsVersion` plus layout pins, engine/shape resource tokens and version headers in the cloud boundary.
4. **More plugins need migration:** 155 source paths in 21 plugin packages changed after C7. Search copy permissions, i18n fallback/lazy loading, live command label parameters/plurals, and router/native facade vocabulary need explicit coverage.
5. **Our starting work advanced:** plugins retain Apple/macOS packaging and visibility-aware tiled rendering beyond `f28ed4ea4`; impl has the devcontainer work. [Branch divergence](branch-divergence.txt) and the branch-specific path inventories replace the old identical-tip claim.

Detailed source deltas: [new upstream history](viewer-since-c7-commits.tsv), [full upstream history](viewer-since-baseline-commits.tsv), [new upstream paths](viewer-since-c7-paths.tsv), [functional diff](viewer-functional.diff), [contract migration excerpts](contract-migration-map.txt), and [runtime delta](runtime-since-c7.diff).

## Global constraints

- Preserve both histories, existing Rust-only accepted behavior, private regression cases, Apple/macOS/tiled UI changes, and immutable evidence/artifacts. No force push, history rewrite or deletion.
- Operate in the fresh `embed-pdf` development container, image `sha256:30b97dec3bd8d007b9197ab8fc0027f3b62ea3bd25390a5f516b4cbab085df84`, with checkout in persistent volume `embedpdf-workspace`. Linux ARM64 has no Android NDK; Apple/iOS/macOS requires macOS/Xcode. Missing native hosts remain unvalidated gates.
- Keep one owner for mutation, prediction, geometry, permissions and lifecycle. Read the owning repository conventions; include target `docs/conventions/api-evolution.md` for response/request compatibility.
- One changeset file per directly affected publishable package; inherit upstream notes, then audit coverage for the actual Rust/native integration. No implied NuGet release, new RID, CloudPDF Rust server, external notification or workflow execution.
- Planning/source reads and separately authorized devcontainer commits are distinct from catch-up implementation. Historical tests and old library metadata cannot establish a new pass. Two build jobs per heavy build, isolated outputs, and exact source/artifact provenance remain proposed execution constraints.

## Branch integration approach

**Recommended, still a proposal:** create an owned integration branch from the frozen plugins baseline, incorporate the frozen impl baseline so the devcontainer changes survive, then merge the fixed upstream and port remaining work once. Close both public research refs at one reviewed result only after implementation authorization. The user explicitly asked to catch up both named branches; neither can be omitted from the final ancestry and behavior checks.

The authorized devcontainer merge already makes impl `5a2f25f063054c344ea514c2f6da788ad45052f3` an ancestor of plugins `8fa01cc1ec4d0739af535feec6928e6a81163fd9` (divergence 0 / 1,563). Skip a repeat branch merge; start the proposed integration from the frozen plugins tip. If keeping an engine-only impl branch is desired, define the layering and a separate acceptance graph before implementation; do not infer it from branch names. The shared-closure recommendation was not approval to implement it.

Current overlap inventories identify possible conflict locations, not actual merge conflicts. Both branches overlap upstream in convention routing, release workflow, engine entry point, worker host, runtime gitlink and lockfile; plugins additionally overlap actions lifecycle and search identity tests. No trial upstream merge was run. Read overlapping hunks and preserve script-print policy, search-hit identity, C5 command/app-state locks, runtime layer ownership, macOS static targets and tiled-image lifetime.

## Review focus

1. **Shape drift:** omitted/null/empty/value, `kind` versus `status`, renamed units/hashes and ISO strings survive every DTO/router/FFI/native crossing. An unknown response kind falls back; a malformed known kind still fails; writes remain strict.
2. **Text/search correctness:** CRLF and noncharacters change text offsets without moving character geometry; cursor/cache validity follows stored contents and layout versions; copy-denied sessions never leak snippets.
3. **Annotation/signature integrity:** subtype conversion and mark-preserving moves retain identity; style changes to marked widgets refuse before mutation; rollback/cancel does not leave partial state.
4. **Resource/render ownership:** ink/font/snapshot output matches the fixed oracle; stale loaders/font answers and destroyed owners cannot publish; snapshot cap is 16,000,000 pixels and peak memory includes copies/encoding.
5. **Actual contract and artifact association:** string-dispatched methods are exercised, not just compiled; headers/symbols/runtime/UniFFI/WASM/fonts match; existing Apple/macOS package consumers and unsupported platforms are reported truthfully.

## Dependency jobs

### Job 1 — Preserve branch inputs and integrate the fixed runtime

**Depends:** future implementation authorization. **Consumes:** both recorded runtime baselines. **Owners:** runtime `public/epdf_annot.h`, `public/epdf_form.h`, `fpdfsdk/epdf_annot.cpp`, `fpdfsdk/epdf_form.cpp`, native appearance/ink/font internals, `scripts/embedpdf-runtime`, DEPS/.gn. **Produces:** reviewed fork commit containing the required fork histories and exact runtime target, initial matching Linux ARM64/WASM libraries and export/build provenance.

- [ ] Verify branch/source/runtime pins against the manifest; preserve all fork-only work, including plugin baseline `82f2dfabe…` macOS static targets and `3c55b7a1c…` layer ownership, plus impl `8b1e69abd…` toolchain/target-args history.
- [ ] Add failing regression cases for kind/no-color, reader-default rich comments, signature/orphan-widget box generation, and retained layer/target-args/macOS behavior; merge exact `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` in an owned runtime checkout.
- [ ] Run target-args and relevant native embedder/held-layer tests against the new library. Recheck public declarations/exports: C7 added `EPDFAnnot_SetNoColor` and `EPDFAnnot_SetSubtype`; the new signature-box change updates behavior of existing calls. Never infer symbol availability from a header.
- [ ] Build initial Linux ARM64/WASM inputs, record DEPS/toolchain/symbol provenance, review and commit the runtime result before dependent acceptance. Preserve old release bytes.

Estimate: **4–8 engineering hours**, **2–6 machine hours** for initial libraries; final additional platform builds are counted in Job 5.

### Job 2 — One pinned TypeScript integration with both branch histories

**Depends:** Job 1 runtime identity; branch integration strategy settled for execution. **Owners:** owned viewer integration checkout, `packages/engine/main/src/index.ts`, `packages/engine/services/src/worker-host/WorkerHost.ts`, `.agents/skills/embedpdf-conventions/SKILL.md`, `.github/workflows/release.yml`, runtime gitlink, `pnpm-lock.yaml`, plus plugin actions/search overlaps. **Produces:** exact merged TS oracle and a preserved baseline for both refs.

- [ ] Create the integration from the recorded branch tips, keeping impl infrastructure and plugins' completed C5/C8/Apple/macOS/tiled work. Check ancestry before merging any already-included branch.
- [ ] Merge exact `8297bad8d388eb9c70324c22cb001c59248f95f4`, resolve actual hunks by ownership and target behavior, and retain private print/gesture/search identity/lock regressions. Import upstream CloudPDF/SDK/docs changes through the TS merge; do not create a separate Rust server project.
- [ ] Build/typecheck affected TS packages and run focused upstream tests for C7 plus the new text, signature, tolerant read, null/date/tag, search permissions, i18n and command-label behavior. Run inherited CloudPDF cache/version/header/remote-font tests at that boundary.
- [ ] Generate engine/plugin oracle outputs into fresh owned paths using the matching runtime. Record every shape/throw/stale mismatch as a migration work item. Capture final target contracts and enumerate check scripts from the merged tree.

Estimate: **3–6 engineering hours**, **30–90 machine minutes**, after runtime inputs. Path overlap is not a compatibility guarantee.

### Job 3 — Engine semantics and shared contract migration

**Depends:** Jobs 1–2. **Owners:** exact Rust/TS families and case matrix in [core-runtime-inventory.md](core-runtime-inventory.md). **Produces:** final engine DTO/operation semantics, text/versions/signature behavior, and shared vocabulary consumed by plugins/bindings.

- [ ] Add failing cases for all retained C7 requirements, then new CRLF/noncharacter/char-map behavior, marked signature-widget refusal/move/reopen, ISO offsets and invalid dates, strict-write/tolerant-read distinctions, required-null outputs, final named units/flags and `kind`/`status` tags.
- [ ] Migrate shared DTOs and host/wire dispatch first: examples include `sizeBytes`, `md5`, `displaySha256`, `sourceSha256`, `mimeType`, `contentsSizeBytes`, `baseSha256`, `baseSizeBytes`, named permission bits and ISO timestamp strings. Use the exact final schemas, not a global text replacement or permissive fallback on malformed known data.
- [ ] Port text normalization once at the text owner with preserved character indexes/maps; normalize annotation/rich-text line breaks on read/write. Implement `hasMark` and box regeneration without replacing a stored signature mark. Reconcile stored `contentsVersion`, search cursors, token pins and relevant manifest changes with TS; leave HTTP cache/version mechanics at their cloud owner while proving shared DTO compatibility.
- [ ] Replay affected vectors and narrow Rust tests, then all merged engine check scripts, workspace checks/clippy and changed formatting with matching libraries. Include mutation/refusal/transaction controls. Publish reviewed shared contract outputs before Job 4 consumes them.

Estimate: **24–44 engineering hours**, **1–3 machine hours**. This includes the old 4–8-hour core task plus the broad new contract, text/search/signature work, once.

### Job 4 — All affected plugin contracts, portable behavior and native binding surfaces

**Depends:** Job 3 shared contracts; geometry before snapshot, contracts before generated bindings. **Owners:** [plugin-binding-inventory.md](plugin-binding-inventory.md), all changed final plugin contracts, Rust router/harness/FFI and existing Swift/Kotlin/WASM/.NET consumers. **Produces:** P1–P5 closure plus cross-plugin naming/null/tag migration, search permissions, i18n loading and live command labels.

- [ ] Complete C7 P1 annotation/ink, P2 caret/font, P3 stage/spreads and P4 snapshot with exact target field names and host lifetime. Retain 16M pixel cap, scale 2, raw image ownership, copy/render authority and clipboard activation gates.
- [ ] Audit every changed final plugin contract against string router dispatch, Rust capability/records, FFI/UniFFI and facade callers. Port `changedKeys`, get/read/find names, nullable returns, unit/hash names, event `type` and operation `status`/field `kind` tags; compilation alone misses method strings.
- [ ] Add behavioral cases for copy-denied/default/explicit snippets; fallback and requested-language loader races/failure/destruction; live `labelParams`, plural counts, thrown parameter callbacks and no-i18n interpolation; marked-widget style refusal and retained marks. Preserve C5 locks/app state and C8 tiled-image lifetime.
- [ ] Regenerate/replay plugin vectors and native records, exercise real router methods, and validate supported facade contracts. Audit package boundaries/API snapshots/changesets, one file per directly changed publishable package. Existing C# region support is not a new full plugin product.

Estimate: **41–74 engineering hours** (C7's 29–52 plus 12–22 for new plugin/binding work), **1–3 machine hours** after shared inputs. Shared engine DTO migration remains Job 3; P1 geometry and P5 packaging are not duplicated.

### Job 5 — Final source/artifact graph and both public refs

**Depends:** accepted Jobs 1–4. **Produces:** matched supported consumer artifacts, fresh verification and final ancestry/behavior closure for both named branches in the user's fork.

- [ ] Build required final artifacts once per source graph, associate runtime/headers/symbols/bindings/fonts/notices and payload hashes, provision generated fixtures, then run stable-source/default-consumer checks with overrides unset.
- [ ] Validate Linux ARM64 Rust/C/.NET local package and WASM/browser clipboard/menu/router behavior; validate existing Swift/iOS/macOS SwiftPM/XCFramework and tiled UI on matching Apple hosts; generate/compile Kotlin and run available Android gates on an NDK-capable host. Explicitly report every unrun host/device/RID.
- [ ] Prepare exact content-addressed runtime/Apple release bytes and pin proposals. Release publication, remote notification and new NuGet/RID claims require their own execution scope; importing workflow text is not execution.
- [ ] Review every inventory row and final source/artifact graph. Under then-current implementation/push authorization, advance the two public research refs without force, verify `8297bad8d388eb9c70324c22cb001c59248f95f4` in both ancestries, verify preservation of both starting commits and runtime fork history, and record both fresh remote SHAs. Never push upstream.

Estimate: **6–12 engineering hours**, including final review/branch preservation and release preparation. Combined machine budget below counts final builds once.

## Estimate and platform limits

The revised component sum is **78–144 engineering hours** (4–8 + 3–6 + 24–44 + 41–74 + 6–12). Use **80–145 engineering hours** as the planning envelope, plus **8–24 machine hours and native/device queues**. C7's 40–75 hours is superseded. These are estimates from scope inspection, not measured runtimes or a promise of a finish date. A contract migration that reveals additional port gaps requires a new estimate at the owning job, not silently reduced acceptance.

| Surface | Required evidence | Environment limit |
|---|---|---|
| Linux ARM64 Rust/C/.NET | Exact native graph, vectors, local package relocation and real invocation | Fresh container tool versions must be checked; project dependencies/artifacts not provisioned by planning |
| WASM/browser | Matching exports/router, bounded snapshot, actual clipboard activation/copy/save/menu | Historical browser failures/skips remain historical |
| Apple Swift/iOS/macOS | New generated records, canonical Swift package/macOS slices, tiled model lifecycle, actual consumer run | Linux Swift does not validate Apple frameworks; macOS/Xcode required |
| Android Kotlin | New generated records, arm64/x86_64 graph/alignment, compile and available device smoke | This ARM64 Linux image has no Android NDK; use amd64 NDK host and report device gaps |
| Other desktop RIDs | Only explicitly selected real loader/package/native invocation legs | No new validated RID or published NuGet status inferred |

## Fixed-target done state

Both public research refs contain exact `8297bad8d388eb9c70324c22cb001c59248f95f4` and the preserved starting histories, with accepted Rust/plugin/native behavior and matching runtime fork graph. Every inventory row is implemented and proved, inherited and proved, or explicitly accepted as a deferral. Default-valued fields, stale vectors, permissive reads of malformed known values, old libraries, source overlays and historical counts do not satisfy a gate. Both branch SHAs and actual test commands/exits/counts/skips/limitations are recorded. Further upstream progress is a later scope.

This document is planning complete only when its pin/path/link/hash checks pass; product completion remains unclaimed. The former independent C7 review of 0/0/0 applies only to historical C7. This refresh has a plugin-scope review and self-review, not a new whole-plan independent 0/0/0 certification.

Next action (under two minutes): review the target and runtime table, then the shared-closure recommendation before authorizing Job 1.

## Execution addendum

Implementation was authorized after the planning checkpoint. [IMPLEMENTATION.md](IMPLEMENTATION.md) is the live versioned ledger. [PARITY-AND-PLATFORMS.md](PARITY-AND-PLATFORMS.md) makes current platform packages and maintained/expanded TS→Rust vectors mandatory acceptance gates across Jobs3–5. Original planning-only status and historical verification describe the earlier planning phase.
