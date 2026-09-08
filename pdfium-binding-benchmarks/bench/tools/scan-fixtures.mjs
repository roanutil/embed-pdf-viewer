#!/usr/bin/env node
/**
 * Glyph density per page, for picking a timing fixture.
 *
 * This is how `report.pdf` page 4 got chosen. The first pick was
 * `examples/react/public/ebook.pdf`, whose densest page carries 625 glyphs,
 * which is thin enough that the boundary-crossing count stops being the
 * dominant term in the geometry workload and every arm looks better than it
 * is. Running this over six candidates found 8,106 glyphs on page 4 of
 * `examples/snippet-react/public/report.pdf`, 13x denser.
 *
 * Needs arm a built: `bash build/link-wasm.sh a`.
 *
 * Usage, from the pdfium-binding-benchmarks directory:
 *   node bench/tools/scan-fixtures.mjs ../examples/snippet-react/public/report.pdf ...
 */
import { readFileSync } from 'node:fs';
import { loadArm } from '../arm.mjs';

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error('usage: node bench/tools/scan-fixtures.mjs <pdf> [pdf...]');
  process.exit(1);
}

const arm = await loadArm('a');
const rows = [];

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
  let best = { page: -1, glyphs: 0 };
  for (let i = 0; i < pages; i++) {
    const page = arm.fn.FPDF_LoadPage(doc, i);
    if (!page) continue;
    const textPage = arm.fn.FPDFText_LoadPage(page);
    const glyphs = textPage ? Math.max(arm.fn.FPDFText_CountChars(textPage), 0) : 0;
    if (glyphs > best.glyphs) best = { page: i, glyphs };
    if (textPage) arm.fn.FPDFText_ClosePage(textPage);
    arm.fn.FPDF_ClosePage(page);
  }
  rows.push({ file, pages, ...best });

  arm.fn.FPDF_CloseDocument(doc);
  arm.mem.free(buf);
}

rows.sort((a, b) => b.glyphs - a.glyphs);
console.log('densest page per document, most glyphs first:\n');
for (const r of rows) {
  console.log(
    `${String(r.glyphs).padStart(6)} glyphs  page ${String(r.page).padStart(3)}` +
      ` of ${String(r.pages).padStart(3)}  ${r.file}`,
  );
}

arm.close();
