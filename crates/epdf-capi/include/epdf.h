#ifndef EPDF_CAPI_H
#define EPDF_CAPI_H

#include <stdint.h>

#if defined(_WIN32)
#define EPDF_CALL __cdecl
#define EPDF_API __declspec(dllimport)
#else
#define EPDF_CALL
#define EPDF_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define EPDF_ABI_VERSION UINT32_C(0x00010000)
#define EPDF_WAIT_FOREVER UINT32_MAX

typedef uint64_t EpdfHandle;
typedef uint32_t EpdfStatus;

enum {
  EPDF_OK = 0,
  EPDF_TIMEOUT = 1,
  EPDF_INVALID_ARGUMENT = 2,
  EPDF_INVALID_HANDLE = 3,
  EPDF_CLOSED = 4,
  EPDF_BUSY = 5,
  EPDF_LIMIT_EXCEEDED = 6,
  EPDF_PANICKED = 7,
  EPDF_RUNTIME_UNAVAILABLE = 8,
  EPDF_ALREADY_TAKEN = 9
};

enum {
  EPDF_OPEN_BYTES = 1,
  EPDF_CLOSE_DOCUMENT = 2,
  EPDF_LIST_PAGES = 3,
  EPDF_READ_TEXT = 4,
  EPDF_READ_GEOMETRY = 5,
  EPDF_RENDER = 6,
  EPDF_SEARCH_LITERAL = 7
};

enum {
  EPDF_TEXT_SEGMENTS = 1,
  EPDF_CHARACTER_QUAD = 2,
  EPDF_SLICE_TEXT = 3,
  EPDF_TEXT_OFFSETS_TO_CHARS = 4,
  EPDF_CHARACTER_BOUNDARY_TO_TEXT = 5,
  EPDF_PAGE_TRANSFORM = 6
};

typedef struct {
  EpdfHandle result;
  EpdfHandle document;
} EpdfReply;

/* All handles are opaque. Zero is invalid. Stale, wrong-kind and cross-engine
 * document handles are rejected; callers must not construct or modify IDs.
 * Input spans must be readable for the duration of the call. Null is valid only
 * for zero length. Outputs must be valid, aligned, distinct writable storage
 * without overlap with inputs. Writable outputs are zeroed before work.
 * Every export contains Rust unwinding; native process faults are not isolated.
 */
EPDF_API uint32_t EPDF_CALL epdf_abi_version(void);

/* Options are UTF-8 JSON; payloads are framed binary startup fonts. Registration
 * completes in order on the owner thread before this call returns an engine.
 * Errors are optional owned result handles; release any nonzero error even when
 * the status is not OK. No supplied input pointer is retained.
 */
EPDF_API EpdfStatus EPDF_CALL epdf_engine_create(
    const uint8_t *options, uint64_t options_len,
    const uint8_t *payloads, uint64_t payloads_len,
    EpdfHandle *engine, EpdfHandle *error);

/* Reserves bounded capacity and copies input before returning a job handle.
 * Args are UTF-8 JSON without routing/authority fields. Document is zero only
 * for OPEN_BYTES. Accepted jobs drain on close; explicit cancellation is
 * cooperative. Engine/helper operation implementations may return the engine
 * error code NotImplemented in a lifecycle-only build.
 */
EPDF_API EpdfStatus EPDF_CALL epdf_engine_submit(
    EpdfHandle engine, EpdfHandle document, uint32_t operation,
    const uint8_t *args, uint64_t args_len,
    const uint8_t *payloads, uint64_t payloads_len,
    EpdfHandle *job, EpdfHandle *error);

/* Zero timeout polls, WAIT_FOREVER waits indefinitely. Timeout does not cancel.
 * Collection transfers a result once; later collection is ALREADY_TAKEN. A
 * nonzero result can also accompany a failure status and must be released.
 * Host errors, including Aborted, are envelopes in completed results.
 */
EPDF_API EpdfStatus EPDF_CALL epdf_job_wait(
    EpdfHandle job, uint32_t timeout_ms, EpdfReply *reply);
EPDF_API EpdfStatus EPDF_CALL epdf_job_cancel(EpdfHandle job);
/* Drops interest, wakes waiters and signals cancellation without joining. */
EPDF_API EpdfStatus EPDF_CALL epdf_job_release(EpdfHandle job);

/* Close immediately seals admission, then waits for accepted jobs and confined
 * state disposal. Timeout leaves it closing; repeated close is safe. Release
 * invalidates the ID and requests the same drain without joining, even while
 * other calls hold internal leases. Close on a released ID is INVALID_HANDLE.
 */
EPDF_API EpdfStatus EPDF_CALL epdf_engine_close(
    EpdfHandle engine, uint32_t timeout_ms);
EPDF_API EpdfStatus EPDF_CALL epdf_engine_release(EpdfHandle engine);

EPDF_API EpdfStatus EPDF_CALL epdf_helper_call(
    uint32_t operation, const uint8_t *args, uint64_t args_len,
    EpdfHandle *result);

/* A result owns immutable framed bytes until release, independently of its job,
 * document or engine. View access must not race release. Copy if another thread
 * can release the handle. Do not free the returned pointer with a C allocator.
 * Response frame: u32 JSON length, JSON, u32 payload count, then u32 length and
 * bytes per payload, all lengths little-endian. Input payload frames start at
 * the count. Lengths/size sums are checked against explicit budgets and u32.
 */
EPDF_API EpdfStatus EPDF_CALL epdf_result_view(
    EpdfHandle result, const uint8_t **bytes, uint64_t *len);
EPDF_API EpdfStatus EPDF_CALL epdf_result_release(EpdfHandle result);

#ifdef __cplusplus
}
#endif
#endif
