import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

/**
 * The record producers (crates/ops/src/record.rs, cpp/record.h) copy PDFium's
 * EPDF_CHAR_GEOMETRY::flags verbatim and then OR their own ASCENT_FLIP bit in,
 * and bench/workloads.mjs decodes that bit back out. So ASCENT_FLIP must be
 * the bit directly above the highest one PDFium defines, in all three places.
 *
 * The C++ and Rust sources carry a compile-time assertion of the same rule,
 * but each can only see the flags its header (or hand-written pdfium-sys
 * mirror) names. This test reads the header the arms are actually compiled
 * against, so a new EPDF_CHARGEO_* define landing on bit 7 fails here even
 * before anyone updates a mirror.
 */
const here = import.meta.dirname;
const read = (p) => readFileSync(resolve(here, p), 'utf8');

const HEADER = 'build/libpdfium/wasm32/include/epdf_text.h';

function pdfiumFlags(headerText) {
  const flags = new Map();
  for (const m of headerText.matchAll(/#define (EPDF_CHARGEO_\w+) \(1u << (\d+)\)/g)) {
    flags.set(m[1], Number(m[2]));
  }
  return flags;
}

function shiftOf(text, pattern) {
  const m = text.match(pattern);
  assert.ok(m, `pattern not found: ${pattern}`);
  return Number(m[1]);
}

test('the header defines a contiguous flag set ending at SYNTHESIZED', () => {
  const flags = pdfiumFlags(read(`../${HEADER}`));
  assert.ok(flags.size >= 7, `expected at least 7 EPDF_CHARGEO flags, saw ${flags.size}`);
  const shifts = [...flags.values()].sort((a, b) => a - b);
  assert.deepEqual(shifts, shifts.map((_, i) => i), 'flag bits must be contiguous from 0');
  assert.equal(flags.get('EPDF_CHARGEO_SYNTHESIZED'), Math.max(...shifts));
});

test('ASCENT_FLIP is the bit directly above the highest PDFium flag, everywhere it is spelled', () => {
  const flags = pdfiumFlags(read(`../${HEADER}`));
  const expected = Math.max(...flags.values()) + 1;

  assert.equal(
    shiftOf(read('../cpp/record.h'), /constexpr uint32_t kFlagAscentFlip = 1u << (\d+);/),
    expected,
    'cpp/record.h',
  );
  assert.equal(
    shiftOf(read('../crates/ops/src/record.rs'), /pub const FLAG_ASCENT_FLIP: u32 = 1 << (\d+);/),
    expected,
    'crates/ops/src/record.rs',
  );
  assert.equal(
    shiftOf(read('./workloads.mjs'), /ASCENT_FLIP: 1 << (\d+),/),
    expected,
    'bench/workloads.mjs',
  );
});

test('the hand-written mirrors name every PDFium flag, so their guards see the whole set', () => {
  const flags = pdfiumFlags(read(`../${HEADER}`));
  const sys = read('../crates/pdfium-sys/src/lib.rs');
  const workloads = read('./workloads.mjs');
  for (const [name, shift] of flags) {
    assert.match(
      sys,
      new RegExp(`pub const ${name}: u32 = 1 << ${shift};`),
      `crates/pdfium-sys/src/lib.rs is missing ${name} = 1 << ${shift}`,
    );
    const short = name.replace('EPDF_CHARGEO_', '');
    assert.match(
      workloads,
      new RegExp(`\\b${short}: 1 << ${shift},`),
      `bench/workloads.mjs F is missing ${short} = 1 << ${shift}`,
    );
  }
});
