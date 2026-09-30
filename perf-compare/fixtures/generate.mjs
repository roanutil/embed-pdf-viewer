// Writes large.pdf: 400 US Letter pages, each with one filled rectangle and 40
// lines of Courier text carrying the page number and the word `the` a fixed
// number of times. Hand-written objects, so no PDF library computes the file.
// The output is committed: a generator change shows up as a changed hash in
// fixtures.json rather than as a silently different fixture.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';

// Copied from tooling/viewer-e2e/fixtures/generate.mjs on
// research/multi-platform-rust-core-impl: a tiny PDF writer, objects in, a
// valid file out.
function build(objects, rootIndex = 1) {
  let pdf = '%PDF-1.4\n';
  const offsets = [0];
  objects.forEach((obj, i) => {
    offsets.push(Buffer.byteLength(pdf));
    pdf += `${i + 1} 0 obj\n${obj}\nendobj\n`;
  });
  const xref = Buffer.byteLength(pdf);
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets
    .slice(1)
    .map((n) => `${String(n).padStart(10, '0')} 00000 n \n`)
    .join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root ${rootIndex} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return pdf;
}

export const PAGES = 400;
export const LINES = 40;
// Each line has `the` twice, so a search for `the` finds 80 per page.
const line = (page, n) => `Page ${page} line ${n}: the quick fox and the lazy dog`;

{
  const objects = [];
  const add = (s) => (objects.push(s), objects.length);
  add('<< /Type /Catalog /Pages 2 0 R >>');
  add('');
  add('<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>');
  const pages = [];
  for (let p = 1; p <= PAGES; p++) {
    let text = 'BT /F1 11 Tf 13 TL 54 730 Td\n';
    for (let n = 1; n <= LINES; n++) text += `(${line(p, n)}) Tj T*\n`;
    text += 'ET\n';
    // Vector content that differs per page, so no two pages rasterize alike.
    const shade = ((p * 37) % 100) / 100;
    const content = `q ${shade} 0.4 ${1 - shade} rg 430 40 128 128 re f Q\n${text}`;
    const stream = add(`<< /Length ${Buffer.byteLength(content)} >>\nstream\n${content}endstream`);
    pages.push(
      add(
        `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents ${stream} 0 R >>`,
      ),
    );
  }
  objects[1] = `<< /Type /Pages /Count ${pages.length} /Kids [${pages.map((p) => `${p} 0 R`).join(' ')}] >>`;
  writeFileSync(new URL('large.pdf', import.meta.url), build(objects));
}

// fixtures.json: name -> sha256, recomputed and checked at every run start.
const hash = (name) => createHash('sha256').update(readFileSync(new URL(`${name}.pdf`, import.meta.url))).digest('hex');
const fixtures = { ebook: hash('ebook'), large: hash('large') };
writeFileSync(new URL('fixtures.json', import.meta.url), JSON.stringify(fixtures, null, 2) + '\n');
console.log(fixtures);
