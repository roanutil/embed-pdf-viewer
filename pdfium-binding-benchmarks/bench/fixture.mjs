/**
 * One PDF, one page, chosen by glyph count rather than by guess, and recorded
 * in every results file. `examples/snippet-react/public/report.pdf` is a real document from the
 * repo's own examples, not a synthetic page. Page 4 carries 8,106 glyphs,
 * the densest in the repo's example PDFs; ebook.pdf's best page has 625,
 * which is too thin to make the crossing count the dominant term.
 *
 * That page is also entirely upright (0 non-upright glyphs, 0 SPACE flags),
 * so the fairness gate needs a second fixture to exercise the rotation /
 * ascent-flip branch and the SPACE flag branch that all four arms implement.
 * `examples/viewer-react/public/rotated_text.pdf` page 0 covers both. The
 * gate compares every arm on both fixtures; TIMING keeps using only the
 * default fixture below, via `PDF_PATH`/`PAGE_INDEX`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
export const PDF_PATH = resolve(here, '../../examples/snippet-react/public/report.pdf');
export const SEARCH_TERM = 'the';

/** Picked once by `node bench/fixture.mjs`, then frozen here so every round agrees. */
export const PAGE_INDEX = Number(process.env.BENCH_PAGE ?? 4);

/**
 * Fixtures the fairness gate compares every arm on. `report-p4` is the
 * timing fixture above; `rotated-p0` is entirely non-upright with SPACE
 * glyphs, covering the two branches `report-p4` never touches; `flipped-p0`
 * has text matrices with a negative determinant, the only way to make the
 * ascent-flip bit true.
 */
export const GATE_FIXTURES = [
  { label: 'report-p4', pdfPath: PDF_PATH, pageIndex: PAGE_INDEX },
  {
    label: 'rotated-p0',
    pdfPath: resolve(here, '../../examples/viewer-react/public/rotated_text.pdf'),
    pageIndex: 0,
  },
  {
    // Every matrix in rotated_text.pdf has det > 0, so it never sets
    // ascentFlip. This page does (bench/tools/make-flipped-fixture.mjs).
    label: 'flipped-p0',
    pdfPath: resolve(here, 'fixtures/flipped_text.pdf'),
    pageIndex: 0,
  },
];

/**
 * Native arms (`arm.m.loadMemDocument`) load a document straight from a
 * Buffer and never see `mem.alloc`/`mem.writeBytes`: the napi-rs addon owns
 * the copy it makes inside `load_mem_document`, and the shipping addon (arm
 * a) exposes `FPDF_LoadMemDocument` directly over its own BigInt-pointer
 * memory, same as any other native call. Every wasm arm falls through to the
 * `mem.alloc`/`FPDF_LoadMemDocument` path exactly as before.
 */
export function openFixture(arm, { pdfPath = PDF_PATH, pageIndex = PAGE_INDEX } = {}) {
  const { fn, mem } = arm;
  const bytes = readFileSync(pdfPath);
  let buf = 0;
  let doc;
  if (arm.m.loadMemDocument) {
    doc = arm.m.loadMemDocument(Buffer.from(bytes));
  } else {
    buf = mem.alloc(bytes.length);
    mem.writeBytes(buf, bytes);
    doc = fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  }
  if (!doc) {
    if (buf) mem.free(buf);
    throw new Error(`FPDF_LoadMemDocument failed for ${pdfPath}`);
  }
  const page = fn.FPDF_LoadPage(doc, pageIndex);
  if (!page) {
    fn.FPDF_CloseDocument(doc);
    if (buf) mem.free(buf);
    throw new Error(`FPDF_LoadPage failed for index ${pageIndex}`);
  }
  const textPage = fn.FPDFText_LoadPage(page);
  if (!textPage) {
    fn.FPDF_ClosePage(page);
    fn.FPDF_CloseDocument(doc);
    if (buf) mem.free(buf);
    throw new Error(`FPDFText_LoadPage failed for index ${pageIndex}`);
  }
  const glyphCount = Math.max(fn.FPDFText_CountChars(textPage), 0);

  return {
    doc,
    page,
    textPage,
    pageIndex,
    glyphCount,
    width: fn.FPDF_GetPageWidthF(page),
    height: fn.FPDF_GetPageHeightF(page),
    close() {
      fn.FPDFText_ClosePage(textPage);
      fn.FPDF_ClosePage(page);
      fn.FPDF_CloseDocument(doc);
      if (buf) mem.free(buf);
    },
  };
}

/** `node bench/fixture.mjs` prints glyph counts so a page can be chosen. */
if (import.meta.filename === process.argv[1]) {
  const { loadArm } = await import('./arm.mjs');
  const arm = await loadArm('a');
  const bytes = readFileSync(PDF_PATH);
  const buf = arm.mem.alloc(bytes.length);
  arm.mem.writeBytes(buf, bytes);
  const doc = arm.fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  const pages = arm.fn.FPDF_GetPageCount(doc);
  const rows = [];
  for (let i = 0; i < Math.min(pages, 40); i++) {
    const p = arm.fn.FPDF_LoadPage(doc, i);
    const tp = arm.fn.FPDFText_LoadPage(p);
    rows.push({ page: i, glyphs: Math.max(arm.fn.FPDFText_CountChars(tp), 0) });
    arm.fn.FPDFText_ClosePage(tp);
    arm.fn.FPDF_ClosePage(p);
  }
  rows.sort((x, y) => y.glyphs - x.glyphs);
  console.log(`${pages} pages; densest first:`);
  for (const r of rows.slice(0, 10)) console.log(`  page ${r.page}: ${r.glyphs} glyphs`);
  arm.fn.FPDF_CloseDocument(doc);
  arm.mem.free(buf);
  arm.close();
}
