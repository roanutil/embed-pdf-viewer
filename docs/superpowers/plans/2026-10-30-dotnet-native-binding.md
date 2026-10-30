# Native .NET Engine Binding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship reviewable native C ABI and .NET 10 package outputs for existing
engine reads/rendering and selected pure text/geometry helpers on five RIDs.

**Architecture:** epdf-capi owns checked handles, bounded jobs and result
lifetimes; Host and PDFium stay on OwnerThread. A LibraryImport facade maps
typed .NET operations onto that adapter. Existing algorithms remain in Rust.

**Tech Stack:** Rust 1.98.0, PDFium manifest pin 1461b0c5, .NET SDK 10.0.403,
C ABI, NuGet, current TS viewer as differential reference.

**Spec:** `docs/superpowers/specs/2026-10-30-dotnet-native-binding-design.md`

## Global constraints

- Implement the spec's exact C signatures/status/opcode/handle/limit contracts;
  no unrestricted Host calls, kernel/controller dependencies, annotation scenes,
  browser migration, new PDF/quote algorithms or release publishing.
- Rebuild `.devcontainer/Dockerfile` from the current merged base before work.
  Linux builds/tests run in that image; record image ID/source/OS/arch per run.
  Current rebuilt ARM64 ID is
  `sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703`.
  macOS/Windows tests run manually on those hosts; declare the exception.
- Preserve existing OwnerThread/Host callers. Native pointer validity remains
  the C caller's obligation; checked handles do not validate arbitrary memory.
- Double geometry stays unrounded until PDFAbner's actual serialized-token
  decimal parse + 4dp AwayFromZero gate. No geometry persistence before parity.
- Five RIDs: linux-x64, linux-arm64, osx-arm64, osx-x64, win-x64. No merge,
  release tag, NuGet push or public artifact publication is implicit.
- Standing hold: no `.github/**` changes, new CI or CI dependencies. Every
  validation run has a checked-in environment/commands/SHA256/results record;
  missing platform access is a declared limitation, never a CI workaround.

## Review focus

Stale/cross-kind handles and call/release races; close/cancel under saturated
admission; supplementary/dropped characters and oriented quads; shifted crop
origins with rotation and rounding midpoints; relocated loading and configured
fallback fonts. Each has an owning verification below.

## Ownership, dependencies and order

Coordinator radiant-linnet assigns implementers before execution. Proposed
primary is mild-pangolin; lunar-echidna reviews the ABI/parity/package boundaries.
These are proposed allocations, not claims that implementation has started.

| Slice | Owner/files                                                             | Depends on                             | Reviewable deliverable                                      |
| ----- | ----------------------------------------------------------------------- | -------------------------------------- | ----------------------------------------------------------- |
| 0     | Complete: merged PR14, `.devcontainer/`                                 | none                                   | Pinned SDK and validated ARM64/AMD64 images                 |
| 1     | Native implementer: epdf-capi ABI/handles/results/jobs; engine dispatch | design approval                        | Memory/thread/lifecycle ABI with C tests                    |
| 2     | Native implementer: epdf-capi engine/operations; Host cancellation      | 1                                      | Bytes -> metadata/text/geometry/region RGBA, literal search |
| 3     | Helper implementer: epdf-capi helpers only                              | 1 ABI freeze                           | Existing text-layout and page/view transforms               |
| 4     | Managed implementer: platforms/dotnet                                   | 1 header, 2, 3                         | Typed .NET facade + wire-vector/interop tests               |
| 5     | Parity implementer + PDFAbner consumer owner                            | 2, 3, 4                                | Live browser/native storage-boundary gate                   |
| 6     | Packaging implementer: build-support/capi build + manual tooling        | 2, 3, 4; release acceptance requires 5 | Five locally verified RID/NuGet outputs and run records     |
| 7     | Primary + independent reviewer                                          | 1–6                                    | Docs, final review and consumer handoff PRs                 |

Slices 2 and 3 can run independently after header/registry freeze. Slice 6 can
build alongside 5, but cannot claim consumer compatibility before 5 passes.
Only one implementer edits shared ABI/registry files; helpers consume those
interfaces rather than modifying them concurrently.

### Slice 1: Checked ABI, results and owner-thread jobs

**Files:** Create `crates/epdf-capi/{Cargo.toml,src/lib.rs,src/abi.rs,src/handles.rs,
src/buffers.rs,src/jobs.rs,src/engine.rs,include/epdf.h,tests/lifecycle.rs,
tests/c_smoke.c}`; modify root `Cargo.toml`/`Cargo.lock` and
`crates/epdf-engine/src/dispatch.rs`.
Create `platforms/dotnet/validation/{README.md,record.schema.json}` and this
slice's manual run records. The schema is shared test infrastructure available
before parity/packaging; its fields are listed in slice 6.

**Interfaces:** Export the spec header verbatim. Rust registry typed wrappers
are EngineId/DocumentId/JobId/ResultId over u64. Add
`OwnerThread::submit<T>(job) -> Result<OwnerResult<T>, EngineError>` with T:Send,
`OwnerResult::wait(self) -> Result<T, EngineError>`, and
`OwnerThread::shutdown_detached(&mut self)`; preserve run/close behavior.

- [ ] Write lifecycle tests: wrong tag/generation/parent rejected; slot retirement
      prevents reuse after generation exhaustion; zero outputs on refusal; result
      lives after engine close; duplicate result/job release is safe; wait transfers
      exactly once and timeout does not cancel. Use fake owner state, not a PDF.
- [ ] Run `cargo test -p epdf-capi --test lifecycle` in the rebuilt container;
      verify the intended missing-implementation failures before implementing.
- [ ] Implement bounded admission, exact default byte/pixel limits, immutable
      result registry with checked framing, and job wait/cancel/release. Document
      C pointer validity and view/release races in the header. Use fallible reserves.
- [ ] Add saturation tests: close seals immediately with all 16 permits held;
      cancel/release need no queue slot; detached release does not block finalizer;
      accepted work drains/drops state on owner thread; panic completes waiters,
      poisons engine and prevents later state access. Assert thread IDs/drop counts.
- [ ] Verify create-time ordered binary font payloads reach registration on the
      owner thread before engine admission opens; any failure unwinds registration
      and closes on that thread. Test aggregate startup-font limits without base64.
- [ ] Run `cargo test -p epdf-capi --test lifecycle` and
      `cargo test -p epdf-engine dispatch::tests`; compile/run c_smoke.c against the
      cdylib/header to verify calling convention, sizes and exported symbols.
      Commit only when tests pass; share the frozen header with slices 2–4.

### Slice 2: Existing Host reads and cooperative cancellation

**Files:** Create `crates/epdf-capi/src/operations.rs`,
`crates/epdf-capi/tests/{engine_reads.rs,cancellation.rs}`; extend engine.rs;
modify `crates/epdf-engine/src/host.rs` and its focused tests.

**Interfaces:** `Host::call_with_cancel(&mut self, request: &str,
payloads: Payloads, cancel: &CancelToken) -> HostResponse`; existing call remains.
Map ABI operations 1–7 to the spec's Host routes. Only the adapter generates
docIds/authority/routing fields. EpdfReply.document transfers a checked open
handle; other operations return zero in that field.

- [ ] Add failing tests using existing engine fixtures: bytes/password open,
      wrong password, malformed PDF, metadata/rotation, text/map, geometry, close
      and cross-engine/stale document handles. Reject unsupported opcodes and
      caller-provided routing/authority fields before Host execution.
- [ ] Translate parked password-required/rejected opens to typed errors and
      close parked state; test repeated failures do not retain bytes/documents.
      Construct current JobEnvelope/RequestEffect and local session authority
      explicitly, without doc-hidden fill_envelope/call_filled helpers.
- [ ] Add a controllable token test proving Host uses supplied cancellation
      during existing read/render checks; call still behaves as before. Write
      abandoned-open/job tests proving no document/result ownership is lost.
- [ ] Implement operation adapters and supplied-token Host routing without
      adding PDF feature algorithms. Enforce output pixel/stride/length limits
      before raster allocation and checked framing before result handoff.
- [ ] Test full/region renders for every rotation, shifted CropBox, transparent
      straight RGBA, output dimensions/stride/area, oversized/NaN inputs, cancellation
      and literal-search cursor ranges/segments. Do not promise regex or PNG.
      Return bounded SearchSlice with NextCursor/PagesSearched/PageCount, never
      hidden unbounded aggregation; test continuation/cancel and wrong-query/doc
      cursor rejection against existing behavior.
- [ ] Run `cargo test -p epdf-capi --test engine_reads --test cancellation`
      and `cargo test -p epdf-engine --test text_corpus --test geometry_corpus --test search_dialect`;
      commit passing adapter and invariants.

### Slice 3: Pure layout/charmap and transform exports

**Files:** Create `crates/epdf-capi/src/helpers/{mod.rs,text.rs,transform.rs}`,
`crates/epdf-capi/tests/helpers.rs`; add epdf-geometry dependency with
default-features=false. Do not change plugin algorithms.

**Interfaces:** epdf_helper_call opcodes 1–6 and result contract from the spec.
Text ops consume existing page-space text/geometry snapshots and explicit
character/UTF-16 ranges. Transform consumes PageTransformInput and four arrays
of page/view points/rectangles, returning metrics and transformed arrays.

- [ ] Write vector-driven tests for all six operations; include supplementary
      and zero-text chars, empty ranges, RTL/vertical/sheared merged segments,
      malformed/non-tiling snapshots and out-of-range indices. Pin rejection rather
      than accidentally inheriting selection's permissive malformed-host behavior.
- [ ] Run `cargo test -p epdf-capi --test helpers` and observe intended failure.
- [ ] Bind existing engine page-space TextLayout/charmap and epdf-geometry
      page_transform methods. Validate inputs and share result/framing ownership;
      do not send pure helpers through Host or the PDFium owner thread.
- [ ] Compare transforms at all quarter turns with actual snapped viewport,
      dpr, baseScale and UserUnit handling; reject singular/non-finite input.
      Consume committed layout/charmap and geometry vectors rather than recreate
      the expected algorithm in tests. Run helper tests green and commit.

### Slice 4: Typed .NET 10 facade

**Files:** Create `platforms/dotnet/{EmbedPDF.sln,Directory.Build.props,global.json,
README.md,EmbedPDF/EmbedPDF.csproj,EmbedPDF/NativeMethods.cs,
EmbedPDF/NativeHandles.cs,EmbedPDF/PdfEngine.cs,EmbedPDF/PdfDocument.cs,
EmbedPDF/Models.cs,EmbedPDF/TextGeometry.cs,EmbedPDF/PageTransforms.cs,
EmbedPDF.Tests/EmbedPDF.Tests.csproj}` and focused tests in that test project.

**Interfaces:** net10.0 only; scoped global.json pins 10.0.403. Implement the
spec's named public classes/methods. PageInfo carries PageIndex, distinct
page reference, unrotated size/boxes/rotation/UserUnit. PageTextSnapshot and
PageGeometrySnapshot preserve the Rust DTO data. RgbaImage has owned bytes,
Width/Height/Stride/Area. PdfCharRange uses character Start/Count; Utf16Offset
never implicitly converts to/from PdfCharIndex.
SearchOptions/SearchSlice expose the spec's bounded cursor contract.
RenderOptions contains the spec's explicit region/scale/rotation/background/
annotation/widget/pixel fields. StartupFonts pass binary create payloads in
the configured order, before OpenAsync can admit a user request.

- [ ] Write integration tests against locally built native libraries for Create/Open/
      metadata/text/geometry/render/search/helper calls, cancellation, stale child
      documents, repeated/concurrent dispose, retained images after close and
      exception mapping. Add compile-time misuse tests for index unit conversions.
      Use `cargo build -p epdf-capi` from slices 2–3 and a test-only EPDF_NATIVE_DIR
      resolver pointing to that build output. Packed/relocated tests belong to
      slice 6, avoiding a dependency cycle between facade tests and packaging.
- [ ] Write DTO/frame tests consuming existing committed
      `tooling/engine-conformance/vectors/{wire,text,search}`. Check every supported
      variant and null/empty distinctions; unknown required variants fail clearly.
- [ ] Implement Cdecl LibraryImport, SafeHandle leases, bounded managed channel
      and async methods that return without inline native blocking. Only one
      dispatcher per engine waits on native jobs. Copy result views under a live
      lease; no Marshal.FreeHGlobal, float narrowing or pre-rounding.
- [ ] Run `dotnet test platforms/dotnet/EmbedPDF.Tests/EmbedPDF.Tests.csproj`
      in the rebuilt container with the matching native library. A stress test
      cancels/disposes queued work under the configured byte/slot bounds; all
      waiters terminate, resource counters return to baseline. Commit when green.

### Slice 5: Live viewer/native parity gate

**Files:** Create `tooling/dotnet-parity/{README.md,manifest.json,run.mjs,
fixtures/}` and a .NET parity runner project under platforms/dotnet.
Reuse `tooling/viewer-e2e/playwright.config.ts`, its real viewer bridge/entry
at `examples/viewer-react/e2e.html`, and its packed-artifact mode; extend the
read-only bridge for corpus snapshots rather than build a substitute viewer.
Load the exact manifest viewer package/flags and assert the actual backend;
never relabel a fallback path. PDFAbner supplies its consumer serialization/
decimal harness and deployment manifest.

**Interfaces:** Corpus manifest contains PDF SHA256, viewer package integrity/
flags, source/runtime pins, fonts SHA256/order and environment. Runners emit
the same operation/request identifiers, raw JSON tokens, text/maps/segments,
coordinate metadata and render descriptors. Comparator calls the consumer's
decimal parse/round harness; it does not reimplement a binary-double tolerance.

- [ ] Add cases where native and viewer intentionally differ at .00005 and
      crop/rotation boundaries; verify the runner rejects them. Include surrogate/
      dropped/combining/RTL/vertical/sheared chars, repeated quotes, empty cells,
      nonzero crop origins beyond MediaBox, all rotations and transparent renders.
- [ ] Compare live TS viewer paths to managed native results, including
      arbitrary selection segments and page/view transforms. Pin a controlled
      shared font set initially, then the actual supplied deployment font bundle.
- [ ] Run `node tooling/dotnet-parity/run.mjs --manifest tooling/dotnet-parity/manifest.json`
      in-container and attach machine-readable
      per-case evidence. Exact text/maps/rounded geometry and metadata must match;
      apply a separately approved tolerance only to raster pixels.
- [ ] PDFAbner consumer owner verifies one-based page conversion, complete
      provenance and actual API serialization/decimal behavior. Until deployment
      font inventory, libc floor and normalizeRotation are settled and the gate
      passes, prohibit persisted-geometry switchover. Re-run this gate manually
      on each relevant pin bump and check in the environment, exact commands,
      artifact SHA256s and results using slice 1's validation-record schema.

### Slice 6: Relocated native builds and five-RID packaging

**Files:** Create `crates/epdf-capi/build.rs`,
`platforms/dotnet/{pack.sh,package-manifest.schema.json,validate.sh,validate.ps1}`,
per-run JSON records under `platforms/dotnet/validation/`; consume the schema
created in slice 1. Add native asset packing rules and modify build-support narrowly only
if needed. Keep existing native/mobile loader paths working. Do not create or
modify `.github/**`; no step relies on a hosted runner or CI result.

**Interfaces:** `bash platforms/dotnet/pack.sh --rid <rid> --out <directory>`
consumes Cargo release library, the existing checksum-verified runtime manifest,
font manifest and managed build; emits RID archive, SHA256 manifest and local
nupkg. No command invokes a release publication or runtime download.
`validate.sh --rid <rid> --record <path>` runs manually in the Linux container
or macOS host; `validate.ps1 -Rid win-x64 -Record <path>` does so on Windows.
Both validate the same checked-in record schema. Each record includes
source/runtime/font/corpus pins, image digest or host OS/arch, emulation,
toolchain versions, exact commands/exit codes, artifact relative paths/SHA256s,
suite counts and failures/skips/limitations. Commit records, not private PDFs,
credentials or generated binaries. Slice 5 uses the same slice-1 schema before
packaging is complete; parity/facade tests never depend on package generation.

- [ ] Add loader tests that relocate packed outputs away from the build tree
      and fail on missing/wrong RID, dependency, ABI or runtime identity. Verify
      all shipped dynamic dependencies and symbols; a build-machine rpath fails.
- [ ] Implement $ORIGIN/@loader_path+install-name handling for this cdylib;
      Windows resolver loads packaged PDFium by absolute path before wrapper.
      Test manually on native OS hosts; do not widen process DLL search paths.
- [ ] Build/test the five spec targets with libclang and exact Rust/SDK pins.
      Linux legs use project-container builds; macOS/Windows use their hosts
      manually and record exceptions, image/source/compiler/OS floors. Validate the MCP libc requirement
      against actual ELF symbols before declaring Linux compatibility.
- [ ] Pack runtimes/{rid}/native with both libraries and required font/notices;
      checksum every delivered file, verify source/runtime/RID metadata and unpack
      a fresh .NET consumer for each RID. Run its open/text/region render/helper
      smoke and applicable parity corpus manually. Record artifact/package
      SHA256s and outcomes in version control; keep produced binaries local.
- [ ] Run `dotnet pack platforms/dotnet/EmbedPDF/EmbedPDF.csproj -c Release`
      using the verified RID assets. Release acceptance requires slice 5; package
      production claims remain blocked if any requested native leg is untested.
- [ ] If a requested platform cannot be tested locally, record it as unvalidated
      with the missing host/toolchain and required manual follow-up. Currently
      this worker lacks Windows and established native macOS x64 validation.
      Do not substitute hosted CI, emulation or cross-compilation for native
      evidence, or claim all five targets pass. An unavailable leg is a stated
      delivery limitation; slice 1 and other validated legs may still proceed.

### Slice 7: Final review and consumer handoff

**Files:** Update `platforms/dotnet/README.md`, header/API docs and the completed
slice records. Audit package changesets using embedpdf-changesets.

- [ ] Document supported capabilities, C lifetime/cancellation limits, startup
      font policy, coordinate/index units, exact tested platform floors, build/
      install steps, known limitations and versioned upgrade/parity process.
- [ ] Run the focused Rust/.NET/relocated-package/live-parity checks appropriate
      to the final diff; record commands/results and environment IDs. Run
      `git diff --check` and changeset status, explaining internal-tooling/Rust-only
      changes that do not release a JS package. Do not invent unrelated bumps.
- [ ] Independent reviewer checks the whole branch plus shared-interface
      changes. Resolve blocking findings with reproducing tests, then open the
      implementation PR(s) into research/multi-platform-rust-core-impl-plugins.
      Provide PDFAbner the reviewed header/facade/artifact manifest and parity
      evidence. Human approval is required before merge or publication.
- [ ] Confirm `git diff --name-only <base>...HEAD` contains no `.github/` paths.
      Review the checked-in manual run records and verify every claimed RID's
      commands, environment, artifact hashes and outcomes. State unvalidated
      platforms explicitly in the handoff; do not infer support from packaging.

## Immediate next action and current status

Design/plan drafting only. Toolchain slice 0 is merged and rebuilt; product
slices 1–7 have not started. The fresh design builder check found nothing
important unanswered; independent channel review is requested before handoff.
Coordinator reviews the ABI/default-budget choices, confirms allocations and
obtains design/plan approval, then assigns slice 1. Open consumer deployment
details are explicit acceptance dependencies, not reasons to stall the first
controlled-fixture native slice.
