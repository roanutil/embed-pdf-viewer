#include "epdf.h"
#include <assert.h>
#include <stddef.h>
#include <stdio.h>

#ifdef __cplusplus
#define EPDF_ASSERT static_assert
#else
#define EPDF_ASSERT _Static_assert
#endif

EPDF_ASSERT(sizeof(EpdfHandle) == 8, "handle width");
EPDF_ASSERT(sizeof(EpdfReply) == 16, "reply layout");
EPDF_ASSERT(offsetof(EpdfReply, document) == 8, "document offset");

int main(void) {
  assert(epdf_abi_version() == EPDF_ABI_VERSION);
  EpdfHandle engine = UINT64_MAX, error = UINT64_MAX;
  assert(epdf_engine_create(NULL, 0, NULL, 0, &engine, &error) == EPDF_OK);
  assert(engine != 0 && error == 0);

  const uint8_t *bytes = (const uint8_t *)1;
  uint64_t len = UINT64_MAX;
  assert(epdf_result_view(engine, &bytes, &len) == EPDF_INVALID_HANDLE);
  assert(bytes == NULL && len == 0);

  EpdfReply reply = {UINT64_MAX, UINT64_MAX};
  assert(epdf_job_wait(engine, 0, &reply) == EPDF_INVALID_HANDLE);
  assert(reply.result == 0 && reply.document == 0);

  EpdfHandle result = 0;
  assert(epdf_helper_call(UINT32_MAX, NULL, 0, &result) == EPDF_INVALID_ARGUMENT);
  assert(result != 0);
  assert(epdf_result_view(result, &bytes, &len) == EPDF_OK);
  assert(bytes != NULL && len >= 8);
  uint64_t held_len = len;
  assert(epdf_engine_close(engine, EPDF_WAIT_FOREVER) == EPDF_OK);
  assert(epdf_engine_close(engine, 0) == EPDF_OK);
  assert(epdf_engine_release(engine) == EPDF_OK);
  assert(epdf_engine_release(engine) == EPDF_INVALID_HANDLE);
  assert(epdf_result_view(result, &bytes, &len) == EPDF_OK);
  assert(len == held_len);
  assert(epdf_result_release(result) == EPDF_OK);
  assert(epdf_result_release(result) == EPDF_INVALID_HANDLE);
  puts("C ABI layout, calls and owned-result lifetime: PASS");
  return 0;
}
