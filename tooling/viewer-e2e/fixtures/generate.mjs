// Hand-authored PDFs: no renderer/library under test computes the oracle.
// Courier is a PDF standard font with 600-unit glyph advances.
import { writeFileSync } from 'node:fs';

/** A tiny PDF writer: objects in, a valid file out. */
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

// ── baseline.pdf: twelve plain pages, the oracle for F1 through F5 ─────────
{
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
  writeFileSync(new URL('baseline.pdf', import.meta.url), build(objects));
}

// ── features.pdf: links, annotations and form fields, at known positions ───
//
// A second document rather than more pages on the first, because F1 through
// F5 read `baseline.pdf`'s pixels, its page count and its match counts as
// their oracle, and every one of those would move.
//
// Three pages, 600x400 like the baseline so the test harness's content-space
// arithmetic carries over unchanged:
//
//   page 1  the link page: one /GoTo to page 3, one /URI
//   page 2  the annotation page: a square and a highlight, both at fixed
//           rects, both with /NM so they have durable identities
//   page 3  the form page: a text field, a checkbox and a listbox, each with
//           a value the reader can state without running anything
{
  const objects = [];
  const add = (s) => (objects.push(s), objects.length);
  const catalog = add(''); // 1, patched below
  const pageTree = add(''); // 2, patched below
  const font = add('<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>');
  const zapf = add('<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>');

  const text = (body) => {
    const stream = `BT /F1 18 Tf ${body} ET\n`;
    return add(`<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}endstream`);
  };
  const resources = `<< /Font << /F1 ${font} 0 R /ZaDb ${zapf} 0 R >> >>`;
  const pageIds = [add(''), add(''), add('')];

  // Page 1's two links. Rects are PDF space, y-up, and the viewer converts.
  const internalLink = add(
    `<< /Type /Annot /Subtype /Link /NM (link-internal) /Rect [40 300 320 330] ` +
      `/Border [0 0 0] /A << /S /GoTo /D [${pageIds[2]} 0 R /XYZ 0 400 0] >> >>`,
  );
  const externalLink = add(
    `<< /Type /Annot /Subtype /Link /NM (link-external) /Rect [40 240 360 270] ` +
      `/Border [0 0 0] /A << /S /URI /URI (https://example.invalid/target) >> >>`,
  );

  // Page 2's two annotations.
  const square = add(
    `<< /Type /Annot /Subtype /Square /NM (annot-square) /Rect [60 240 260 340] ` +
      `/C [1 0 0] /IC [1 1 0] /CA 1 /F 4 /T (E2E) /Contents (square annotation) >>`,
  );
  const highlight = add(
    `<< /Type /Annot /Subtype /Highlight /NM (annot-highlight) /Rect [40 140 400 180] ` +
      `/QuadPoints [40 180 400 180 40 140 400 140] /C [1 1 0] /CA 1 /F 4 ` +
      `/T (E2E) /Contents (highlight annotation) >>`,
  );

  // Page 3's three fields, each a widget that is also its field dictionary,
  // which is the single-widget form a viewer meets most often.
  const textField = add(
    `<< /Type /Annot /Subtype /Widget /NM (field-text) /FT /Tx /T (fullName) ` +
      `/V (Ada Lovelace) /DV (Ada Lovelace) /Rect [40 300 360 330] /F 4 ` +
      `/DA (/F1 12 Tf 0 g) /MK << /BC [0 0 0] >> >>`,
  );
  const checkbox = add(
    `<< /Type /Annot /Subtype /Widget /NM (field-checkbox) /FT /Btn /T (subscribe) ` +
      `/V /Yes /DV /Yes /AS /Yes /Rect [40 240 60 260] /F 4 ` +
      `/DA (/ZaDb 0 Tf 0 g) /MK << /BC [0 0 0] /CA (4) >> ` +
      `/AP << /N << /Yes ${add('<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream')} 0 R ` +
      `/Off ${add('<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream')} 0 R >> >> >>`,
  );
  const listbox = add(
    `<< /Type /Annot /Subtype /Widget /NM (field-listbox) /FT /Ch /Ff 0 /T (language) ` +
      `/Opt [[(en) (English)] [(fr) (French)] [(ja) (Japanese)]] /V (fr) /DV (fr) ` +
      `/Rect [40 140 260 200] /F 4 /DA (/F1 12 Tf 0 g) /MK << /BC [0 0 0] >> >>`,
  );

  const contents = [
    text('40 360 Td (Links page) Tj 0 -50 Td (Go to page three) Tj 0 -60 Td (Open example) Tj'),
    text('40 360 Td (Annotations page) Tj'),
    text('40 360 Td (Form page) Tj'),
  ];
  const annots = [
    `[${internalLink} 0 R ${externalLink} 0 R]`,
    `[${square} 0 R ${highlight} 0 R]`,
    `[${textField} 0 R ${checkbox} 0 R ${listbox} 0 R]`,
  ];
  pageIds.forEach((id, i) => {
    objects[id - 1] =
      `<< /Type /Page /Parent ${pageTree} 0 R /MediaBox [0 0 600 400] ` +
      `/Resources ${resources} /Contents ${contents[i]} 0 R /Annots ${annots[i]} >>`;
  });

  objects[pageTree - 1] =
    `<< /Type /Pages /Count ${pageIds.length} /Kids [${pageIds.map((p) => `${p} 0 R`).join(' ')}] >>`;
  objects[catalog - 1] =
    `<< /Type /Catalog /Pages ${pageTree} 0 R ` +
    `/AcroForm << /Fields [${textField} 0 R ${checkbox} 0 R ${listbox} 0 R] ` +
    `/DA (/F1 12 Tf 0 g) /DR ${resources} /NeedAppearances true >> >>`;

  writeFileSync(new URL('features.pdf', import.meta.url), build(objects, catalog));
}
