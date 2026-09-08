// The 112-byte glyph record, duplicated from crates/ops/src/record.rs and
// decoded in bench/workloads.mjs. All three must agree; the fairness gate is
// what proves they do.
#pragma once
#include <cstdint>
#include <cstring>

constexpr std::size_t kRecordBytes = 112;
// Set alongside the EPDF_CHARGEO bits when the char matrix has det < 0.
// Bit 7, the first bit above EPDF_CHARGEO_SYNTHESIZED (1u << 6); the record
// copies PDFium's flags verbatim, so this must not collide with any of them.
constexpr uint32_t kFlagAscentFlip = 1u << 7;

struct Record {
  uint32_t flags = 0;
  uint32_t object_key = 0;
  float font_size = 0;
  float rotation = 0;
  float loose_box[4] = {0, 0, 0, 0};
  float tight_box[4] = {0, 0, 0, 0};
  float loose_quad[8] = {0, 0, 0, 0, 0, 0, 0, 0};
  float tight_quad[8] = {0, 0, 0, 0, 0, 0, 0, 0};
};

static_assert(sizeof(Record) == kRecordBytes, "record layout must stay 112 bytes");

inline void WriteRecord(uint8_t* base, std::size_t index, std::size_t cap, const Record& r) {
  if (index >= cap) return;
  uint8_t* p = base + index * kRecordBytes;
  std::memcpy(p + 0, &r.flags, 4);
  std::memcpy(p + 4, &r.object_key, 4);
  std::memcpy(p + 8, &r.font_size, 4);
  std::memcpy(p + 12, &r.rotation, 4);
  std::memcpy(p + 16, r.loose_box, 16);
  std::memcpy(p + 32, r.tight_box, 16);
  std::memcpy(p + 48, r.loose_quad, 32);
  std::memcpy(p + 80, r.tight_quad, 32);
}
