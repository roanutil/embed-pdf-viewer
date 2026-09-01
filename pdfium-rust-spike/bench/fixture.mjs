/**
 * One PDF, one page, chosen by glyph count rather than by guess, and recorded
 * in every results file. `examples/snippet-react/public/report.pdf` is a real document from the
 * repo's own examples, not a synthetic page. Page 4 carries 8,106 glyphs,
 * the densest in the repo's example PDFs; ebook.pdf's best page has 625,
 * which is too thin to make the crossing count the dominant term.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
export const PDF_PATH = resolve(here, '../../examples/snippet-react/public/report.pdf');
export const SEARCH_TERM = 'the';

/** Picked once by `node bench/fixture.mjs`, then frozen here so every round agrees. */
export const PAGE_INDEX = Number(process.env.BENCH_PAGE ?? 4);

export function openFixture(arm) {
  const { fn, mem } = arm;
  const bytes = readFileSync(PDF_PATH);
  const buf = mem.alloc(bytes.length);
  mem.writeBytes(buf, bytes);
  const doc = fn.FPDF_LoadMemDocument(buf, bytes.length, 0);
  if (!doc) {
    mem.free(buf);
    throw new Error(`FPDF_LoadMemDocument failed for ${PDF_PATH}`);
  }
  const page = fn.FPDF_LoadPage(doc, PAGE_INDEX);
  if (!page) {
    fn.FPDF_CloseDocument(doc);
    mem.free(buf);
    throw new Error(`FPDF_LoadPage failed for index ${PAGE_INDEX}`);
  }
  const textPage = fn.FPDFText_LoadPage(page);
  if (!textPage) {
    fn.FPDF_ClosePage(page);
    fn.FPDF_CloseDocument(doc);
    mem.free(buf);
    throw new Error(`FPDFText_LoadPage failed for index ${PAGE_INDEX}`);
  }
  const glyphCount = Math.max(fn.FPDFText_CountChars(textPage), 0);

  return {
    doc,
    page,
    textPage,
    pageIndex: PAGE_INDEX,
    glyphCount,
    width: fn.FPDF_GetPageWidthF(page),
    height: fn.FPDF_GetPageHeightF(page),
    close() {
      fn.FPDFText_ClosePage(textPage);
      fn.FPDF_ClosePage(page);
      fn.FPDF_CloseDocument(doc);
      mem.free(buf);
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
