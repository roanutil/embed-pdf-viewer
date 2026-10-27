# Engine lifecycle, buffers and coordinates

The contracts `multiplatform-plan.md` Section 5.3 makes a Phase 0 deliverable.
Read this before adding an entry point to `crates/epdf-engine`, changing how a
buffer crosses the host boundary, or touching close and cancellation.

Every clause below says where it is implemented and what proves it, or says it
is owed and by which group. A clause with no implementation is a clause
somebody still has to build; the point of writing them all down now is that the
ones that are missing are visible rather than assumed.

## 1. Close

**Close is idempotent.** Calling it twice is not an error and the second call
does nothing. `SpikeSession::close` returns whether the document was released;
`closing_an_idle_session_releases_it_at_once` covers both calls.

**Close rejects new work immediately, then drains.** From the moment close is
called, checkouts and reads fail with `DocNotOpen`. Work already outstanding is
still accepted to completion, because refusing it would deadlock the thing the
close is waiting for. `close_under_lease_waits_for_the_lease_before_releasing_the_document`.

**Close never releases a resource the host is holding.** A close that arrives
while a lease is out records the intent and returns `false`; the release
happens in `finalize`. This is the clause whose violation is a use-after-free
rather than a wrong answer, which is why the spike exists at all.

**Closing an engine closes its documents.** Owed. There is no engine object
yet; G3 brings one.

**A running synchronous PDFium call still delays disposal.** Structural, not
enforced: a synchronous call cannot be interrupted, so disposal waits. Native
cancellation sets an atomic flag and does not wait for the owner's work queue.
`crate::cancel::CancelToken` is that flag. Owed: the owner thread it belongs
to, in G3.

## 2. Poisoning

**A panic partway through a write poisons the session.** A reported failure and
a panic are different: after a failure the host knows what happened, after a
panic nobody does. A poisoned session refuses everything with
`RuntimeUnavailable`, including reads, because a read of a half-applied
structural edit is a plausible-looking wrong answer and that is worse than an
error. `a_panic_partway_through_a_write_poisons_the_session`.

**Close is the exception.** A poisoned session still owns a document, and
refusing to release it would leak for the life of the process.

**A panic before any write does not poison.** Poisoning keys on `wrote`, not on
`panicked` alone, or every transient host bug would throw away a good session.
`a_panic_before_any_write_does_not_poison`.

**Pending requests reject on worker failure.** Owed. Host-side, and it belongs
with the acknowledged event path Stage B has to add.

**Nothing unwinds across the C ABI.** Every `extern "C"` export wraps its work
in `catch_unwind`, because the workspace deliberately does not set
`panic = "abort"` and an escaping panic is undefined behaviour.
`crates/epdf-wasm/src/lib.rs`.

## 3. Admission and budgets

**Lengths and multiplications are validated before allocation.** Every product
that becomes an allocation size is `checked_mul`, and the result is refused if
it exceeds `isize::MAX`. `BufferLayout::new`, and its tests
`overflowing_layouts_are_refused_before_any_allocation` and
`a_stride_narrower_than_a_row_is_refused`.

**A document larger than the process can hold is an error, not an abort.**
`Document::from_bytes` reserves fallibly. `read_page_geometry` reserves its
glyph vector with `try_reserve` for the same reason.

**Bounded queues for jobs, open documents, input bytes, raster and cache bytes
and search output.** Owed, all of it. `crate::dispatch` describes the shape and
implements none of it. The reserved control route, so saturation cannot block a
close or a cancel, is owed with it.

**PDFium's own allocations are invisible to Rust budgets.** Structural. Any
budget built later bounds what Rust allocates, not what PDFium does behind
`FPDF_*`, and a budget that claims otherwise is lying.

**A live `WebAssembly.Memory` has no shrink API.** Measure live allocator
usage, never `byteLength`. The Phase 0 baseline measures JS heap retained per
snapshot for this reason:
`tooling/engine-baseline/RESULTS.md`.

## 4. Handles and generations

**Raw pointers never appear in a consumer API.** Stage A takes
`RawPageHandle`, a `*mut c_void`, at the host adapter boundary only, so nothing
above `epdf-wasm` names a `pdfium-sys` type. `crate::access`.

**An index is not identity.** Pages are addressed by PDF indirect object
number, which survives insertion and reordering. `crate::identity`, and
`PageObjectNumber::new` rejects `0` as `MalformedPdf` because a direct page
dictionary has no identity to carry.

**Public bindings hand out opaque handles carrying an engine generation, so a
handle from a replaced engine fails rather than resolving against new state.**
Owed. The uniffi and napi surfaces are stubs, and building generation-carrying
handles for a consumer that does not exist would be building the wrong thing
twice. G3.

**No process-wide singleton without reset.** Owed with the above.

## 5. Buffers

Anything crossing the host boundary as pixels carries its layout with it:
format, width, height and stride, validated on construction.
`crate::buffer::BufferLayout`.

**Stride is explicit and may exceed the packed row width.** Rows are addressed
by stride and stop at the bottom, never by assuming `width * bytes_per_pixel`.
`rows_are_addressed_by_stride_and_stop_at_the_bottom`.

**A buffer whose length disagrees with its layout is refused**, rather than
read to whichever of the two is shorter. `a_buffer_whose_length_disagrees_with_its_layout_is_refused`.

**Heap views are rebuilt after WASM memory growth.** Host-side. A `Uint8Array`
over the old `ArrayBuffer` is detached by growth, so no view may be cached
across a call that can allocate.

**The web `PageRender` contract is preserved**: `deviceRaster.ts` returns
`color: 'rgba8'`, `premultipliedAlpha: false` and owned bytes, using
`FPDF_REVERSE_BYTE_ORDER`. Rust's internal buffers are BGRA where PDFium
produces BGRA, so the conversion is explicit and belongs at the public service
boundary. Owed with G4; test channel order and transparent-pixel alpha there,
because a grayscale fixture cannot tell a B-to-R swap from a correct one.

## 6. Coordinates

**Geometry is PDF user space, y-up, with no device transform.** The viewer
converts through the page geometry matrix; readers never apply a y-flip.
`crate::geom`, and `read_page_geometry` emits nothing else.

**Rects are normalized so edges are ordered**, whatever order the file used.
`normalize_pdf_rect`, and `normalizing_orders_edges_whatever_order_the_file_used`.

**Quad corners are frame-geometric, not reading order.** `p1` upper-start,
`p2` upper-end, `p3` lower-start, `p4` lower-end, in the glyph's own upright
frame. Deliberately not a bidi statement: advance direction is a glyph-sequence
concern carried separately.

**Rotation is one of the four `/Rotate` values**, and page pointers are
acquired through the pool that normalizes it, so render, text, geometry and
annotation coordinates share one space. Rust must not call `FPDF_LoadPage`
itself in Stage A for exactly this reason.

## 7. Cancellation

Four cases, one per operation, from Section 7. Which case each of the 65 wire
kinds falls into is recorded in
`tooling/engine-conformance/OPERATIONS.md`, generated from
`src/operations.ts`, and CI fails if a kind is added without a row.

- `queued-only`: droppable before it starts, not after. No `throwIfAborted`
  reaches the handler.
- `cooperative`: the token is checked between units of work, before any write.
- `non-preemptible`: cooperative up to one synchronous native call that cannot
  be interrupted. The wait ends; the work completes.
- `after-first-write`: bookkeeping completes. Mutations are not
  rollback-atomic, and an `AbortError` is never evidence of rollback.

`crate::cancel::CancelToken` is the Rust side and is a flag, not an
`AbortSignal`: cancellation of waiting is a host concern and stays in
TypeScript.
