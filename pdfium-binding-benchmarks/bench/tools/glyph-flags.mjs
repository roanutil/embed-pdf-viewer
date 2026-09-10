#!/usr/bin/env node
/**
 * EPDF_CHAR_GEOMETRY flag distribution per page.
 *
 * This is what found the hole in the fairness gate. The timing fixture,
 * `examples/snippet-react/public/report.pdf` page 4, reports:
 *
 *   total 8106  empty 1458  upright 6648  nonUpright 0  space 0
 *
 * Zero non-upright glyphs means the rotation and ascent-flip branch that all
 * four arms implement was never once compared, and neither was the SPACE
 * branch. Those are exactly where the coarse arms could diverge from arm a
 * with the gate seeing nothing, since it only checks that the answers match.
 * Running this over the repo's PDFs found `rotated_text.pdf` page 0 (30
 * non-upright glyphs, all carrying a loose quad, plus 2 spaces), which is now
 * the gate's second fixture in `bench/fixture.mjs`.
 *
 * Needs arm a built: `bash build/link-wasm.sh a`.
 *
 * Usage, from the pdfium-binding-benchmarks directory:
 *   node bench/tools/glyph-flags.mjs ../examples/viewer-react/public/rotated_text.pdf
 *   node bench/tools/glyph-flags.mjs --interesting ../examples/**\/*.pdf
 *
 * With --interesting, only pages carrying a non-upright or space glyph print,
 * which is what you want when hunting a fixture rather than inspecting one.
 */
import { readFileSync } from 'node:fs';
import { loadArm } from '../arm.mjs';

/** Bits from packages/engine/runtime/runtime-src/public/epdf_text.h:19-23. */
const F = {
  HAS_TIGHT_BOX: 1 << 0,
  HAS_LOOSE_QUAD: 1 << 1,
  HAS_TIGHT_QUAD: 1 << 2,
  UPRIGHT: 1 << 3,
  SPACE: 1 << 4,
  EMPTY: 1 << 5,
};
/** Offset of `flags` in the 124-byte struct; see bench/workloads.mjs. */
const FLAGS_OFFSET = 120;
const GEO_BYTES = 124;

const args = process.argv.slice(2);
const interestingOnly = args.includes('--interesting');
const files = args.filter((a) => a !== '--interesting');
if (files.length === 0) {
  console.error('usage: node bench/tools/glyph-flags.mjs [--interesting] <pdf> [pdf...]');
  process.exit(1);
}

const arm = await loadArm('a');
const geoPtr = arm.mem.alloc(GEO_BYTES);

for (const file of files) {
  let bytes;
  try {
    bytes = readFileSync(file);
  } catch (err) {
    console.error(`${file}: ${err.message}`);
    continue;
  }
  const buf = arm.mem.alloc(bytes.length);
  arm.mem.writeBytes(buf, bytes);
  const doc = arm.fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  if (!doc) {
    console.error(`${file}: FPDF_LoadMemDocument failed`);
    arm.mem.free(buf);
    continue;
  }

  const pages = arm.fn.FPDF_GetPageCount(doc);
  for (let i = 0; i < pages; i++) {
    const page = arm.fn.FPDF_LoadPage(doc, i);
    if (!page) continue;
    const textPage = arm.fn.FPDFText_LoadPage(page);
    if (!textPage) {
      arm.fn.FPDF_ClosePage(page);
      continue;
    }
    const glyphs = Math.max(arm.fn.FPDFText_CountChars(textPage), 0);
    const c = {
      glyphs,
      notOk: 0,
      empty: 0,
      upright: 0,
      nonUpright: 0,
      nonUprightWithQuad: 0,
      space: 0,
      hasTightBox: 0,
    };

    for (let g = 0; g < glyphs; g++) {
      if (!arm.fn.EPDFText_GetCharGeometry(textPage, g, geoPtr)) {
        c.notOk++;
        continue;
      }
      const flags = arm.mem.peek32(geoPtr + FLAGS_OFFSET) >>> 0;
      if (flags & F.EMPTY) {
        c.empty++;
        continue;
      }
      if (flags & F.UPRIGHT) c.upright++;
      else c.nonUpright++;
      if (!(flags & F.UPRIGHT) && flags & F.HAS_LOOSE_QUAD) c.nonUprightWithQuad++;
      if (flags & F.SPACE) c.space++;
      if (flags & F.HAS_TIGHT_BOX) c.hasTightBox++;
    }

    if (!interestingOnly || c.nonUpright > 0 || c.space > 0) {
      console.log(`${file.split('/').pop()} page ${i}:`, c);
    }

    arm.fn.FPDFText_ClosePage(textPage);
    arm.fn.FPDF_ClosePage(page);
  }

  arm.fn.FPDF_CloseDocument(doc);
  arm.mem.free(buf);
}

arm.mem.free(geoPtr);
arm.close();
