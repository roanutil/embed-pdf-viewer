# Native engine C ABI

`include/epdf.h` owns signatures, status values, frames and lifetime rules.
The ABI version packs major in bits 31–16 and minor in bits 15–0. Frames carry
at most 64 binary payloads. Startup font descriptions are immutable creation
options; invalid font bytes produce `INVALID_ARGUMENT` with the Host error.

## Ownership and control

- Create an engine, submit bounded work, collect a job once, and release the
  job. Release every nonzero result/error handle, including failure results.
- Close seals admission and drains accepted work. A timeout leaves it closing;
  repeat close to await disposal. Release invalidates the engine handle and
  requests the drain without joining, for safe finalizer use.
- Cancellation signals the job directly. It is cooperative and does not
  interrupt arbitrary native calls. Releasing a job abandons its result and
  signals cancellation. Process faults are not isolated.
- Results outlive their engine until released. A view is borrowed, immutable
  memory; do not race view access with release or free it with another allocator.
- Opaque tagged generation handles reject stale/wrong-kind/wrong-parent IDs.
  Invalid memory addresses are still a C caller contract violation. Input spans
  and aligned output storage must be valid and must not overlap.

Every C export contains unwinding, and each owner job catches failure through
result framing/completion. A panic closes/poisons the engine and completes
pending work. Its status remains `Panicked` even if no error buffer can fit.

## Creation options

Options are UTF-8 JSON, or an empty span for defaults. Unknown fields are
rejected. `limits` accepts the following camelCase settings:

| Setting                | Default    |
| ---------------------- | ---------- |
| maxPendingJobs         | 16         |
| maxJsonBytes           | 8 MiB      |
| maxPayloadBytes        | 256 MiB    |
| maxQueuedInputBytes    | 256 MiB    |
| maxRetainedInputBytes  | 1 GiB      |
| maxResultBytes         | 256 MiB    |
| maxRetainedResultBytes | 512 MiB    |
| maxOutputPixels        | 16,777,216 |

Settings must be positive and fit the checked ABI/framing ceilings. Creation
options have an 8 MiB bootstrap JSON limit. Result frames across all engines
and helpers also share a 1 GiB ceiling, so one engine's retained results can
exhaust another's capacity. Dispose results promptly. These budgets do not
bound allocator overhead or PDFium's own internal allocations.

`startupFonts` is an ordered array of `{key, payloadIndex, familyName?, weight?,
italic?, fallback?}`. Use unique keys and exactly one distinct payload per font.
The binary payload frame contains their bytes. Omitted family/weight/italic
infer font metadata; `italic` is boolean or null. All fonts and fallbacks are
registered on the owner thread before creation returns. A failed registration
tears down that state on the same thread. Font memory stays charged until
owner-thread disposal.

## Engine operations

Arguments use the existing Host JSON vocabulary, without envelope fields.
The adapter supplies the document identity, job identity, effect and local
session scope. Extra top-level fields are rejected before Host execution.

| Operation | Arguments | Payloads |
| --- | --- | --- |
| `OPEN_BYTES` | `{ "password": null }` (password optional) | PDF bytes as the first payload |
| `CLOSE_DOCUMENT` | `{}` | None |
| `LIST_PAGES` | `{}` | None |
| `READ_TEXT` | `{ "page": pageRef }` | None |
| `READ_GEOMETRY` | `{ "page": pageRef }` | None |
| `RENDER` | `{ "page": pageRef, "options": renderOptions }` | None |
| `SEARCH_LITERAL` | `{ "request": searchRequest }` | None |

Use page references from `LIST_PAGES.result.pages[].ref`; the listing also
carries zero-based indices, unrotated dimensions, rotation, boxes and UserUnit.
Text, geometry, search segments and raster areas use the Host's unrotated
visible-crop page space. Geometry is not rounded by this adapter.

Display render options accept `target` (`{ "kind": "page" }` or
`{ "kind": "rect", "rect": { "x": 0, "y": 0, "width": 20, "height": 10 } }`),
`viewport` (`{ "kind": "scale", "scale": 1 }`), quarter-turn `rotation`,
`background` (`white` or `transparent`), `includeAnnotations`,
`includeFormFields` and `maxOutputPixels`. The engine clamps pixel capacity
to its configured limits and reserves engine/process output headroom before
raster allocation. The existing renderer checks dimensions, stride and byte
lengths. The returned payload is straight-alpha RGBA; do not swap channels
or unpremultiply it again. Print and encoded output are outside this surface.

Literal search accepts the existing `text`, `matchCase`, `matchDiacritics`,
`wholeWord`, `ignoreWhitespace`, `snippets`, optional starting-page `from`,
`cursor` and `limit: { "pages": 1, "matches": 256 }` fields. Each call returns
one `SearchSlice`: `matches`, `nextCursor`, cumulative `pagesSearched` and
`pageCount`. A null cursor ends the scan. Limits preserve the Host's page
boundaries: a page's matches are not split across calls. Continuations are
opaque and bound to the document and query. Regex and trusted `skip` positions
are refused; no unbounded aggregation happens in the adapter.

## Collection and cancellation

Open transfers a checked document handle only for an unlocked usable document.
Missing/rejected passwords become `DocPasswordRequired`/`DocPasswordIncorrect`
error envelopes, and parked Host sessions are closed before returning failure.
Close invalidates its document at admission and runs after earlier admitted
operations. Cancellation of an admitted close still disposes that document.

A result-registry refusal preserves the completion for a later collection
attempt. Releasing an uncollected successful open closes it on the owner thread
and releases retained PDF bytes. Failed result framing does the same. Result
allocations survive document/engine close until explicitly released.

Cancellation uses the job's shared token at existing Host/read/render/search
checks. It is cooperative; a native call can delay observation. A cancelled
operation reports the existing `Aborted` error envelope. Poisoning rejects
submissions with `PANICKED`; close still disposes confined state and reports
`OK` after disposal. Cleanup does not re-enter poisoned Host state.

## Pure helpers

`epdf_helper_call` runs on the caller's thread. It never touches a Host
session, the owner thread or PDFium. Arguments are one UTF-8 JSON object of at
most 8 MiB with no unknown fields. A success is `{"result": ...}` with status
`EPDF_OK`; a refusal is an `InvalidArg` error envelope with
`EPDF_INVALID_ARGUMENT`. Frames carry no binary payloads and count against the
256 MiB helper result cap and the shared 1 GiB ceiling.

| Opcode                                | Arguments                                                   | Result                                                                  |
| ------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------------------- |
| `EPDF_TEXT_SEGMENTS` (1)              | `{geometry, range: {start, count}}`                         | segments `[{quad, rect, advance}]`                                      |
| `EPDF_CHARACTER_QUAD` (2)             | `{geometry, index}`                                         | quad or `null`                                                          |
| `EPDF_SLICE_TEXT` (3)                 | `{text, range: {start, end}}` in characters                 | string                                                                  |
| `EPDF_TEXT_OFFSETS_TO_CHARS` (4)      | `{text, range: {start, end}}` in UTF-16 units               | `{start, end}` in characters                                            |
| `EPDF_CHARACTER_BOUNDARY_TO_TEXT` (5) | `{text, boundary}`                                          | UTF-16 offset                                                           |
| `EPDF_PAGE_TRANSFORM` (6)             | `{input, pagePoints?, pageRects?, viewPoints?, viewRects?}` | `{transform, pageToView, pageRectsToView, viewToPage, viewRectsToPage}` |

`geometry` is a page-space `pages.geometry` snapshot and `text` a `pages.text`
snapshot (`text`, `charCount`, `charMap`). The layout is the engine's
`page_space::text::create_text_layout` and the charmap is
`features::text::charmap`, so answers match the shared
`tooling/engine-conformance/vectors/text` vectors.

We refuse input those algorithms would otherwise clamp: runs that don't tile
characters `0..n` in order, empty runs, non-finite geometry, a charmap that
fails `char_map_violation`, and indices or ranges past the page. An inverted
range isn't malformed. It keeps the empty answer the vectors pin.

`input` is `epdf_geometry::PageTransformInput`: `pageSize`, `rotation` (0, 90,
180 or 270), `scale`, optional `baseScale` and `dpr`, all finite and positive.
Fold `/UserUnit` into `scale` and `baseScale` as the viewer does. The metrics
include the snapped device size. The inverse determinant must be finite and
nonzero even when no points or rectangles are requested. Singular transforms
and non-finite metrics or mapped values are refused.

Arguments decode with exact decimal-to-double conversion, so a shortest
round-trip spelling reaches the algorithm bit for bit. That feature also
applies to the engine's own JSON parsing in any build that links this crate.

## Build and manual validation

Use the rebuilt project dev container and pinned runtime artifacts:

```sh
cargo test -p epdf-capi
cargo test -p epdf-capi --test helpers
cargo test -p epdf-engine --lib dispatch::tests
cargo build -p epdf-capi
mkdir -p target/manual
clang -std=c11 -Wall -Wextra -Werror -Icrates/epdf-capi/include \
  crates/epdf-capi/tests/c_smoke.c -Ltarget/debug -lepdf_capi \
  -Wl,-rpath,"$PWD/target/debug" -o target/manual/c-smoke
target/manual/c-smoke
```

Record source/runtime/font pins, image or host environment, commands, artifact
SHA256s and outcomes using [`platforms/dotnet/validation`](../../platforms/dotnet/validation/README.md).
No `.github/**` changes, hosted checks, package publication or release action
is part of this slice. Only environments with recorded evidence are validated.
