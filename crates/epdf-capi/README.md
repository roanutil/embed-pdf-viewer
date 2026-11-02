# Native engine C ABI

`include/epdf.h` owns signatures, status values, frames and lifetime rules.
The ABI version packs major in bits 31–16 and minor in bits 15–0. Frames carry
at most 64 binary payloads. Startup font descriptions are immutable creation
options; invalid font bytes produce `INVALID_ARGUMENT` with the Host error.

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
