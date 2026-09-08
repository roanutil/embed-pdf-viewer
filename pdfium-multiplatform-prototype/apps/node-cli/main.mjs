#!/usr/bin/env node
import { parseArgs } from 'node:util';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openDocument } from '../../packages/node/index.mjs';
import { encodeBmp } from './bmp.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const { values } = parseArgs({
  options: {
    page: { type: 'string', default: '4' },
    scale: { type: 'string', default: '1.0' },
    out: { type: 'string', default: resolve(process.cwd(), 'page.bmp') },
    pdf: { type: 'string', default: resolve(root, 'fixtures/report.pdf') },
    query: { type: 'string', default: 'the' },
  },
});

// parseArgs hands back strings; Number('four') is NaN and Number('') is 0,
// both of which PDFium would happily treat as page 0.
const invalid = (name, raw) => {
  console.error(`invalid --${name}: ${JSON.stringify(raw)}`);
  process.exit(1);
};
const pageIndex = /^\d+$/.test(values.page) ? Number(values.page) : invalid('page', values.page);
const scale = Number(values.scale);
if (values.scale.trim() === '' || !Number.isFinite(scale) || scale <= 0) invalid('scale', values.scale);

const doc = openDocument(await readFile(values.pdf));
try {
  const pageCount = doc.pageCount();
  const size = doc.pageSize(pageIndex);
  const text = doc.pageText(pageIndex);
  const hits = doc.search(pageIndex, values.query, false);
  const bitmap = doc.renderPage(pageIndex, scale);

  await writeFile(values.out, encodeBmp(bitmap));

  console.log(`pdf: ${values.pdf}`);
  console.log(`pages: ${pageCount}`);
  console.log(`page ${pageIndex}: ${size.width.toFixed(1)} x ${size.height.toFixed(1)} pt`);
  console.log(`chars: ${text.length}`);
  console.log(`hits: ${hits.length} for "${values.query}"`);
  console.log(`render: ${bitmap.width}x${bitmap.height} stride ${bitmap.stride} -> ${values.out}`);
  console.log(`text preview: ${JSON.stringify(text.slice(0, 120))}`);
} finally {
  doc.close();
}
