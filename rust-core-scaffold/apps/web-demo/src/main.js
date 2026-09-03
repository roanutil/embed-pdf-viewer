import { createEpdf } from '../../../packages/web/index.mjs';

const PAGE_INDEX = 4;
const SCALE = 2;

const set = (id, value) => { document.getElementById(id).textContent = String(value); };

const epdf = await createEpdf();
const bytes = new Uint8Array(await (await fetch('/report.pdf')).arrayBuffer());
const doc = epdf.openDocument(bytes);

set('pages', doc.pageCount());
const size = doc.pageSize(PAGE_INDEX);
set('size', `${size.width.toFixed(1)} x ${size.height.toFixed(1)} pt`);
const text = doc.pageText(PAGE_INDEX);
set('chars', text.length);
set('hits', doc.search(PAGE_INDEX, 'the', false).length);
document.getElementById('text').textContent = text.slice(0, 2000);

const bitmap = doc.renderPage(PAGE_INDEX, SCALE);
const canvas = document.getElementById('page');
canvas.width = bitmap.width;
canvas.height = bitmap.height;
canvas.style.width = `${bitmap.width / SCALE}px`;

// PDFium gives BGRA; ImageData wants RGBA. Swap in place, one pass.
const rgba = new Uint8ClampedArray(bitmap.bgra);
for (let i = 0; i < rgba.length; i += 4) {
  const b = rgba[i];
  rgba[i] = rgba[i + 2];
  rgba[i + 2] = b;
}
canvas.getContext('2d').putImageData(new ImageData(rgba, bitmap.width, bitmap.height), 0, 0);
