/*
 * epdf_manual.h: the experimental hand-written C bridge (Task 11).
 *
 * Written by hand, not generated. Every struct below asserts its size and
 * field offsets, and crates/epdf-manual-bridge/tests/layout.rs checks the
 * Rust #[repr(C)] twins against the same numbers, so the two sides cannot
 * drift without a failing test. 64-bit targets only.
 *
 * Not shipped. build-xcframework.sh --experimental-manual-bridge links it
 * into a separate experimental build; the normal build never sees it.
 *
 * Ownership. Every `result` and `payload` pointer is valid only for the
 * duration of the callback, with two exceptions: an EpdfHits and an
 * EpdfPixels answered to an EpdfComplete carry an `owner` the caller frees
 * with epdf_hits_free or epdf_pixels_free, from any thread. Until then the
 * pointers inside them stay valid, which is what lets a hit list or a
 * raster cross without a copy.
 *
 * Threads. Callbacks run on the viewer's kernel thread and must not block.
 * A call made after the viewer shut down answers EPDF_ERROR_CLOSED, which
 * may happen on the calling thread before the call returns. Every
 * EpdfComplete runs exactly once.
 */
#ifndef EPDF_MANUAL_H
#define EPDF_MANUAL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── errors ─────────────────────────────────────────────────────────────── */

#define EPDF_ERROR_OK 0
#define EPDF_ERROR_KERNEL 1
#define EPDF_ERROR_CLOSED 2

/* kind EPDF_ERROR_OK has null strings. `code` is the plugin error code
 * ("operation-cancelled", "invalid-input", ...) or null; `detail` is
 * "Name: message". Free a non-ok error with epdf_error_free. */
typedef struct EpdfError {
  int32_t kind;
  char *code;
  char *detail;
} EpdfError;
_Static_assert(sizeof(EpdfError) == 24, "EpdfError");
_Static_assert(offsetof(EpdfError, kind) == 0, "EpdfError.kind");
_Static_assert(offsetof(EpdfError, code) == 8, "EpdfError.code");
_Static_assert(offsetof(EpdfError, detail) == 16, "EpdfError.detail");

void epdf_error_free(EpdfError error);

typedef void (*EpdfComplete)(void *ctx, EpdfError error, const void *result);

/* Abort a call that answered a call id. A settled call, or an unknown id,
 * is a no-op. */
void epdf_cancel(uint64_t call_id);

/* ── geometry ───────────────────────────────────────────────────────────── */

typedef struct EpdfPoint {
  double x;
  double y;
} EpdfPoint;
_Static_assert(sizeof(EpdfPoint) == 16, "EpdfPoint");
_Static_assert(offsetof(EpdfPoint, x) == 0, "EpdfPoint.x");
_Static_assert(offsetof(EpdfPoint, y) == 8, "EpdfPoint.y");

typedef struct EpdfRect {
  double x;
  double y;
  double width;
  double height;
} EpdfRect;
_Static_assert(sizeof(EpdfRect) == 32, "EpdfRect");
_Static_assert(offsetof(EpdfRect, x) == 0, "EpdfRect.x");
_Static_assert(offsetof(EpdfRect, y) == 8, "EpdfRect.y");
_Static_assert(offsetof(EpdfRect, width) == 16, "EpdfRect.width");
_Static_assert(offsetof(EpdfRect, height) == 24, "EpdfRect.height");

typedef struct EpdfQuad {
  EpdfPoint upper_start;
  EpdfPoint upper_end;
  EpdfPoint lower_start;
  EpdfPoint lower_end;
} EpdfQuad;
_Static_assert(sizeof(EpdfQuad) == 64, "EpdfQuad");
_Static_assert(offsetof(EpdfQuad, upper_start) == 0, "EpdfQuad.upper_start");
_Static_assert(offsetof(EpdfQuad, upper_end) == 16, "EpdfQuad.upper_end");
_Static_assert(offsetof(EpdfQuad, lower_start) == 32, "EpdfQuad.lower_start");
_Static_assert(offsetof(EpdfQuad, lower_end) == 48, "EpdfQuad.lower_end");

/* ── search ─────────────────────────────────────────────────────────────── */

/* TextSegment: one merged visual line of a match, in page space. */
typedef struct EpdfSegment {
  EpdfQuad quad;
  EpdfRect rect;
  double advance; /* 1 or -1 */
} EpdfSegment;
_Static_assert(sizeof(EpdfSegment) == 104, "EpdfSegment");
_Static_assert(offsetof(EpdfSegment, quad) == 0, "EpdfSegment.quad");
_Static_assert(offsetof(EpdfSegment, rect) == 64, "EpdfSegment.rect");
_Static_assert(offsetof(EpdfSegment, advance) == 96, "EpdfSegment.advance");

/* SearchHit, flat. The hit's segments are
 * EpdfHits.segments[segment_offset ..< segment_offset + segment_count].
 * `bounds` is meaningful when has_bounds; the snippet fields when
 * has_snippet, and snippet_text is snippet_len bytes of UTF-8 with no
 * terminator. */
typedef struct EpdfSearchHit {
  uint32_t page_object_number;
  uint32_t page_index;
  double char_start;
  double char_count;
  EpdfRect bounds;
  uint32_t segment_offset;
  uint32_t segment_count;
  const uint8_t *snippet_text;
  size_t snippet_len;
  double snippet_match_start;  /* UTF-16 code units */
  double snippet_match_length; /* UTF-16 code units */
  uint8_t has_bounds;
  uint8_t has_snippet;
} EpdfSearchHit;
_Static_assert(sizeof(EpdfSearchHit) == 104, "EpdfSearchHit");
_Static_assert(offsetof(EpdfSearchHit, page_object_number) == 0, "EpdfSearchHit.page_object_number");
_Static_assert(offsetof(EpdfSearchHit, page_index) == 4, "EpdfSearchHit.page_index");
_Static_assert(offsetof(EpdfSearchHit, char_start) == 8, "EpdfSearchHit.char_start");
_Static_assert(offsetof(EpdfSearchHit, char_count) == 16, "EpdfSearchHit.char_count");
_Static_assert(offsetof(EpdfSearchHit, bounds) == 24, "EpdfSearchHit.bounds");
_Static_assert(offsetof(EpdfSearchHit, segment_offset) == 56, "EpdfSearchHit.segment_offset");
_Static_assert(offsetof(EpdfSearchHit, segment_count) == 60, "EpdfSearchHit.segment_count");
_Static_assert(offsetof(EpdfSearchHit, snippet_text) == 64, "EpdfSearchHit.snippet_text");
_Static_assert(offsetof(EpdfSearchHit, snippet_len) == 72, "EpdfSearchHit.snippet_len");
_Static_assert(offsetof(EpdfSearchHit, snippet_match_start) == 80, "EpdfSearchHit.snippet_match_start");
_Static_assert(offsetof(EpdfSearchHit, snippet_match_length) == 88, "EpdfSearchHit.snippet_match_length");
_Static_assert(offsetof(EpdfSearchHit, has_bounds) == 96, "EpdfSearchHit.has_bounds");
_Static_assert(offsetof(EpdfSearchHit, has_snippet) == 97, "EpdfSearchHit.has_snippet");

typedef struct EpdfHits {
  const EpdfSearchHit *hits;
  size_t hit_count;
  const EpdfSegment *segments;
  size_t segment_count;
  void *owner; /* epdf_hits_free; null in an event payload */
} EpdfHits;
_Static_assert(sizeof(EpdfHits) == 40, "EpdfHits");
_Static_assert(offsetof(EpdfHits, hits) == 0, "EpdfHits.hits");
_Static_assert(offsetof(EpdfHits, hit_count) == 8, "EpdfHits.hit_count");
_Static_assert(offsetof(EpdfHits, segments) == 16, "EpdfHits.segments");
_Static_assert(offsetof(EpdfHits, segment_count) == 24, "EpdfHits.segment_count");
_Static_assert(offsetof(EpdfHits, owner) == 32, "EpdfHits.owner");

void epdf_hits_free(void *owner);

/* Opaque: a document's search capability, held on the kernel thread. */
typedef struct EpdfSearch EpdfSearch;

/* `kernel` is ViewerHost.experimentalKernelToken(). result: EpdfSearch *,
 * freed with epdf_search_free. */
void epdf_search_open(uint64_t kernel, const char *document_id, void *ctx, EpdfComplete done);
void epdf_search_free(EpdfSearch *search);

/* result: const uint32_t * */
void epdf_search_hit_count(EpdfSearch *search, void *ctx, EpdfComplete done);
/* result: const EpdfHits *, owner to free */
void epdf_search_hits(EpdfSearch *search, void *ctx, EpdfComplete done);
/* Start a session search, superseding a running one. Returns the call id
 * for epdf_cancel (0 if the call failed at once). result: const uint32_t *
 * hit count. A cancelled search keeps its hits and still answers. */
uint64_t epdf_search_search(EpdfSearch *search, const char *text, void *ctx, EpdfComplete done);
/* Run a query to completion, touching no session state. result:
 * const EpdfHits *, owner to free. Cancelling answers EPDF_ERROR_KERNEL
 * with code "operation-cancelled". */
uint64_t epdf_search_find_all(EpdfSearch *search, const char *text, void *ctx, EpdfComplete done);

/* Event kinds and their payloads. Strings are NUL-terminated UTF-8. */
#define EPDF_SEARCH_EVENT_STARTED 0            /* EpdfSearchStarted */
#define EPDF_SEARCH_EVENT_PROGRESS 1           /* EpdfSearchProgress */
#define EPDF_SEARCH_EVENT_COMPLETED 2          /* EpdfSearchCompleted */
#define EPDF_SEARCH_EVENT_CANCELLED 3          /* EpdfSearchCancelled */
#define EPDF_SEARCH_EVENT_FAILED 4             /* EpdfSearchFailed */
#define EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED 5 /* EpdfSearchActiveHitChanged */
#define EPDF_SEARCH_EVENT_CLEARED 6            /* null */
#define EPDF_SEARCH_EVENT_END 7                /* null: the document closed */

typedef struct EpdfSearchStarted {
  const char *text;
  const char *operation_id;
} EpdfSearchStarted;
_Static_assert(sizeof(EpdfSearchStarted) == 16, "EpdfSearchStarted");
_Static_assert(offsetof(EpdfSearchStarted, text) == 0, "EpdfSearchStarted.text");
_Static_assert(offsetof(EpdfSearchStarted, operation_id) == 8, "EpdfSearchStarted.operation_id");

typedef struct EpdfSearchProgress {
  double scanned;
  double total;
  uint32_t hit_count;
  const char *operation_id;
} EpdfSearchProgress;
_Static_assert(sizeof(EpdfSearchProgress) == 32, "EpdfSearchProgress");
_Static_assert(offsetof(EpdfSearchProgress, scanned) == 0, "EpdfSearchProgress.scanned");
_Static_assert(offsetof(EpdfSearchProgress, total) == 8, "EpdfSearchProgress.total");
_Static_assert(offsetof(EpdfSearchProgress, hit_count) == 16, "EpdfSearchProgress.hit_count");
_Static_assert(offsetof(EpdfSearchProgress, operation_id) == 24, "EpdfSearchProgress.operation_id");

typedef struct EpdfSearchCompleted {
  uint32_t hit_count;
  const char *operation_id;
} EpdfSearchCompleted;
_Static_assert(sizeof(EpdfSearchCompleted) == 16, "EpdfSearchCompleted");
_Static_assert(offsetof(EpdfSearchCompleted, hit_count) == 0, "EpdfSearchCompleted.hit_count");
_Static_assert(offsetof(EpdfSearchCompleted, operation_id) == 8, "EpdfSearchCompleted.operation_id");

typedef struct EpdfSearchCancelled {
  const char *operation_id;
  int32_t reason; /* 0 superseded, 1 cancelled, 2 cleared */
} EpdfSearchCancelled;
_Static_assert(sizeof(EpdfSearchCancelled) == 16, "EpdfSearchCancelled");
_Static_assert(offsetof(EpdfSearchCancelled, operation_id) == 0, "EpdfSearchCancelled.operation_id");
_Static_assert(offsetof(EpdfSearchCancelled, reason) == 8, "EpdfSearchCancelled.reason");

typedef struct EpdfSearchFailed {
  const char *code;
  const char *message;
  const char *operation_id;
} EpdfSearchFailed;
_Static_assert(sizeof(EpdfSearchFailed) == 24, "EpdfSearchFailed");
_Static_assert(offsetof(EpdfSearchFailed, code) == 0, "EpdfSearchFailed.code");
_Static_assert(offsetof(EpdfSearchFailed, message) == 8, "EpdfSearchFailed.message");
_Static_assert(offsetof(EpdfSearchFailed, operation_id) == 16, "EpdfSearchFailed.operation_id");

/* `hits` holds the one active hit (owner null), or is null when none is. */
typedef struct EpdfSearchActiveHitChanged {
  uint32_t index;
  const EpdfHits *hits;
} EpdfSearchActiveHitChanged;
_Static_assert(sizeof(EpdfSearchActiveHitChanged) == 16, "EpdfSearchActiveHitChanged");
_Static_assert(offsetof(EpdfSearchActiveHitChanged, index) == 0, "EpdfSearchActiveHitChanged.index");
_Static_assert(offsetof(EpdfSearchActiveHitChanged, hits) == 8, "EpdfSearchActiveHitChanged.hits");

typedef void (*EpdfSearchEvent)(void *ctx, int32_t kind, const void *payload);
/* Called once, on the kernel thread, when no event can follow. */
typedef void (*EpdfRelease)(void *ctx);

typedef struct EpdfSubscription EpdfSubscription;

/* Register for every session event. `release` may be null. */
EpdfSubscription *epdf_search_subscribe(EpdfSearch *search, void *ctx, EpdfSearchEvent on_event,
                                        EpdfRelease release);
/* Cancel and free. `release` follows on the kernel thread. */
void epdf_subscription_free(EpdfSubscription *subscription);

/* ── render ─────────────────────────────────────────────────────────────── */

/* Memory order R, G, B, A, straight (not premultiplied) alpha. */
#define EPDF_PIXEL_FORMAT_RGBA8 0

/* `data` is `len` (= stride * height) bytes, valid until
 * epdf_pixels_free(owner). It points into the buffer the render plugin's
 * raster store holds: nothing was copied to make it. */
typedef struct EpdfPixels {
  const uint8_t *data;
  size_t len;
  uint32_t width;
  uint32_t height;
  uint32_t stride;
  uint32_t format;
  double scale; /* device pixels per PDF point */
  void *owner;
} EpdfPixels;
_Static_assert(sizeof(EpdfPixels) == 48, "EpdfPixels");
_Static_assert(offsetof(EpdfPixels, data) == 0, "EpdfPixels.data");
_Static_assert(offsetof(EpdfPixels, len) == 8, "EpdfPixels.len");
_Static_assert(offsetof(EpdfPixels, width) == 16, "EpdfPixels.width");
_Static_assert(offsetof(EpdfPixels, height) == 20, "EpdfPixels.height");
_Static_assert(offsetof(EpdfPixels, stride) == 24, "EpdfPixels.stride");
_Static_assert(offsetof(EpdfPixels, format) == 28, "EpdfPixels.format");
_Static_assert(offsetof(EpdfPixels, scale) == 32, "EpdfPixels.scale");
_Static_assert(offsetof(EpdfPixels, owner) == 40, "EpdfPixels.owner");

void epdf_pixels_free(void *owner);

typedef struct EpdfRender EpdfRender;

/* result: EpdfRender *, freed with epdf_render_free. */
void epdf_render_open(uint64_t kernel, const char *document_id, void *ctx, EpdfComplete done);
void epdf_render_free(EpdfRender *render);
/* Returns the call id for epdf_cancel. result: const EpdfPixels *, owner
 * to free. */
uint64_t epdf_render_page(EpdfRender *render, uint32_t page_object_number, double scale, void *ctx,
                          EpdfComplete done);

/* ── benchmark controls ─────────────────────────────────────────────────── */

/* Returns at once on the calling thread. */
void epdf_bench_noop(void);
/* Posts an empty job to the kernel thread and answers from it. result: null. */
void epdf_bench_noop_hop(uint64_t kernel, void *ctx, EpdfComplete done);

#ifdef __cplusplus
}
#endif

#endif /* EPDF_MANUAL_H */
