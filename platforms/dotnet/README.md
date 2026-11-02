# EmbedPDF for .NET 10

The `EmbedPDF.Engine` namespace exposes an in-process, typed read facade over
`epdf-capi`. The package name is provisional; this slice does not package,
relocate or publish native assets. Target framework: **net10.0 only**, 64-bit.
The scoped SDK pin is `10.0.403`.

```csharp
using EmbedPDF.Engine;

await using var engine = await PdfEngine.CreateAsync();
await using var document = await engine.OpenAsync(await File.ReadAllBytesAsync("example.pdf"));
var pages = await document.GetPagesAsync();
var text = await document.GetTextAsync(new PageIndex(0));
var geometry = await document.GetGeometryAsync(new PageIndex(0));
var segments = TextGeometry.GetSegments(geometry, new(new PdfCharIndex(0), 3));
var image = await document.RenderAsync(new PageIndex(0), new RenderOptions { Scale = 1.5 });
```

Open accepts bytes and an optional password. Reads expose unrotated page
metadata, text with explicit charmap, oriented/upright geometry, straight-alpha
RGBA8 rendering and one bounded literal-search slice. `PageInfo.Reference` is
a durable page object reference; `PageIndex` is its zero-based position.
`SearchOptions.Cursor` resumes a slice; null `SearchSlice.NextCursor` means
complete. Cursors are bound to their document/query. No implicit all-pages
search aggregation, regex, file-path native open or write API is exposed.

## Coordinates and index units

Page points/rects/quads use PDF points from the unrotated visible crop-box
upper-left, with y down. `PdfCropBox` separately preserves the original PDF
edges. Page size is never swapped for `/Rotate`; `UserUnit` and rotation remain
explicit metadata. Coordinates stay `double`, without facade rounding or
single-precision narrowing. Render options carry optional page-space region,
scale, quarter-turn rotation, background, annotation/widget flags and pixel cap;
`RgbaImage.Area`, dimensions and stride describe the actual raster. The owned
RGBA bytes need no additional swizzle or alpha conversion and remain valid
after document/engine disposal.

`PdfCharIndex` addresses PDFium's character slots; `Utf16Offset` addresses UTF-16
units in the extracted .NET string. Neither converts to the other. A dropped
character contributes a character slot without text; a supplementary character
contributes two UTF-16 units. Use the native `TextGeometry` charmap helpers for
conversions. `PdfCharRange` has character `Start`/`Count`. Geometry snapshots use
immutable arrays and distinct upright/oriented run/glyph types, preserving
loose, optional tight, empty, space and shear-only oriented variants.
`PageTransforms.Apply` delegates all transform/snapping math to the native
helper. Fold `UserUnit` into input `Scale`/`BaseScale` as the viewer does;
the helper does not silently apply it.

## Admission and lifetime

One managed dispatcher thread per engine performs native create/submit/wait/
close. Native PDFium access stays on its separate confined owner thread.
Async methods perform no native blocking work inline. A bounded channel and
byte admission limit queued and active managed requests; the defaults are 16
requests and 256 MiB. Saturation returns `PdfException` with `Code = "Busy"`
immediately. Callers choose bounded retries or use a bounded pool of engines.
`PdfEngineOptions.Limits` exposes the native limits independently.

Document disposal seals that document and queues ordered close control after
earlier admitted work. Close control needs no user-request slot or payload
budget. Engine disposal seals admission, drains requests/control, waits for
native owner-thread close and releases the engine. Concurrent/repeated disposal
shares completion. An abandoned engine can be finalized; its finalizer only
requests dispatcher shutdown and never waits for native close. Dispose documents
explicitly to release their retained PDF bytes while keeping an engine alive.
SafeHandle leases protect every native engine/job/result call and result view;
frame validation/copy finishes before releasing the view's owner.

Cancellation callbacks only call native `job_cancel`; they never wait or invoke
PDFium. Cancellation is cooperative at existing safe checks. A PDFium call can
delay cancellation, and a native process fault can terminate the application.
`Aborted` envelopes map to `OperationCanceledException`. Other native/engine
errors map to `PdfException.Code`; adapter failures also carry `NativeStatus`,
including when the native error result is unavailable. Password-required and
incorrect-password failures retain their existing typed error codes.

Startup fonts are ordered binary `StartupFont` payloads passed to
`CreateAsync`; native registration/fallback initialization completes before
creation returns. No mutable post-start font API is exposed. Input memories
must remain readable/unmodified until their owning call has accepted/copied
them; await creation before reusing startup-font buffers.

## Manual validation

Build and test inside the pinned project container, with the pinned PDFium
mounted at `/opt/pdfium`:

```sh
export PDF_RUNTIME_LIB_DIR=/opt/pdfium
export LD_LIBRARY_PATH=/opt/pdfium
cargo build -p epdf-capi
EPDF_NATIVE_DIR="$PWD/target/debug" dotnet test platforms/dotnet/EmbedPDF.Tests/EmbedPDF.Tests.csproj
```

When `CARGO_TARGET_DIR` is configured, point `EPDF_NATIVE_DIR` to its `debug`
directory. `EPDF_NATIVE_DIR` is read **only by the test assembly's resolver**;
the production facade does not consume it or widen library search paths.
From `platforms/dotnet`, the SDK resolver uses the scoped `global.json`.

Tests cover native reads/render/search/helpers, binary startup fonts, typed
errors, compile-time index misuse, all committed layout/charmap probes, wire
operation vocabulary, search DTO ranges, malformed frames, result disposal
races, GC, stale documents, concurrent disposal and admission/cancellation
stress. Repeated operations under small native input/job/result limits prove
capacity is reusable; managed slot/byte counters return to zero. The C ABI has
no public resource-counter export, so tests do not claim direct inspection of
native registries. The default annotation/widget renderer also runs in a child
test process, so native crashes are reported without losing the other results.

The checked-in validation record identifies source/image/runtime/font/fixture
pins, commands, exit codes and actual artifact hashes. Linux arm64 is the
only facade integration environment validated here. Five-RID loading/packing,
relocation, native macOS/Windows validation and live browser parity remain the
separate packaging/parity slices. No published platform support is implied.

Known upstream regression: the unchanged pinned PDFium build still crashes
with SIGSEGV in the broad engine
`document_session::a_page_held_past_close_keeps_a_registry_layers_base_alive`
test ([issue #16](https://github.com/roanutil/embed-pdf-viewer/issues/16)).
Ordinary facade default rendering is covered separately and does not reproduce
that held-layer-page case. No test is skipped and no alternate runtime is used.
