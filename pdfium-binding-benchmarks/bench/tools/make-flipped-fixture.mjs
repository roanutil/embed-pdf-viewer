#!/usr/bin/env node
/**
 * Writes bench/fixtures/flipped_text.pdf, the gate's ascent-flip fixture.
 *
 * `examples/viewer-react/public/rotated_text.pdf` covers the rotation branch,
 * but every one of its four text matrices has a POSITIVE determinant, so no
 * glyph in either gate fixture ever set ascentFlip. The producer bit arms c
 * and d OR into the record (FLAG_ASCENT_FLIP / kFlagAscentFlip) was therefore
 * never compared against arm a's JS computation of the same thing, which is
 * how it sat on PDFium's own SYNTHESIZED bit until b26d9f314 with the gate
 * green throughout.
 *
 * Three runs, same font as rotated_text.pdf:
 *   1. `1 0 0 -1`: a vertical flip. det = -1. Non-upright by the fork's
 *      IsUpright (fpdf_text.cpp:77 requires a > 0 AND d > 0), so the quad
 *      branch runs and ascentFlip must come out true.
 *   2. a 45-degree reflection. det = -1, rotation pi/4: the flip AND the
 *      rotation in one glyph run.
 *   3. a 45-degree rotation. det = +1: the control, so the fixture also has
 *      non-upright glyphs WITHOUT the flip and the gate can tell "flipped"
 *      from "merely non-upright".
 * The text contains the timing fixture's search term so the search workload
 * has hits on this page too.
 *
 * Deterministic: re-running it reproduces the committed bytes exactly.
 *
 *   node bench/tools/make-flipped-fixture.mjs
 */
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const R = '0.70710678118';
const content = [
  '0 0 Td',
  '/F1 12 Tf',
  `1 0 0 -1 40 150 Tm`,
  '(the flipped) Tj',
  '0 0 Td',
  '/F1 12 Tf',
  `${R} ${R} ${R} -${R} 100 100 Tm`,
  '(the mirror) Tj',
  '0 0 Td',
  '/F1 12 Tf',
  `${R} -${R} ${R} ${R} 40 40 Tm`,
  '(the upright) Tj',
].join('\n');

const objects = [
  '<<\n  /Type /Catalog\n  /Pages 2 0 R\n>>',
  '<<\n  /Type /Pages\n  /MediaBox [ 0 0 200 200 ]\n  /Count 1\n  /Kids [ 3 0 R ]\n>>',
  '<<\n  /Type /Page\n  /Parent 2 0 R\n  /Resources <<\n    /Font <<\n      /F1 4 0 R\n    >>\n  >>\n  /Contents 5 0 R\n>>',
  '<<\n  /Type /Font\n  /Subtype /Type1\n  /BaseFont /Times-Roman\n>>',
  `<<\n  /Length ${Buffer.byteLength(content)}\n>>\nstream\n${content}\nendstream`,
];

let pdf = '%PDF-1.7\n';
const offsets = [];
objects.forEach((body, i) => {
  offsets.push(Buffer.byteLength(pdf));
  pdf += `${i + 1} 0 obj ${body}\nendobj\n`;
});
const xref = Buffer.byteLength(pdf);
pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
for (const o of offsets) pdf += `${String(o).padStart(10, '0')} 00000 n \n`;
pdf += `trailer <<\n  /Root 1 0 R\n  /Size ${objects.length + 1}\n>>\nstartxref\n${xref}\n%%EOF\n`;

const out = resolve(import.meta.dirname, '../fixtures/flipped_text.pdf');
writeFileSync(out, pdf);
console.log(`wrote ${out} (${Buffer.byteLength(pdf)} bytes)`);
