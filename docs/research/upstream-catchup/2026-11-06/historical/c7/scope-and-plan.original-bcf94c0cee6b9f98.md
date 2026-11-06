# C7 pinned latest-main catch-up implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking. **This is scope/design/plan only; no implementation is authorized.**

**Goal:** catch BOTH private research/multi-platform-rust-core-impl and research/multi-platform-rust-core-impl-plugins up to the same frozen canonical main, with portable behavior and existing supported consumer contracts.

**Architecture:** merge canonical TypeScript while preserving private additions, merge upstream runtime into the retained fork first, port changed decisions at their existing owners, then regenerate bindings and validate one final source/artifact graph. Both branches now point to f28ed; recommend one integration sequence and one shared final closure SHA.

**Tech stack:** TypeScript oracle, Rust/PDFium, existing C ABI/UniFFI/WASM adapters, .NET10 local package, Apple/Android consumers.

**Spec:** coordinator916's new latest-main request and pinned-targets.json. Detailed owners/feature commits/tests: core-runtime-inventory.md/json and FINAL plugin-binding-inventory.md/json. Those linked inventories are part of this plan; their work is not added twice. Original-target C5h remains complete.

## Global constraints

Repository Git/API/source operations use approved Docker image sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703; native platform builds need separately authorized matching hosts. Preserve user checkout, private history/tests, released bytes and immutable evidence. Future implementation uses owned stable worktrees/targets and existing FIFO/heavy-build rules. No force/rewrite/delete, unrelated workflow policy changes, implied C# plugin product, public NuGet or new RID claim. One changeset per directly changed publishable package. Preserve accepted Rust-only divergences unless explicitly reconsidered. Planning performed no source edit, merge/probe, build, test, release, pin update or push.

## Frozen targets and proposed choices

| Item | Fixed identity |
|---|---|
| Both private branch tips | f28ed4ea4aa46c8e69676558993b103d68b19e08, freshly verified2026-11-04T13:26:32Z |
| Embedded runtime fork | 3c55b7a1c0bf956231392a573e87f28dd6d6667d |
| Canonical viewer main | https://github.com/embedpdf/embed-pdf-viewer.git — 4ad8986099314b9587df423c802db103a21d665d |
| Viewer time / observed | 2026-11-04T15:23:43+02:00 / 2026-11-04T13:25:23Z |
| Viewer-selected runtime A | f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185 |
| Canonical runtime default B | https://github.com/embedpdf/runtime.git, embedpdf/main — eeac787c02fac7b40c74c91bbff0ffb5f59ff97e |

Canonical repos/default branches were verified from existing origin remotes, repository API metadata and viewer .gitmodules. Fork main is not canonical. No pins silently move during execution.

**Runtime recommendation, not user-approved:** choose fixed B. It is A plus two history commits/one functional fix, 8bdd5c097be2c29f72a70dd13e0a1e98ce28f65c: markup comments read rich text over reader defaults rather than drawing /DA. It changes five files and no exported signatures, directly supporting the new rich-comment feature. Option A exactly follows the viewer gitlink but omits that identified correction. Record which fixed option is selected before implementation; a later moving runtime HEAD is a new scope.

**Branch recommendation, not user-approved:** one integration from now-identical f28ed, then both private refs advance to the same reviewed closure. Alternative: intentionally recreate engine-only impl plus plugin child with a new written layering rule and separate acceptance. Avoid duplicate cherry-picks/endpoint replacement. Public plugin mirroring needs an explicit destination in eventual execution scope; no public impl branch creation is proposed.

There are82upstream-only viewer history commits,51nonmerges,794changed paths,151core/engine paths and36functional plugin source paths in6packages. Runtime has38upstream-only and11fork-only history commits relative to A. These counts are inventory dimensions, not feature estimates. CloudPDF tenant/module/usage/licensing/Postgres/migrations, docs/examples/version history arrive as TypeScript through merge and receive appropriate upstream checks; they are not a new Rust service project.

## Functional scope and review focus

| Work | Concrete target requirement |
|---|---|
| Engine kind/null stroke/dash | Identity-preserving sibling changes; explicit null stroke writes /C []; omitted retains defaults; [] dash refused, null and nonnegative nonempty [0] accepted, omitted dashed pattern [3,3]. Current Rust refuses subtype/null-color and accepts empty dash. |
| Rich comments/ink/font fallback | Reuse FreeText/raw-fact foundations; add markup RC/source/plain-projection/null/default semantics, Acrobat-fit ink geometry, native standard fallback and final display-chain encoding. Trace shared DTO adapter before adding duplicate Rust mapper. |
| Final XYZ | Upstream01f97 reads raw short-array parameters and normalizes literal zoom0 to null. Current raw.location helper differs; intermediate3fd is superseded, not a second port or already-completed final behavior. |
| Plugin P1–P3 | Annotation payload/geometry; caret/range/whole-box text style and stale font answers; page/spread stepping/turn gestures. Preserve C5 locks/appState. |
| Plugin P4–P5 | Snapshot/crop/rotation, scale2/cap16,000,000pixels and host copy/save lifetime; generated native/WASM records and current C#/package compatibility. Existing C# Region/Rotation is low-level support, not a snapshot plugin. |

Public C exports1066→1068: exactly EPDFAnnot_SetNoColor(FPDF_ANNOTATION,FPDFANNOT_COLORTYPE) and EPDFAnnot_SetSubtype(FPDF_ANNOTATION,FPDF_ANNOTATION_SUBTYPE), both FPDF_BOOL. Zero removals/signature changes. Worker request-union file is unchanged; existing operation payloads and semantics still evolve.

1. Preserve omitted/null/value/empty distinctions across schemas, prediction, dictionaries and readback.
2. Kind conversion keeps identity/authority; refusal and transaction/cancellation paths do not partially mutate state.
3. Preserve fork layer ownership, target-args/emsdk/confinement across PDFium/DEPS changes; new headers must never link old libraries.
4. Ink/fallback/snapshot must match the frozen oracle without stale answers, duplicate policy owners or hidden memory inflation.
5. Stable sources, generated fixtures, artifact association and override-free default checks prevent contaminated-checkout/setup verdicts.

## Dependency jobs

### Job 1 — Runtime fork and matching oracle inputs

**Depends:** reviewed A/B choice. **Files:** public/epdf_annot.h; annotation/ink/font/image internals; DEPS/.gn; existing scripts/embedpdf-runtime target args/tests. **Produces:** reviewed fork commit containing3c55 and selected target, both new callable symbols, matching initial LinuxARM64/WASM libraries and export/build provenance.

- [ ] Prepare an owned runtime worktree and regression cases for chosen comment-default behavior, layer ownership and target-args; retain all11fork-history commits, then merge the fixed runtime target.
- [ ] Run target-args, relevant annotation/ink/rich-text/font/linearized-layer embedder tests and held-layer regression against the new library; require +2/0removed/0changed-signature symbol association.
- [ ] Build initial LinuxARM64/WASM with exact DEPS/toolchain provenance and independently review the result before dependent acceptance; other current consumers use the same source graph later.

Estimate3–6engineering hours,2–6machine hours initially; cold dependencies/native target queues explicit.

### Job 2 — One pinned viewer merge preserving private work

**Depends:** Job1 identity. **Files:** integration tree fromf28ed; runtime gitlink/lockfile/release-workflow conflicts; upstream TS contracts/tests. **Produces:** pinned merged TS oracle and retained private policy/identity regressions.

- [ ] Merge exact4ad into an owned integration worktree. Preserve script-print gate/tests5462de7/253bca6, gesture-removal7a7c94f, active-search-hit identityca6d809 and C5 locks/appState; never replace whole endpoint files.
- [ ] Inspect actual conflict hunks, build/typecheck affected TS packages and run target sibling/comment/dash/no-stroke/fallback/XYZ plus inherited CloudPDF boundary checks. Path overlap is release.yml/gitlink/pnpm-lock only; no trial merge was run and this is not a compatibility guarantee.
- [ ] Generate engine/plugin oracle outputs into owned paths using matching runtime; stale/throw/shape drift becomes the port worklist, not grounds to restore old fixtures.

Estimate1–3engineering hours,20–60machine minutes after inputs.

### Job 3 — Core engine behavior and invariant proof

**Depends:** Jobs1–2. **Owners:** crates/epdf-engine/src/features/annotation/write/{prepare,patch,create,update}.rs, annotation readers/rich_text, features/actions/destination.rs, existing raw font facts; crates/pdfium/src/{annot_write,action,document_fonts}.rs. Exact TS files/commits are in core inventory. **Produces:** target semantics through existing operations, one mutation-policy owner.

- [ ] First add failing oracle cases for valid/refused sibling conversion and identity, null/omitted stroke, []refusal/[0]acceptance/defaultdash, rich-comment mismatch/newline/null/source, XYZzero/null/shortarray and fallback ordering. Pure ink geometry is pluginP1's single owner.
- [ ] Implement at those owners with actual new C calls; keep lifecycle/authorization/transactions/error semantics. Reuse shared DTO/raw facts and trace final DTO consumer before introducing a new mapper.
- [ ] Replay narrow engine vectors and Rust annotation/write-transaction/action/font suites, then enumerate all merged :check scripts/workspace tests/clippy/changed formatting with new native library. Positive controls show new tests fail if their guard is removed.

Proposed, NOT executed: pnpm --filter @embedpdf/tooling-engine-conformance annotation-update:check (plus create/write/read/appearance/actions/fonts/schema/wire/operations); CARGO_BUILD_JOBS=2 cargo test -p epdf-engine --test annotation_update --test write_transactions --test document_fonts_wire; then cargo test --workspace --no-fail-fast with matching library/resource wrapper. Do not freeze an old script count.

Estimate4–8engineering hours,30–90machine minutes after matching inputs.

### Job 4 — Final plugin/native P1–P5 worklist

**Depends:** Job3 payloads; P2afterP1vocabulary; P3can overlap after kernel/oracle compatibility; P4afterP3 geometry and engine region rendering; P5afterP1–P4. **Files/interfaces/acceptance cases:** FINAL plugin-binding-inventory.md/json is authoritative. **Produces:** portable decisions and existing adapter contracts matching pinned TS, without a new C# plugin product.

- [ ] Execute P1annotation/geometry6–10h, P2caret/font5–9h, P3stage4–7h, then P4snapshot/hostports8–14h with linked exact owners/cases. Snapshot's16Mpixel cap is about64MB raw RGBA before copies; measure peak memory and preserve native raw-image ownership.
- [ ] Regenerate/replay plugin vectors and Swift/Kotlin/WASM records; prove refusal/disposal/font-staleness/user-activation behavior. Audit dependency/exports/changesets with one file per directly affected package.
- [ ] P5binding/package graph6–12h consumes Jobs1–3 artifacts; revalidate existing LinuxARM64 C# local package and actual native facades. No extra runtime/core port, publicNuGet or new RID acceptance is implied.

Proposed, NOT executed: pnpm check:plugin-boundaries; plugin vectors:check/coverage:check; selected Rust plugins with/without uniffi; pnpm api:check; node tooling/dotnet-parity/run.mjs --manifest <new-owned-matching-manifest>. Existing historical graph/source pins cannot claim new acceptance. Exact commands/resources remain in linked plugin inventory.

Estimate29–52engineering hours for P1–P5 total, excluding Jobs1–3. Do NOT add P1geometry or P5consumer/package acceptance again elsewhere.

### Job 5 — Exact-source artifacts, release/pin and both-ref closure

**Depends:** accepted Jobs1–4. **Produces:** accepted source/artifact graph, truthful platform matrix and fresh remote refs.

- [ ] Build final artifacts once per required graph; P5checks symbols/manifests/fonts/notices/relocation and consumers. Provision canonical generated fixtures before stable-source/override-free guards; retain actual pass/skip/fail counts and commands/exits/hashes.
- [ ] Prepare content-addressed WASM/Apple saved bytes/pins from the finished link graph. Publication/pin execution is a future separately reviewed operational action under then-current authorization. Importing release/notify-platform workflow text does not publish, contact another repository or grant credentials. NewNuGet publication stays separately named.
- [ ] Independently review the whole source/artifact result, then under later execution authorization ordinarily advance both private refs to one reviewed closure and verify fresh SHAs/ancestry (or execute the deliberately chosen layered option). Retain old evidence/releases; cleanup remains separate.

Distinct release/remote-close handling1–3engineering hours. Consumer/package acceptance is already P5, not duplicate effort.

## Platform/resource matrix and union estimate

| Surface | Proposed acceptance/resource | Current limitation |
|---|---|---|
| LinuxARM64 Rust/C/.NET10 | ApprovedDocker,2build jobs/FIFO; new library, localnupkg relocation, matching packed-viewer/native raster graph | J8BASE6a740localpackage is historical/net10/glibc2.41; noNuGet |
| WASM/browser | ABI/loader/router and crop/snapshot clipboard/menu/activation gates;45–90minutes after artifacts | Previous22browser fixture failures are not passes; no performance claim |
| AppleSwift/iOS | New records/facades/XCFramework association and stable override-free simulator run;60–180minutes plus queue | Prior340pass/2opt-in skip is baseline; no physical-device/nativeMac promise |
| AndroidKotlin | Generate/compile, current arm64/x86_64 runtime/16KB alignment and available device smoke;60–180minutes plus queue | Missingdevice legs remain unvalidated |
| Other desktopRIDs | Matching native host loader/package gates only if selected for execution | Existing linux-x64/musl/osx/win gaps stay explicit |

Approximately40–75engineering hours, not measured duration: component estimates are core/runtime/merge8–17, plugin/native/package29–52 and distinct release-close1–3, rounded for coordination/review uncertainty. Machine envelope6–18hours plus unavailable native/device queues. Shared builds, P1geometry, P5package acceptance and two branch names are not multiplicative jobs. Assumptions: retained toolchains/corpus/fonts available, no new API redesign, coordinated contract-first workers and no major unknown oracle drift. Cold DEPS, snapshot memory/clipboard and platform availability dominate uncertainty.

## Fixed-target done state and retained proof

Both branches contain exact4ad plus accepted portable/native behavior and preserved private additions. Runtime fork contains3c55 and declared fixedA/B with matching headers/symbols/provenance. Each inventory row has implementation+oracle/invariant proof, inherited implementation+proof, or explicit accepted deferral. Stale vectors/default-only fields/source overlays/override-linked libraries do not count. Final supported consumer evidence is fresh; unsupported platforms/external deployment fonts remain explicit. Required saved-byte releases/pins match final source; a localnupkg is never called publishedNuGet. Final fresh remoteSHAs, commands/exits/hashes/counts/skips/limitations are sealed. Further upstream progress is another scope.

Metadata source/proof paths and hashes are in proof-index.json and the linked inventories. Agent-owned bare metadata/new fetched objects were exported to HOST viewer-metadata.git/runtime-metadata.git; they use retained source.git object alternates, so preserve original repositories. No source/tests/build resources were changed by planning. Shared combined-container reservation ends only after both inventory readers finish.

Next action (under2minutes): review runtimeB and shared-integration recommendations with the linked inventories before authorizing implementation.
