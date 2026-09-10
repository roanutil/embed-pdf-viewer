// Hand-authored PDF: no renderer/library under test computes the oracle.
// Courier is a PDF standard font with 600-unit glyph advances.
import { writeFileSync } from 'node:fs';
const objects = [];
const add = (s) => (objects.push(s), objects.length);
add('<< /Type /Catalog /Pages 2 0 R >>');
add('');
add('<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>');
const pages = [];
for (let i = 1; i <= 12; i++) {
  const n = String(i).padStart(2, '0');
  const content = `q 1 0 0 rg 500 180 40 40 re f Q\nBT /F1 18 Tf 40 340 Td (Viewer baseline page ${n}) Tj 0 -50 Td (needle ${n}) Tj 0 -240 Td (Boundary ${n}) Tj ET\n`;
  const stream = add(`<< /Length ${Buffer.byteLength(content)} >>\nstream\n${content}endstream`);
  pages.push(
    add(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Resources << /Font << /F1 3 0 R >> >> /Contents ${stream} 0 R >>`,
    ),
  );
}
objects[1] = `<< /Type /Pages /Count ${pages.length} /Kids [${pages.map((p) => `${p} 0 R`).join(' ')}] >>`;
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
pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
writeFileSync(new URL('baseline.pdf', import.meta.url), pdf);
