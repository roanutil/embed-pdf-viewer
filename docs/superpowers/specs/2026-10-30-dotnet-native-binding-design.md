# Native engine binding for .NET 10

## Intent and approved boundary

PDFAbner needs to obtain text, quads and rasters from the same PDFium engine as
the browser viewer. Its current PdfPig text and Node rendering paths use
different geometry authorities. This design exposes existing Rust capabilities
through an in-process C ABI and a typed .NET 10 facade.

The first surface includes opening bytes/passwords, document close, page
count/size/rotation/boxes, text and UTF-16 charmap, character geometry, literal
search, full/region RGBA rendering, existing page-space text-layout helpers and
page/view transforms. It excludes kernel and selection/search controllers,
annotation scenes/editing, browser Rust migration, OCR, semantic reading order,
and new quote-prefix/suffix algorithms. PDFAbner owns application policy,
PNG/compositing, match IDs, anchoring context and database persistence.

This is the implementation design for the approved scope. Product code starts
after review of this document and its implementation plan. Publishing is a
separate decision. The native package and managed facade belong to this repo;
PDFAbner owns its adapter, consumer parity harness and dependency record.

## Existing source and approach

The base is research/multi-platform-rust-core-impl-plugins at
`3dec6b60b09ddc7e16976536672de8aa4a0ff95e`, including the merged SDK toolchain.
PDFium is pinned by `packages/engine/runtime/engine-runtime-build.json` to fork
`1461b0c5105137954b77dea0a6e2c15668518651`, upstream
`9c866a447ca929bac93ed7da5f957fddb16ccde9`. Reuse that manifest.

Use a new `crates/epdf-capi` cdylib over `epdf-engine::host::Host`, retaining
`OwnerThread` confinement. A handwritten facade lives at
`platforms/dotnet/EmbedPDF`. `epdf-geometry` is used with default features off.
There is no production dependency on kernel, interaction, selection or
annotation-core. Keep C ABI ownership in this adapter and PDF algorithms in
their existing crates.

Alternatives considered: UniFFI C# generation currently has a 0.32 workspace /
0.31 generator compatibility gap that was source-inspected, not generator-tested;
it also requires expanding the typed smoke API. The experimental manual bridge
has unnecessary kernel/plugin dependencies and pointer-handle obligations.
Neither justifies coupling this engine consumer to the viewer host. Existing
epdf-wasm framing is precedent, not a native thread-safety implementation.

## Native surface

Publish `crates/epdf-capi/include/epdf.h`. All five targets are 64-bit. Use C
calling convention, fixed-width integers, `repr(C)` structs, no C++/Rust types,
and explicit UTF-8 pointer/length input. ABI major 1 is queried before use.
Opaque engine, document, job and result handles are uint64 values; zero is
invalid. Bits 63..56 encode kind (engine=1, document=2, job=3, result=4), bits
55..32 encode a 24-bit generation, and bits 31..0 encode a nonzero slot.
Registries reject wrong-kind/cross-engine handles and retire slots instead of
wrapping a generation. A document entry retains its full parent engine ID.

The following signatures are the proposed contract; names/values are frozen by
the first implementation slice and consumed by the managed interop tests:

```c
typedef uint64_t EpdfHandle;
typedef uint32_t EpdfStatus;
typedef struct { EpdfHandle result; EpdfHandle document; } EpdfReply;
uint32_t epdf_abi_version(void);
EpdfStatus epdf_engine_create(const uint8_t *options, uint64_t options_len,
                             const uint8_t *payloads, uint64_t payloads_len,
                             EpdfHandle *engine, EpdfHandle *error);
EpdfStatus epdf_engine_submit(EpdfHandle engine, EpdfHandle document,
                             uint32_t operation,
                             const uint8_t *args, uint64_t args_len,
                             const uint8_t *payloads, uint64_t payloads_len,
                             EpdfHandle *job, EpdfHandle *error);
EpdfStatus epdf_job_wait(EpdfHandle job, uint32_t timeout_ms, EpdfReply *reply);
EpdfStatus epdf_job_cancel(EpdfHandle job);
EpdfStatus epdf_job_release(EpdfHandle job);
EpdfStatus epdf_engine_close(EpdfHandle engine, uint32_t timeout_ms);
EpdfStatus epdf_engine_release(EpdfHandle engine);
EpdfStatus epdf_helper_call(uint32_t operation, const uint8_t *args,
                           uint64_t args_len, EpdfHandle *result);
EpdfStatus epdf_result_view(EpdfHandle result, const uint8_t **bytes,
                           uint64_t *len);
EpdfStatus epdf_result_release(EpdfHandle result);
```

Status values: Ok=0, Timeout=1, InvalidArgument=2, InvalidHandle=3, Closed=4,
Busy=5, LimitExceeded=6, Panicked=7, RuntimeUnavailable=8, AlreadyTaken=9.
Initialize writable output values to zero before work. Null spans are accepted
only with zero length. The C caller must provide valid, aligned, live pointers
for declared spans/outputs; arbitrary dangling addresses cannot be validated.
No unwind crosses an export, including cancellation and release.

Engine operation values: OpenBytes=1, CloseDocument=2, ListPages=3, ReadText=4,
ReadGeometry=5, Render=6, SearchLiteral=7. They map to existing Host operations
open.fatMem, close, pages.list, pages.text, pages.geometry, pages.render and
search.query. Args are operation arguments without caller-controlled kind,
docId, layerName or authority. The adapter owns document IDs/envelopes and
rejects unsupported operations and extra routing fields. No file-path open,
embedded mutation or raw unrestricted Host call is public. Startup font
registration is internal initialization, not an extra PDF mutation API.
Creation options carry an ordered startupFonts list of existing font
descriptions, keys, fallback flags and payloadIndex values; the create payload
frame carries their bytes. The owner thread registers fonts in list order and
adds fallbacks in that same order before the engine handle becomes available.
Any failed font registration fails creation and tears down on the owner thread.
No base64 font transport or mutable post-start font configuration is exposed.

The adapter supplies effect/jobId using RequestEffect and its own job identity;
it constructs local open identity/scope and uses existing SessionAuthority wire
data where required, rather than the doc-hidden fill_envelope/call_filled
convenience. It preserves existing password/PDF-permission enforcement and
never accepts caller-supplied grants, effect, scope or identity overrides.

Open uses document=0 and returns the new document handle in EpdfReply only on
success; other replies use document=0. A native registry maps that handle to
an unrepeatable Host docId and engine generation. Open succeeds only for an
unlocked usable document. Convert Host's parked password-required/rejected
answers to existing typed password errors and close parked state before
returning failure; no usable handle or failed-open bytes remain retained.
CloseDocument marks that document closing at admission, rejects later requests,
and closes it after earlier admitted operations. Closed handles cannot address
a later document.

Responses reuse Host's JSON/error vocabulary and binary framing privately:
little-endian u32 JSON length, UTF-8 JSON, u32 payload count, then each u32
length and payload. Request payload frames start at the count. Retain this
format for compatibility, with checked arithmetic/conversions before packing
or copying; do not use today's unchecked `frame::pack` for unbounded output.
Host refusal is a completed job with an error envelope. Adapter failures have
status plus an error result when allocation is possible; zero error handle
requires a stable status-derived managed exception, not a global last error.

Job wait supports timeout_ms=0 for polling and UINT32_MAX for indefinite wait.
Timeout does not cancel. Exactly one successful wait transfers the result and
optional new document handle; subsequent collection returns AlreadyTaken.
Job release drops the caller's interest and signals cancellation; the worker
retains its internal lease until completion. Abandoned results are freed;
an abandoned successful open is closed on the owner thread. A result owns its
allocation until result_release and outlives job/document/engine close. Its
view pointer is immutable and valid only while the caller holds the result;
view access must not race release. Releasing a stale handle returns an error,
never reconstructs a Box from caller-supplied pointer/length.

## Threading, close, cancellation and budgets

Host and every PDFium object are created, used and destroyed on one native
owner OS thread per engine. Add an asynchronous submission/completion primitive
to OwnerThread; preserve its existing run/close behavior for current callers.
The adapter admits bounded work before copying input, then submits closures to
that thread. An unbounded channel hidden behind a lock is not the bound.

Engine states are Starting, Open, Closing, Closed and Poisoned. Closing flips
admission immediately through shared control state, then drains accepted jobs.
It does not need a queue slot. close waits up to its timeout; Timeout leaves
the engine Closing and repeat close can await completion. Explicitly cancelled
accepted jobs complete as Aborted. Release invalidates the public handle,
requests the same drain and returns without joining on a managed finalizer
thread. Internal leases keep work alive; the native owner drops Host when its
queue drains. Add a nonblocking shutdown/detach path to OwnerThread for this
case; do not move Host destruction to an arbitrary release thread.

Cancellation lives in a job's shared CancelToken, separately from the owner
queue. Add Host::call_with_cancel(request, payloads, &CancelToken); existing
call delegates with a never-signalled token. Host currently creates that token
inside route, so an ABI-level abort message alone cannot cancel running work.
Pass the supplied token into existing readers/rendering, with no new PDF
algorithm. Cancellation is cooperative at existing safe checks; a long PDFium
call can delay it. Do not claim hard deadlines or crash isolation. A PDFium
fault/abort can terminate .NET.

Catch panic inside every submitted job, not just the C submit call. Complete
waiters deterministically, mark the engine Poisoned, reject queued/new PDF
work, and destroy state on the owner thread. Do not continue using a partially
unwound Host. Admission/control/registry locks are not held while doing PDFium
work, waiting for completion, copying output or joining threads.

Implementation defaults (adapter choices, reviewable rather than consumer
requirements): 16 admitted jobs per engine, 8 MiB JSON per call, 256 MiB total
input payloads per call/queued per engine, 1 GiB retained PDF/font input per
engine, 256 MiB result frame per call, 512 MiB retained results per engine and
16,777,216 output pixels per render. All are explicit engine options with
checked upper bounds below the u32 framing/usize ceilings. Helpers use 8 MiB
input and 256 MiB result caps and share a process-wide 1 GiB retained-result
ceiling with engine results. Enforce admission before allocation and render
dimensions/stride/pixel reservations before raster allocation. Use fallible
allocation where controlled; these limits do not bound all PDFium internals.
When result capacity is exhausted, return LimitExceeded without waiting for a
caller-held result to be released. Close/cancel/release remain usable.
The process-wide ceiling is shared: one engine's retained results can make
another engine/helper fail with LimitExceeded. It reserves no per-engine
headroom; dispose results promptly and size engine pools accordingly.

## Existing pure helpers

Helper operation values: TextSegments=1, CharacterQuad=2, SliceText=3,
TextOffsetsToChars=4, CharacterBoundaryToText=5, PageTransform=6.
All take owned snapshot/value JSON, run on the caller thread with no Host
session or PDFium access, and return owned framed results under the same
ownership/status contract. Validate finite geometry, tiling runs, charmap
invariants and range bounds before calling helpers that assume valid data.
Helper validation/algorithm refusals return an error envelope in result with
a non-Ok status. If that allocation fails, result is zero and status alone
produces a deterministic managed exception.

Use engine page_space::text::create_text_layout(...).segments/char_quad and
features::text::charmap::{slice_text_by_chars,char_range_for_text_offsets,
boundary_text_offset}. Do not depend on epdf-plugin-selection's second copy.
Its i64/malformed-run behavior is outside this validated PDFium-data surface.

PageTransform takes epdf_geometry::PageTransformInput plus lists of page
points/rects and view points/rects. Return its serialized transform metrics and
the existing page_to_view/page_to_view_rect/view_to_page/view_to_page_rect
results. No duplicated C# rotation or pixel-snapping math. PageRotation is
restricted to 0/90/180/270; size, scale and dpr must be finite and positive.

## Typed managed facade

Target net10.0; namespace/package name EmbedPDF.Engine is provisional until
package-name availability is checked. Public classes: PdfEngine, PdfDocument;
value types: PageIndex, PdfCharIndex, Utf16Offset, PdfCharRange. Coordinates are
double, never float or decimal. Keep text and geometry snapshots immutable,
preserve oriented/tight/loose/empty variants, and expose explicit charmap.

PdfEngine.CreateAsync(options) initializes startup fonts before user jobs.
PdfEngine.OpenAsync(ReadOnlyMemory<byte>, password, CancellationToken) returns
PdfDocument. Document methods are GetPagesAsync, GetTextAsync(PageIndex),
GetGeometryAsync(PageIndex), RenderAsync(PageIndex, RenderOptions) and
SearchLiteralAsync(SearchOptions), all with CancellationToken. Pure helper
methods are TextGeometry.GetSegments/GetCharacterQuad/SliceText/
GetCharacterRange/GetTextOffset and PageTransforms.Apply with the above DTOs.
No semantic ordering, regex equivalence or new quote-context policy is exposed.

SearchOptions exposes query text, case/whole-word options, snippets, optional
starting page, Cursor and per-slice MaxPages/MaxMatches. Return SearchSlice
with Matches, NextCursor, PagesSearched and PageCount, matching the existing
Rust slice. Null NextCursor means complete. Never aggregate an unbounded
search implicitly; each continuation is cancellable under the same budgets.
Reject a cursor for a different document/query.

RenderOptions exposes optional page-space Region (null means full page),
Scale (view pixels per PDF point), Rotation, Background (white/transparent),
IncludeAnnotations, IncludeFormFields and MaxOutputPixels. First scope is
display rendering, without print/encoded options. Map to existing target/
viewport/rotation/background fields and preserve Host's default annotation/
widget relationship. Return actual raster area/dimensions, not nominal scale.

Use LibraryImport with explicit Cdecl and SafeHandle wrappers around engine,
job and result IDs. SafeHandle leases protect calls/views from concurrent
managed disposal. Copy validated result spans into managed-owned values;
RgbaImage carries RGBA8 straight-alpha bytes, width/height/stride/area, and is
independent of document/engine lifetime. Native raster already requests RGBA;
do not swizzle or unpremultiply a second time.

A bounded managed channel limits waiting requests/bytes; one dispatcher per
engine submits and waits without blocking the async caller. Cancellation
callbacks only signal job_cancel; they never touch PDFium or wait under a
managed lock. DisposeAsync stops admission, drains, closes and releases.
Aborted is an existing EngineError envelope in a completed result, mapped to
OperationCanceledException, not an added ABI status value.
SafeHandle finalization only requests nonblocking native release. Do not use
Task.Run per unbounded incoming request or hide a blocking call before Task
is returned. Parallel document processing uses a bounded pool of engines.

## Coordinates, fonts and mandatory parity

Wire geometry is embedpdf-page: PDF points, origin at the top-left of CropBox
clipped to MediaBox, x right/y down, unrotated. /Rotate changes no stored
number. Host's boundary shifts/flips once. The PDFAbner adapter alone maps
zero-based PageIndex to one-based API page and supplies coordinateSpace,
pageRotation, normalizeRotation, pageWidth and pageHeight as one unit.
normalizeRotation provenance is an API-owner decision, not a binding default.
UserUnit and actual snapped render/view scale must survive transform mapping.

PDFium characters are not UTF-16 code units. The viewer's charStart/charCount
are PDFium character indices; C# string indexing uses Utf16Offset. Test zero-
text characters, surrogate pairs, normalization, ligatures and repeated text.
No PdfPig index is silently reused. Literal search uses the existing Rust
matcher; regex's documented CR/newline divergences stay outside first scope.

Pin the actual viewer package integrity, flags, engine/runtime manifests and
font bytes/order in differential results. Browser currently uses TS text and
search; rustReads changes geometry only. Compare live browser TS output and
native managed output, not merely native against historical vectors.

PDFAbner's storage gate parses EACH actual serialized JSON number token as
.NET decimal and rounds once to four places AwayFromZero. Assert identical
values after that path, ideally through its contract harness. Do not assert
raw token spelling equality, use a tolerance, pre-round doubles or convert
to decimal inside the binding. Retain tokens for diagnostics. Test positive
and negative .00005 boundaries and double serialization round trips.

Fixtures must cover all rotations, shifted/negative/oversized CropBox origins,
full coordinate metadata, selection segments, supplementary/combining/RTL/
vertical/sheared text, whitespace/empty cells, embedded and non-embedded
fonts. Pin the browser deployment's configured fallback font files, licenses
and registration order; initialize the same set before opening documents on
every native owner thread. Do not rely on OS fallback or waive failing exact
geometry. PDFAbner must supply that deployment font inventory; no universal
browser font bundle is assumed. A controlled shared test font set can prove
the harness before the deployment inventory arrives.

No new native geometry may be persisted in PDFAbner until the live corpus gate
passes. Keep this gate on every engine, viewer, runtime or font bump. Managed
tests also replay committed wire/text/charmap/layout/search vectors to catch
DTO drift. Rendering independently checks dimensions/stride/area/rotation,
transparent straight alpha and a separately agreed pixel tolerance; exact
text/geometry does not require identical raster pixels.

## Build and package delivery

Build Rust 1.98.0/Cargo.lock and .NET SDK 10.0.403. Linux builds/tests use the
rebuilt project dev container; record source commit, image ID and OS/arch in
results. Rebuilt merged-base ARM64 image is
`sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703`.
macOS/Windows native legs run manually on those hosts and record the exception.
The fork has a standing no-CI-change hold: do not add or modify any
`.github/**` file, trigger new CI, or make a slice depend on CI availability.

| RID         | Rust target               | PDFium manifest target |
| ----------- | ------------------------- | ---------------------- |
| linux-x64   | x86_64-unknown-linux-gnu  | linux-x64              |
| linux-arm64 | aarch64-unknown-linux-gnu | linux-arm64            |
| osx-arm64   | aarch64-apple-darwin      | darwin-arm64           |
| osx-x64     | x86_64-apple-darwin       | darwin-x64             |
| win-x64     | x86_64-pc-windows-msvc    | win32-x64              |

NuGet layout: lib/net10.0/EmbedPDF.Engine.dll and runtimes/{rid}/native with
both epdf-capi and libembedpdf/embedpdf.dll. Do not download code at runtime.
Replace build-machine absolute rpaths for this shipping library with $ORIGIN
on Linux and @loader_path plus verified dylib install names on macOS. On
Windows explicitly load the packaged dependency from its absolute package
path before the wrapper; do not mutate process-global DLL search directories.
Do not regress existing Rust/native/mobile loader consumers while doing so.

Run the five-platform build/test matrix manually: Linux inside the rebuilt
dev container, macOS and Windows on those hosts. Test clean relocated NuGet
consumers per RID. Produce and verify checksum-pinned tar/zip artifacts and a
local nupkg without publishing them. Carry the full source/runtime SHA, ABI,
SDK/compiler, RID, OS/libc floors, font inventory and SHA256 for each shipped
file and archive/package. Carry notices. No release, NuGet push, artifact
publication or merge is a side effect of a build/validation command.

Every run produces a checked-in record under `platforms/dotnet/validation/`:
environment (image ID or host OS/arch, emulation status and tool versions),
source/runtime/font/corpus pins, exact commands and exit codes, artifact paths
and SHA256 values, suite results, and explicit failures/skips/limitations.
Retain sanitized log references/hashes; no credentials or private PDF content.
Re-run the manual differential gate and update the record on every relevant
engine/viewer/runtime/font bump before accepting persisted geometry or a
consumer upgrade. There is no automated CI guarantee under this hold.

At design time this worker has macOS arm64 and Linux containers; no Windows
host is available, and native macOS x64 validation is not established. Those
RIDs remain unvalidated until an owner supplies a manual native-host record.
Cross-compilation, an emulated run, or a successful package build must be
labelled accurately and does not substitute for missing native-host evidence.
If the manual matrix cannot be completed, report the exact platform limitation
and withhold a full five-platform compatibility claim; do not work around the
hold by using hosted CI or silently dropping a requested RID.

Proposed support floors must be checked against the downloaded libraries and
MCP's actual image: glibc Linux, macOS 13+, Windows 10 x64. Inspect actual
symbol/deployment requirements before declaring a concrete Linux libc floor;
use an older compatible builder if Trixie raises it beyond the agreed consumer
floor. Alpine/musl is not silently covered by linux-x64/linux-arm64.

## Decisions still owned by consumers/review

Before implementation approval: review the proposed ABI/job ownership and
default budgets; coordinator assigns the slices in the companion plan.
Before consumer parity/packaging acceptance: PDFAbner provides its actual
container libc floor, viewer/font pins, normalizeRotation provenance, API
decimal contract harness and rendering pixel tolerance. These do not prevent
building the core adapter against controlled fixtures; they do block a
production compatibility claim and persisted-geometry switchover.

References: [consumer decision](https://github.com/MFB-Technologies-Inc/PDFAbner/pull/130),
[interop guidance](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/best-practices),
[native NuGet assets](https://learn.microsoft.com/en-us/nuget/create-packages/native-files-in-net-packages).
