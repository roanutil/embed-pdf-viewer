#!/usr/bin/env node
// Deterministic generator for the cross-language conformance fixtures. No
// dependencies, no dates, no randomness, so the committed PDFs are
// reproducible and reviewable.
//
// Regenerate with:
//   node tooling/engine-conformance/fixtures/generate.mjs
//
// Why these exist when the repository already has orientation fixtures:
// `rotated_text.pdf`, `sheared_text.pdf`, `mirrored_text.pdf` and
// `vertical_text.pdf` all draw with the standard-14 names Helvetica and
// Times-Roman and embed no font program. PDFium then substitutes, and what it
// substitutes depends on the host: the WASM build has no system fonts and
// falls back to its built-in mapper, while a native build on macOS reaches
// CoreText. The two disagree by tenths of a point in every glyph box, which
// makes those files unusable as a cross-language oracle. Measured on
// hello_world.pdf: looseBox.bottom 47.012 through WASM against 46.328 through
// the macOS dylib.
//
// Every fixture here embeds its font program in full, so the glyph metrics
// come out of the file rather than out of the host.
//
// Emits:
//   embedded_orientations.pdf — one page, six text objects covering the run
//                               variants the geometry reader discriminates:
//                               upright, rotated, shear-only, mirrored
//                               (negative determinant), quarter-turn, and a
//                               zero-size object whose glyphs are degenerate.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { FILE_ID, standardSecurity } from './encrypt.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');

/** The one font program these fixtures embed. Already in the repository. */
const FONT_PATH = resolve(repoRoot, 'packages/engine/main/test/fixtures/Roboto-Regular.ttf');

// --- TrueType reading -------------------------------------------------------
// Enough of the format to build a correct simple-font dictionary: the metrics
// PDF needs in /Widths and /FontDescriptor. The font program itself is
// embedded whole, so nothing here has to be able to write TrueType.

function readTables(font) {
  const count = font.readUInt16BE(4);
  const tables = new Map();
  for (let i = 0; i < count; i++) {
    const record = 12 + 16 * i;
    tables.set(font.toString('latin1', record, record + 4), {
      offset: font.readUInt32BE(record + 8),
      length: font.readUInt32BE(record + 12),
    });
  }
  return tables;
}

function table(font, tables, tag) {
  const found = tables.get(tag);
  if (!found) throw new Error(`${FONT_PATH} has no ${tag} table`);
  return font.subarray(found.offset, found.offset + found.length);
}

/** Unicode code point to glyph index, from a format 4 cmap subtable. */
function readCmap(cmap) {
  const subtableCount = cmap.readUInt16BE(2);
  let best = -1;
  for (let i = 0; i < subtableCount; i++) {
    const record = 4 + 8 * i;
    const platform = cmap.readUInt16BE(record);
    const encoding = cmap.readUInt16BE(record + 2);
    const offset = cmap.readUInt32BE(record + 4);
    // Windows Unicode BMP, or Unicode platform. Either is a Unicode map.
    const unicode = (platform === 3 && encoding === 1) || platform === 0;
    if (unicode && cmap.readUInt16BE(offset) === 4) best = offset;
  }
  if (best < 0) throw new Error('no format 4 Unicode cmap subtable');

  const sub = cmap.subarray(best);
  const segCount = sub.readUInt16BE(6) / 2;
  const endAt = 14;
  const startAt = endAt + segCount * 2 + 2;
  const deltaAt = startAt + segCount * 2;
  const rangeAt = deltaAt + segCount * 2;

  return (codePoint) => {
    for (let segment = 0; segment < segCount; segment++) {
      const end = sub.readUInt16BE(endAt + segment * 2);
      if (codePoint > end) continue;
      const start = sub.readUInt16BE(startAt + segment * 2);
      if (codePoint < start) return 0;
      const delta = sub.readInt16BE(deltaAt + segment * 2);
      const rangeOffset = sub.readUInt16BE(rangeAt + segment * 2);
      if (rangeOffset === 0) return (codePoint + delta) & 0xffff;
      const glyphAt = rangeAt + segment * 2 + rangeOffset + (codePoint - start) * 2;
      const glyph = sub.readUInt16BE(glyphAt);
      return glyph === 0 ? 0 : (glyph + delta) & 0xffff;
    }
    return 0;
  };
}

/** Advance width in font units, from hmtx. */
function readAdvances(hmtx, longMetricCount) {
  return (glyph) => {
    const index = Math.min(glyph, longMetricCount - 1);
    return hmtx.readUInt16BE(index * 4);
  };
}

function readFont() {
  const font = readFileSync(FONT_PATH);
  const tables = readTables(font);
  const head = table(font, tables, 'head');
  const hhea = table(font, tables, 'hhea');
  const os2 = table(font, tables, 'OS/2');
  const unitsPerEm = head.readUInt16BE(18);
  const toGlyph = readCmap(table(font, tables, 'cmap'));
  const advance = readAdvances(table(font, tables, 'hmtx'), hhea.readUInt16BE(34));
  // PDF glyph space is 1000 units per em regardless of what the font uses.
  const scale = (value) => Math.round((value * 1000) / unitsPerEm);

  return {
    program: font,
    scale,
    toGlyph,
    widthOf: (codePoint) => scale(advance(toGlyph(codePoint))),
    bbox: [
      scale(head.readInt16BE(36)),
      scale(head.readInt16BE(38)),
      scale(head.readInt16BE(40)),
      scale(head.readInt16BE(42)),
    ],
    ascent: scale(hhea.readInt16BE(4)),
    descent: scale(hhea.readInt16BE(6)),
    // sCapHeight, present from OS/2 version 2. Roboto ships version 3+.
    capHeight: os2.readUInt16BE(0) >= 2 ? scale(os2.readInt16BE(88)) : 700,
  };
}

// --- PDF writing ------------------------------------------------------------

const FIRST_CHAR = 32;
const LAST_CHAR = 126;

function escapeLiteral(text) {
  return text.replace(/[\\()]/g, (character) => `\\${character}`);
}

/**
 * Assembles numbered objects into a classic-xref PDF.
 *
 * `objects` is an array of Buffers, each the complete body of object n+1,
 * without the `n 0 obj` / `endobj` wrapper.
 *
 * `trailerExtras` is appended to the trailer dictionary, which is how the
 * encrypted fixtures attach `/Encrypt` and `/ID`. An encrypted document needs
 * both: the file ID feeds the key derivation for revisions 2 through 4, so a
 * document without one cannot be decrypted at all.
 */
function buildPdf(objects, trailerExtras = '') {
  const chunks = [Buffer.from('%PDF-1.7\n%\xe2\xe3\xcf\xd3\n', 'latin1')];
  let offset = chunks[0].length;
  const offsets = [0];

  objects.forEach((body, index) => {
    offsets.push(offset);
    const chunk = Buffer.concat([
      Buffer.from(`${index + 1} 0 obj\n`, 'latin1'),
      body,
      Buffer.from('\nendobj\n', 'latin1'),
    ]);
    chunks.push(chunk);
    offset += chunk.length;
  });

  const startxref = offset;
  let xref = `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (let i = 1; i <= objects.length; i++) {
    xref += `${String(offsets[i]).padStart(10, '0')} 00000 n \n`;
  }
  xref += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R${trailerExtras} >>\nstartxref\n${startxref}\n%%EOF\n`;
  chunks.push(Buffer.from(xref, 'latin1'));
  return Buffer.concat(chunks);
}

function stream(dictionary, payload) {
  return Buffer.concat([
    Buffer.from(`<< ${dictionary} /Length ${payload.length} >>\nstream\n`, 'latin1'),
    payload,
    Buffer.from('\nendstream', 'latin1'),
  ]);
}

/**
 * One text object: a text matrix, a size and a string.
 *
 * `matrix` is the six-number Tm operand. The geometry reader classifies a run
 * off the char matrix PDFium derives from it, so this is the whole control
 * surface for which run variant the fixture produces.
 */
function textObject({ matrix, size, text }) {
  return `BT\n/F1 ${size} Tf\n${matrix.join(' ')} Tm\n(${escapeLiteral(text)}) Tj\nET\n`;
}

function embeddedOrientations(font) {
  const cos30 = 0.8660254;
  const sin30 = 0.5;
  const content = [
    // Upright. The dominant case, and the one whose wire shape predates
    // orientation support.
    textObject({ matrix: [1, 0, 0, 1, 60, 700], size: 24, text: 'Upright ABC xyz' }),
    // Rotated 30 degrees counter-clockwise in PDF y-up space.
    textObject({
      matrix: [cos30, sin30, -sin30, cos30, 60, 600],
      size: 24,
      text: 'Rotated ABC',
    }),
    // Shear only: rotation is exactly 0 and the cells are still
    // parallelograms, which is the case an AABB would misrepresent.
    textObject({ matrix: [1, 0, 0.35, 1, 60, 500], size: 24, text: 'Sheared ABC' }),
    // Negative determinant: the ascentFlip flag.
    textObject({ matrix: [-1, 0, 0, 1, 400, 400], size: 24, text: 'Mirrored ABC' }),
    // Quarter turn.
    textObject({ matrix: [0, 1, -1, 0, 100, 150], size: 24, text: 'Vertical ABC' }),
    // Zero size: every glyph comes back degenerate, zeroed and flagged empty.
    textObject({ matrix: [1, 0, 0, 1, 60, 100], size: 0, text: 'Degenerate' }),
  ].join('');

  const widths = [];
  for (let code = FIRST_CHAR; code <= LAST_CHAR; code++) widths.push(font.widthOf(code));

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from('<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>', 'latin1'),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
      'latin1',
    ),
    stream('', Buffer.from(content, 'latin1')),
    Buffer.from(
      `<< /Type /Font /Subtype /TrueType /BaseFont /Roboto-Regular ` +
        `/FirstChar ${FIRST_CHAR} /LastChar ${LAST_CHAR} /Widths [${widths.join(' ')}] ` +
        `/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>`,
      'latin1',
    ),
    Buffer.from(
      `<< /Type /FontDescriptor /FontName /Roboto-Regular /Flags 32 ` +
        `/FontBBox [${font.bbox.join(' ')}] /ItalicAngle 0 /Ascent ${font.ascent} ` +
        `/Descent ${font.descent} /CapHeight ${font.capHeight} /StemV 80 ` +
        `/FontFile2 7 0 R >>`,
      'latin1',
    ),
    stream(`/Length1 ${font.program.length}`, font.program),
  ]);
}

/**
 * A minimal encrypted document: catalog, pages, one page, one content stream.
 *
 * Deliberately has no text and no strings outside the trailer. String
 * encryption is a separate code path from stream encryption, and a fixture
 * that exercised both would be answering two questions at once; what G3's
 * gate needs from these is that PDFium opens them and reports one security
 * state. The content stream is still encrypted, so the document is a real one
 * rather than an encryption dictionary bolted to plaintext.
 */
function encryptedDocument({ revision, keyLength, userPassword, ownerPassword, permissions, aes }) {
  const security = standardSecurity({
    revision,
    keyLength,
    userPassword,
    ownerPassword,
    permissions,
    aes,
  });

  // A filled rectangle, so the page has content PDFium has to decrypt to parse.
  const content = Buffer.from('0.2 0.4 0.9 rg\n72 72 200 120 re\nf\n', 'latin1');
  const contentObjectNumber = 4;
  const encrypted = security.encrypt(content, contentObjectNumber);

  const objects = [
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from('<< /Type /Pages /Kids [3 0 R] /Count 1 >>', 'latin1'),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << >> >>',
      'latin1',
    ),
    stream('', encrypted),
    // The encryption dictionary itself is never encrypted, which is why it is
    // its own object rather than inline in the trailer.
    Buffer.from(security.dictionary, 'latin1'),
  ];

  const id = `<${FILE_ID.toString('hex')}>`;
  return buildPdf(objects, ` /Encrypt 5 0 R /ID [${id} ${id}]`);
}

/**
 * A document whose security handler PDFium does not implement, which is the
 * only way to reach the `unsupported` probe shape.
 *
 * `/Filter /Frobnicate` is not a real handler and never will be. PDFium
 * reports FPDF_ERR_SECURITY rather than a password prompt, which is the
 * distinction this fixture exists to pin: unsupported is not the same answer
 * as "needs a password", and a reader that collapsed them would send a user
 * to a prompt that cannot help.
 */
function unsupportedSecurityDocument() {
  const objects = [
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from('<< /Type /Pages /Kids [3 0 R] /Count 1 >>', 'latin1'),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << >> >>',
      'latin1',
    ),
    stream('', Buffer.from('72 72 200 120 re\nf\n', 'latin1')),
    Buffer.from('<< /Filter /Frobnicate /V 9 /R 9 /Length 128 /P -4 >>', 'latin1'),
  ];
  const id = `<${FILE_ID.toString('hex')}>`;
  return buildPdf(objects, ` /Encrypt 5 0 R /ID [${id} ${id}]`);
}

/**
 * Four identical pages at `/Rotate` 0, 90, 180 and 270.
 *
 * Nothing else in this repository sets `/Rotate` at all, which left rotation
 * normalization entirely untested. `multiplatform-plan.md` is explicit that
 * loading a page with `FPDF_LoadPage` "would bypass normalized rotation", and
 * `PagePtrPool.acquire` uses `EPDFDoc_LoadPageByObjectNumberNormalized` so that
 * render, text, geometry and annotation coordinates all share one space. A
 * corpus with no rotated page cannot tell the two loaders apart.
 *
 * The pages carry identical content, so under the normalized loader all four
 * must report the same size and the same glyph positions. Under the raw
 * loader, 90 and 270 swap width for height and move the text. That difference
 * is the whole point of the fixture.
 */
function rotatedPages(font) {
  const content = Buffer.from(
    textObject({ matrix: [1, 0, 0, 1, 72, 700], size: 24, text: 'Rotate ABC' }),
    'latin1',
  );

  const widths = [];
  for (let code = FIRST_CHAR; code <= LAST_CHAR; code++) widths.push(font.widthOf(code));

  // Objects 1-2 are the catalog and page tree, 3-6 the pages, 7 the shared
  // content stream, 8-10 the font.
  const pages = [0, 90, 180, 270].map((rotate) =>
    Buffer.from(
      `<< /Type /Page /Parent 2 0 R /Contents 7 0 R /Rotate ${rotate} ` +
        `/Resources << /Font << /F1 8 0 R >> >> >>`,
      'latin1',
    ),
  );

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    ...pages,
    stream('', content),
    Buffer.from(
      `<< /Type /Font /Subtype /TrueType /BaseFont /Roboto-Regular ` +
        `/FirstChar ${FIRST_CHAR} /LastChar ${LAST_CHAR} /Widths [${widths.join(' ')}] ` +
        `/Encoding /WinAnsiEncoding /FontDescriptor 9 0 R >>`,
      'latin1',
    ),
    Buffer.from(
      `<< /Type /FontDescriptor /FontName /Roboto-Regular /Flags 32 ` +
        `/FontBBox [${font.bbox.join(' ')}] /ItalicAngle 0 /Ascent ${font.ascent} ` +
        `/Descent ${font.descent} /CapHeight ${font.capHeight} /StemV 80 ` +
        `/FontFile2 10 0 R >>`,
      'latin1',
    ),
    stream(`/Length1 ${font.program.length}`, font.program),
  ]);
}

/**
 * A page whose extracted text is not the same size as its character list.
 *
 * Every other fixture here has an identity char map, so the whole
 * character-to-text translation was pinned only against hand-built snapshots.
 * `multiplatform-port-order.md` G5 asks for "fixtures with non-empty
 * `charMap` (non-BMP, non-printing)", and this is that.
 *
 * The trick is that PDFium extracts through `/ToUnicode`, not through the
 * glyphs. So the font is a Type0 with Identity-H encoding over an embedded
 * Roboto, and the CMap maps one CID to a supplementary code point and another
 * to nothing at all. The glyphs drawn are ordinary letters; what the text
 * layer says they mean is what makes the map non-identity.
 */
function nonIdentityText(font) {
  // Real glyph ids for the letters, plus glyph 0. In Identity-H the CID is
  // the glyph id, so CID 0 is `.notdef`: it has no entry in the font's own
  // cmap and none in the CMap below, so PDFium reports no Unicode for it and
  // the character contributes zero text units. That is the dropped case, and
  // it is the only way to produce one without a broken font.
  const letters = ['A', 'C', 'D'].map((c) => font.toGlyph(c.codePointAt(0)));
  const glyphs = [letters[0], 0, letters[1], letters[2]];
  const hex = glyphs.map((g) => g.toString(16).padStart(4, '0')).join('');

  // CID 0 -> U+1F600, two UTF-16 units from one character.
  // CID 1 -> nothing, a character contributing zero units.
  // CIDs 2 and 3 -> ordinary letters, one unit each.
  const toUnicode = [
    '/CIDInit /ProcSet findresource begin',
    '12 dict begin begincmap',
    '/CMapName /EmbedPDF-NonIdentity def',
    '/CMapType 2 def',
    '/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def',
    '1 begincodespacerange',
    '<0000> <ffff>',
    'endcodespacerange',
    '3 beginbfchar',
    // A supplementary code point, written as the surrogate pair PDF requires.
    `<${glyphs[0].toString(16).padStart(4, '0')}> <d83dde00>`,
    `<${glyphs[2].toString(16).padStart(4, '0')}> <0043>`,
    `<${glyphs[3].toString(16).padStart(4, '0')}> <0044>`,
    'endbfchar',
    // CID 0 is deliberately absent, which is what makes it contribute nothing.
    'endcmap CMapName currentdict /CMap defineresource pop end end',
  ].join('\n');

  const content = Buffer.from(
    `BT\n/F1 24 Tf\n1 0 0 1 72 700 Tm\n<${hex}> Tj\nET\n`,
    'latin1',
  );

  const widths = glyphs.map((g) => `${g} [${font.widthOf(65)}]`).join(' ');

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
      'latin1',
    ),
    stream('', content),
    Buffer.from(
      '<< /Type /Font /Subtype /Type0 /BaseFont /Roboto-Regular /Encoding /Identity-H ' +
        '/DescendantFonts [6 0 R] /ToUnicode 9 0 R >>',
      'latin1',
    ),
    Buffer.from(
      `<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Roboto-Regular ` +
        `/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> ` +
        `/FontDescriptor 7 0 R /DW 1000 /W [${widths}] /CIDToGIDMap /Identity >>`,
      'latin1',
    ),
    Buffer.from(
      `<< /Type /FontDescriptor /FontName /Roboto-Regular /Flags 4 ` +
        `/FontBBox [${font.bbox.join(' ')}] /ItalicAngle 0 /Ascent ${font.ascent} ` +
        `/Descent ${font.descent} /CapHeight ${font.capHeight} /StemV 80 ` +
        `/FontFile2 8 0 R >>`,
      'latin1',
    ),
    stream(`/Length1 ${font.program.length}`, font.program),
    stream('', Buffer.from(toUnicode, 'latin1')),
  ]);
}

/**
 * Two layout cases no other fixture produces: a run that reads backwards, and
 * a run with a gap wide enough to split it.
 *
 * `layout.ts` derives a segment's `advance` from the glyph SEQUENCE rather
 * than from geometry, and it splits a sub-run when the gap between two glyphs
 * exceeds 2.5 average advances. Mutation testing found both untested: every
 * segment in the corpus advanced forward, and no probed range contained a gap
 * that wide, so a port could reverse the advance rule or double the factor and
 * stay green.
 *
 * Both are made with TJ adjustments inside ONE text object, so they stay one
 * run. A positive TJ number moves BACKWARD along the baseline, which is what
 * lets the second line place its glyphs right to left without a second text
 * object and without a font that has right-to-left glyphs.
 */
function reverseAndGappedText(font) {
  const widths = [];
  for (let code = FIRST_CHAR; code <= LAST_CHAR; code++) widths.push(font.widthOf(code));

  // Backwards: after each glyph the pen moves back far enough that the next
  // lands to its left, but NOT far enough to exceed 2.5 average advances,
  // which would split the run into one sub-run per glyph and leave every
  // advance forward again. A positive TJ number moves backward by
  // value/1000 times the font size, so 1200 at 24pt is 28.8 points against an
  // average advance of about 15.6.
  const backwards = '(A) 1200 (B) 1200 (C) 1200 (D)';
  // Gapped: a forward jump between 2.5 and 5 average advances, so the split
  // happens at the real factor and stops happening at a doubled one. 2500 at
  // 24pt is 60 points, against a 39-point threshold and a 78-point doubled
  // one.
  const gapped = '(E) -2500 (F)';

  const content =
    `BT\n/F1 24 Tf\n1 0 0 1 400 700 Tm\n[${backwards}] TJ\nET\n` +
    `BT\n/F1 24 Tf\n1 0 0 1 72 600 Tm\n[${gapped}] TJ\nET\n`;

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
      'latin1',
    ),
    stream('', Buffer.from(content, 'latin1')),
    Buffer.from(
      `<< /Type /Font /Subtype /TrueType /BaseFont /Roboto-Regular ` +
        `/FirstChar ${FIRST_CHAR} /LastChar ${LAST_CHAR} /Widths [${widths.join(' ')}] ` +
        `/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>`,
      'latin1',
    ),
    Buffer.from(
      `<< /Type /FontDescriptor /FontName /Roboto-Regular /Flags 32 ` +
        `/FontBBox [${font.bbox.join(' ')}] /ItalicAngle 0 /Ascent ${font.ascent} ` +
        `/Descent ${font.descent} /CapHeight ${font.capHeight} /StemV 80 ` +
        `/FontFile2 7 0 R >>`,
      'latin1',
    ),
    stream(`/Length1 ${font.program.length}`, font.program),
  ]);
}

/**
 * A page carrying open and close actions, with a `/Next` chain.
 *
 * `pages.list` reports `PageLayout.actions` and nothing in the repository
 * produced one, so the field was absent on both sides and a comparison could
 * not tell an unread action from an absent one.
 *
 * The open action is a JavaScript node whose `/Next` is a URI node, so the
 * chain is two deep. The close action is a GoTo pointing at the second page
 * with an explicit `/XYZ` destination, which is the destination shape a
 * viewer actually has to honour.
 */
/**
 * A catalog carrying every action slot `actions.read` reports.
 *
 * A JavaScript name tree with two entries in a deliberate order, an
 * `/OpenAction` in its action form, and all five additional events. Nothing
 * else in the repository has any of them, so the reader had no document to be
 * right or wrong about.
 */
function documentActions() {
  const script = (body) => `<< /Type /Action /S /JavaScript /JS (${body}) >>`;

  return buildPdf([
    // The catalog: /Names for the script tree, /OpenAction, and /AA.
    Buffer.from(
      '<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript 4 0 R >> ' +
        '/OpenAction 7 0 R /AA << /WC 8 0 R /WS 9 0 R /DS 10 0 R /WP 11 0 R /DP 12 0 R >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 13 0 R /Resources << >> >>',
      'latin1',
    ),
    // The name tree. Keys are sorted, and the order is the boot order, which
    // decides which script wins when two define the same function.
    Buffer.from('<< /Names [(aaa-first) 5 0 R (zzz-second) 6 0 R] >>', 'latin1'),
    Buffer.from(script('var first = 1;'), 'latin1'),
    Buffer.from(script('var second = 2;'), 'latin1'),
    Buffer.from(script('var opened = true;'), 'latin1'),
    Buffer.from(script('var willClose = true;'), 'latin1'),
    Buffer.from(script('var willSave = true;'), 'latin1'),
    Buffer.from(script('var didSave = true;'), 'latin1'),
    Buffer.from(script('var willPrint = true;'), 'latin1'),
    Buffer.from(script('var didPrint = true;'), 'latin1'),
    stream('', Buffer.from('72 72 100 100 re\nf\n', 'latin1')),
  ]);
}

function pageActions() {
  const content = Buffer.from('72 72 200 100 re\nf\n', 'latin1');

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    // The first page carries both events.
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << >> ' +
        '/AA << /O 6 0 R /C 8 0 R >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << >> >>',
      'latin1',
    ),
    stream('', content),
    // Open: JavaScript, then a URI through /Next.
    Buffer.from(
      '<< /Type /Action /S /JavaScript /JS (app.alert\\(\'opened\'\\);) /Next 7 0 R >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Action /S /URI /URI (https://example.test/opened) >>',
      'latin1',
    ),
    // Close: GoTo page two, /XYZ with a left, a top and no zoom.
    Buffer.from('<< /Type /Action /S /GoTo /D [4 0 R /XYZ 100 200 null] >>', 'latin1'),
  ]);
}

/**
 * A page whose annotations cover every distinction the BASE shell makes,
 * independently of any subtype.
 *
 * Nothing in the repository had these. The fixtures that carry annotations at
 * all carry one shape of them, which means a reader could get identity,
 * relationships or the absent-versus-empty rule wrong and still agree with the
 * TypeScript on every document here.
 *
 * Laid out so each distinction has a case that differs in ONE way from
 * another:
 *
 *   - An indirect annotation, so `EPDFAnnot_GetObjectNumber` answers and the
 *     ref is by object number. This is the ordinary case.
 *   - A DIRECT dictionary inside `/Annots`, which is legal PDF and has no
 *     object number at all. With `/NM` it is still durable; without one the
 *     ref falls back to index plus revision and the whole page is marked weak.
 *     Both are here, adjacent, because the fallback order between them is the
 *     thing that is easy to write backwards. A third carries `/NM ()`, which
 *     is not an address either: an empty name is one two annotations could
 *     share.
 *   - `/Contents ()` against no `/Contents` at all, and the same for `/T`.
 *     Present-and-empty is what a patch operates on and a reader that
 *     collapsed it into absent would look right on every other fixture.
 *   - `/IRT` with `/RT /R`, `/IRT` with `/RT /Group`, and `/IRT` with no
 *     `/RT`. PDFium answers the `/R` default for the third and for an
 *     annotation with no `/IRT` at all, so the last one is the case that
 *     catches a reader consulting `/RT` without gating on `/IRT`.
 *   - `/EMBD_Metadata`, which only EmbedPDF writes, with some fields set and
 *     others absent.
 *   - A `/Rect` in reversed corner order, which the file is allowed to do.
 *   - A `/Sound` annotation, a subtype PDFium knows and this engine does not
 *     read, so `unsupported` is exercised by a real subtype rather than by a
 *     code nobody has issued.
 */
function annotationBase() {
  const content = Buffer.from('72 72 200 100 re\nf\n', 'latin1');
  // Two dates in the two forms PDF permits, so the ISO conversion is pinned
  // for both rather than for whichever one a fixture happened to use.
  const created = "/CreationDate (D:20240115093000+01'00')";
  const modified = '/M (D:20240220120000Z)';

  // The two direct dictionaries. They live inside /Annots rather than as
  // numbered objects, which is what makes them identity cases.
  const directWithNm =
    '<< /Type /Annot /Subtype /Square /Rect [300 600 380 660] /F 4 /NM (direct-but-named) ' +
    '/Contents (a direct object that named itself) >>';
  const directAnonymous =
    '<< /Type /Annot /Subtype /Square /Rect [300 500 380 560] /F 4 ' +
    '/Contents (a direct object with no name) >>';
  // A direct object with an /NM that is present and EMPTY. An empty name is
  // not an address: two annotations could both carry it, so this one has to
  // fall through to the index form exactly as the anonymous one does. Without
  // this case a reader that took any /NM at all would agree with every other
  // annotation here.
  const directEmptyName =
    '<< /Type /Annot /Subtype /Square /Rect [300 400 380 460] /F 4 /NM () ' +
    '/Contents (a direct object whose name is empty) >>';

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> /Annots [' +
        `5 0 R 6 0 R ${directWithNm} ${directAnonymous} ${directEmptyName} ` +
        '7 0 R 8 0 R 9 0 R 10 0 R' +
        '] >>',
      'latin1',
    ),
    stream('', content),

    // 5: the ordinary case. Every optional string present, one of them empty.
    Buffer.from(
      '<< /Type /Annot /Subtype /Text /Rect [72 700 92 720] /F 4 /NM (the-anchor) ' +
        `/Contents (an ordinary sticky note) /T (Ada Lovelace) /Subj () ${created} ${modified} ` +
        '/C [1 1 0] /CA 0.5 >>',
      'latin1',
    ),
    // 6: the same subtype with every optional string ABSENT, and a /Rect whose
    // corners are the other way round.
    Buffer.from(
      '<< /Type /Annot /Subtype /Text /Rect [200 740 180 720] /F 0 >>',
      'latin1',
    ),
    // 7: a reply. /RT /R is the explicit spelling of the default.
    Buffer.from(
      '<< /Type /Annot /Subtype /Text /Rect [92 700 112 720] /F 4 /NM (a-reply) ' +
        '/IRT 5 0 R /RT /R /Contents (replying to the anchor) >>',
      'latin1',
    ),
    // 8: a group member, which is the other /RT value.
    Buffer.from(
      '<< /Type /Annot /Subtype /Text /Rect [112 700 132 720] /F 4 /NM (a-group-member) ' +
        '/IRT 5 0 R /RT /Group /Contents (part of the anchor) >>',
      'latin1',
    ),
    // 9: /IRT with no /RT, which ISO says is a reply, plus the EmbedPDF
    // metadata dictionary with two of its four fields set.
    Buffer.from(
      '<< /Type /Annot /Subtype /Text /Rect [132 700 152 720] /F 4 /NM (stamped-by-embedpdf) ' +
        '/IRT 5 0 R /Contents (no explicit reply type) ' +
        '/EMBD_Metadata << /SchemaVersion 1 /UserID (user-42) /CreatedBy (Ada Lovelace) >> >>',
      'latin1',
    ),
    // 10: a subtype this engine has no reader for.
    Buffer.from(
      '<< /Type /Annot /Subtype /Sound /Rect [400 700 440 740] /F 4 /NM (a-sound) >>',
      'latin1',
    ),
  ]);
}

/**
 * One annotation per geometry subtype, each carrying the entries its reader
 * has to decide something about.
 *
 * Separate from `annotation_base.pdf` because the two fixtures answer
 * different questions. That one varies the shell and holds the subtype
 * roughly constant; this one holds the shell constant and varies the subtype,
 * so a failure names which reader rather than which shared field.
 *
 * The pairs are the point. A square with an explicit `/BS`, `/BE` and `/RD`
 * next to a circle with none of them, so the resolved defaults are exercised
 * rather than assumed. A strikeout with `/IT` next to one without. Ink with
 * the highlighter intent next to ordinary pen ink. A highlight with no `/C`,
 * which defaults to yellow, next to an underline with one, because the
 * text-markup family's default differs between highlight and the other three
 * and a reader that used one default for all four would agree with three. And
 * a third square whose `/BE` is present with intensity zero, which draws no
 * clouds and has to normalize to null. A stamp whose label the fork's enum
 * knows beside one whose label falls outside it. A text redaction with quads
 * and a label beside an area redaction with neither. A free text with a full
 * `/DA` beside one with none.
 */
function annotationKinds() {
  const content = Buffer.from('72 72 200 100 re\nf\n', 'latin1');

  // Two quads, because a markup over two lines of text is the ordinary case
  // and a reader that returned only the first would look right on one line.
  const twoQuads = '[72 720 200 720 72 700 200 700 72 690 160 690 72 670 160 670]';
  const oneQuad = '[72 640 200 640 72 620 200 620]';

  const annots = [
    // Highlight with no /C: the family default that is not black.
    '<< /Type /Annot /Subtype /Highlight /Rect [72 670 200 720] /F 4 /NM (a-highlight) ' +
      `/QuadPoints ${twoQuads} /CA 0.4 >>`,
    // Underline WITH /C, so the explicit path is covered beside the default.
    '<< /Type /Annot /Subtype /Underline /Rect [72 620 200 640] /F 4 /NM (an-underline) ' +
      `/QuadPoints ${oneQuad} /C [0 0 1] >>`,
    '<< /Type /Annot /Subtype /Squiggly /Rect [72 600 200 620] /F 4 /NM (a-squiggly) ' +
      '/QuadPoints [72 620 200 620 72 600 200 600] >>',
    // Strikeout as half of a replace-text edit.
    '<< /Type /Annot /Subtype /StrikeOut /Rect [72 580 200 600] /F 4 /NM (an-edit-strikeout) ' +
      '/QuadPoints [72 600 200 600 72 580 200 580] /IT /StrikeOutTextEdit >>',
    // And a standalone one, which has no /IT at all.
    '<< /Type /Annot /Subtype /StrikeOut /Rect [72 560 200 580] /F 4 /NM (a-plain-strikeout) ' +
      '/QuadPoints [72 580 200 580 72 560 200 560] >>',
    // A square with every optional border entry set: a dashed /BS with a
    // pattern, a cloudy /BE, an /RD inset and an /IC fill.
    '<< /Type /Annot /Subtype /Square /Rect [300 700 400 760] /F 4 /NM (a-decorated-square) ' +
      '/C [1 0 0] /IC [0 1 0] /CA 0.8 /BS << /W 3 /S /D /D [4 2] >> ' +
      '/BE << /S /C /I 2 >> /RD [1 2 3 4] >>',
    // A circle with none of them, so the resolved defaults are read.
    '<< /Type /Annot /Subtype /Circle /Rect [300 620 400 680] /F 4 /NM (a-plain-circle) >>',
    // A /BE that is present and draws nothing. The wire domain for
    // cloudyIntensity is a positive number or null, so a zero has to
    // normalize to null rather than leak a value no schema admits, and no
    // other annotation here can tell a reader that skips the check.
    '<< /Type /Annot /Subtype /Square /Rect [300 540 400 600] /F 4 ' +
      '/NM (a-degenerate-cloudy-square) /BE << /S /C /I 0 >> >>',
    '<< /Type /Annot /Subtype /Polygon /Rect [300 520 420 600] /F 4 /NM (a-polygon) ' +
      '/Vertices [300 520 420 520 360 600] /C [0 0 1] /IC [1 1 0] /BE << /S /C /I 1 >> >>',
    '<< /Type /Annot /Subtype /PolyLine /Rect [300 420 420 500] /F 4 /NM (a-polyline) ' +
      '/Vertices [300 420 360 500 420 420] /LE [/OpenArrow /Diamond] >>',
    '<< /Type /Annot /Subtype /Line /Rect [450 420 560 520] /F 4 /NM (a-line) ' +
      '/L [450 420 560 520] /LE [/Circle /ClosedArrow] /C [1 0 1] /BS << /W 2 >> >>',
    // Two pen strokes, so a reader that returned only the first is caught.
    '<< /Type /Annot /Subtype /Ink /Rect [450 300 560 400] /F 4 /NM (a-highlighter) ' +
      '/InkList [[450 300 480 340 510 300] [520 320 560 380]] /IT /InkHighlight /CA 0.3 >>',
    '<< /Type /Annot /Subtype /Ink /Rect [450 200 560 280] /F 4 /NM (plain-ink) ' +
      '/InkList [[450 200 560 280]] >>',

    // A sticky note with a named icon and a review state, beside one with
    // neither. /State and /StateModel are read faithfully: absent is null,
    // and the standard Table 174 spellings normalize to lowercase while a
    // custom model passes through.
    '<< /Type /Annot /Subtype /Text /Rect [72 520 92 540] /F 4 /NM (a-help-note) ' +
      '/Name /Help /C [0 1 1] /State /Accepted /StateModel /Review >>',
    '<< /Type /Annot /Subtype /Text /Rect [72 490 92 510] /F 4 /NM (a-custom-state-note) ' +
      '/State (Escalated) /StateModel (legal-hold) >>',
    // A caret that is half of a replace-text edit, with an /RD inset.
    '<< /Type /Annot /Subtype /Caret /Rect [72 450 92 470] /F 4 /NM (an-edit-caret) ' +
      '/IT /Replace /RD [1 1 1 1] /C [0 1 0] >>',
    // A stamp with a label the fork's enum knows, and one with a custom label
    // that falls outside it and has to come from the raw /Name entry.
    '<< /Type /Annot /Subtype /Stamp /Rect [140 440 260 490] /F 4 /NM (an-approved-stamp) ' +
      '/Name /Approved >>',
    '<< /Type /Annot /Subtype /Stamp /Rect [140 380 260 430] /F 4 /NM (a-custom-stamp) ' +
      '/Name /LegalReviewPending >>',
    // A text redaction: quads, an overlay label, a /DA and a /Q.
    '<< /Type /Annot /Subtype /Redact /Rect [72 340 240 370] /F 4 /NM (a-text-redaction) ' +
      '/QuadPoints [72 370 240 370 72 340 240 340] /IC [0 0 0] ' +
      '/OverlayText (REDACTED) /Repeat true /DA (1 0 0 rg /Helv 9 Tf) /Q 1 >>',
    // And an area redaction, which has no quads and no label at all.
    '<< /Type /Annot /Subtype /Redact /Rect [72 290 240 320] /F 4 /NM (an-area-redaction) >>',
    // Free text: /C is the BOX BACKGROUND here and the border and text come
    // from /DA, which is the one place in the format where /C means something
    // other than a stroke.
    '<< /Type /Annot /Subtype /FreeText /Rect [300 240 460 300] /F 4 /NM (a-text-box) ' +
      '/DA (0 0 1 rg /Helv 14 Tf) /C [1 1 0.8] /Q 2 /BS << /W 2 /S /S >> ' +
      '/RD [2 2 2 2] >>',
    // A callout, which is the same kind with a /CL leader and an ending on it.
    '<< /Type /Annot /Subtype /FreeText /Rect [300 150 460 210] /F 4 /NM (a-callout) ' +
      '/IT /FreeTextCallout /DA (0 0 0 rg /TiRo 11 Tf) /CL [200 120 260 170 300 180] ' +
      '/LE /OpenArrow /Q 0 >>',
    // A /DA size of zero. Free text has no auto-fit, so it normalizes to the
    // default; a redaction keeps the same zero, because there it means
    // auto-fit the label to the region. The two annotations are adjacent
    // because a reader that used one rule for both agrees with one of them.
    '<< /Type /Annot /Subtype /FreeText /Rect [300 20 460 70] /F 4 /NM (an-autosize-text-box) ' +
      '/DA (0 0 0 rg /Helv 0 Tf) >>',
    '<< /Type /Annot /Subtype /Redact /Rect [72 240 240 270] /F 4 /NM (an-autosize-redaction) ' +
      '/OverlayText (FIT) /DA (0 0 0 rg /Helv 0 Tf) >>',
    // /TextColor overrides the text alone, so a value EQUAL to the /DA colour
    // overrides nothing and must not reach the wire; a different one must.
    '<< /Type /Annot /Subtype /FreeText /Rect [480 240 560 290] /F 4 ' +
      '/NM (a-text-box-with-its-own-text-colour) /DA (0 0 1 rg /Helv 10 Tf) ' +
      '/TextColor [1 0 0] >>',
    '<< /Type /Annot /Subtype /FreeText /Rect [480 180 560 230] /F 4 ' +
      '/NM (a-text-box-whose-text-colour-changes-nothing) /DA (0 0 1 rg /Helv 10 Tf) ' +
      '/TextColor [0 0 1] >>',
    // A free text with no /DA at all, so the whole default triple is read.
    '<< /Type /Annot /Subtype /FreeText /Rect [300 80 460 130] /F 4 /NM (a-bare-text-box) >>',

    // Links, one per route to a target. /A wins over /Dest when both are
    // present, so the pair that carries both is the one that pins the
    // precedence; the dead link carries neither and is legal PDF.
    '<< /Type /Annot /Subtype /Link /Rect [480 80 560 110] /F 4 /NM (a-uri-link) ' +
      '/A << /S /URI /URI (https://example.test/linked) >> >>',
    '<< /Type /Annot /Subtype /Link /Rect [480 40 560 70] /F 4 /NM (a-goto-link) ' +
      '/A << /S /GoTo /D [3 0 R /XYZ 100 200 null] >> >>',
    '<< /Type /Annot /Subtype /Link /Rect [480 0 560 30] /F 4 /NM (a-dest-link) ' +
      '/Dest [3 0 R /Fit] >>',
    '<< /Type /Annot /Subtype /Link /Rect [440 0 470 30] /F 4 /NM (a-link-preferring-its-action) ' +
      '/A << /S /Named /N /NextPage >> /Dest [3 0 R /Fit] >>',
    '<< /Type /Annot /Subtype /Link /Rect [400 0 430 30] /F 4 /NM (a-dead-link) >>',
    // A script link. The target says `javascript` and carries NO script: the
    // text rides the base actions.activate tree, which is the scripting
    // plane's one home, and a copy on the target would be a second thing to
    // keep in step.
    '<< /Type /Annot /Subtype /Link /Rect [360 0 390 30] /F 4 /NM (a-script-link) ' +
      "/A << /S /JavaScript /JS (app.alert\\('hi'\\);) >> >>",
    // An /A whose /Next chains back to itself. PDFium reports the model as
    // incomplete, and an incomplete tree is never navigated, not even at its
    // root, so the target is `unsupported` rather than the URI it names.
    '<< /Type /Annot /Subtype /Link /Rect [320 0 350 30] /F 4 /NM (a-looping-link) ' +
      '/A 7 0 R >>',

    // A file attachment with a real /FS, beside one with none. The second is
    // malformed and still reads: it has a rect, a colour and an icon, and
    // losing all of that because a filespec is broken says less than it costs.
    '<< /Type /Annot /Subtype /FileAttachment /Rect [480 140 500 160] /F 4 ' +
      '/NM (an-attachment) /Name /Paperclip /FS 6 0 R >>',
    '<< /Type /Annot /Subtype /FileAttachment /Rect [510 140 530 160] /F 4 ' +
      '/NM (an-attachment-with-no-filespec) /Name /Graph >>',

    // Widgets, INERT: no /Parent and the document has no /AcroForm, so both
    // report the unattached join values on either side. The attached case
    // needs the reconciled form model, which is G8's.
    '<< /Type /Annot /Subtype /Widget /Rect [72 160 240 190] /F 4 /NM (a-text-widget) ' +
      '/MK << /BC [0 0 0] /BG [0.9 0.9 0.9] >> /DA (0 0 0 rg /Helv 10 Tf) /Q 1 ' +
      '/BS << /W 1 /S /I >> >>',
    // No /DA at all: a widget that draws no text has no font, which is not
    // the same as having the default one.
    '<< /Type /Annot /Subtype /Widget /Rect [72 120 100 148] /F 4 /NM (a-button-widget) ' +
      '/MK << /BG [1 0 0] >> >>',

    // A popup, which neither engine models: it is the PRESENTATION of
    // another annotation's comment, so both read it as unsupported. It is
    // here so the shell-only half of the comparison has a case, and it will
    // keep having one, because popup is not a subtype either side is waiting
    // to implement.
    '<< /Type /Annot /Subtype /Popup /Rect [200 100 380 200] /F 0 /NM (a-popup) /Open false >>',
  ];

  // The embedded file the attachment annotation points at. /Params carries
  // the three derived facts the metadata projection reports: a decoded size,
  // an MD5 checksum as a PDF hex string, and a creation date.
  const payload = Buffer.from('attached bytes\n', 'latin1');

  return buildPdf([
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> /Annots [' +
        annots.join(' ') +
        '] >>',
      'latin1',
    ),
    stream('', content),
    stream(
      '/Type /EmbeddedFile /Subtype /text#2Fplain /Params << ' +
        `/Size ${payload.length} /CheckSum <0123456789ABCDEF0123456789ABCDEF> ` +
        '/CreationDate (D:20240301120000Z) >>',
      payload,
    ),
    Buffer.from(
      '<< /Type /Filespec /F (notes.txt) /UF (notes.txt) /Desc (a note about the page) ' +
        '/EF << /F 5 0 R >> >>',
      'latin1',
    ),
    // The self-referential action. It has to be an indirect object, because
    // a direct dictionary cannot name itself.
    Buffer.from(
      '<< /Type /Action /S /URI /URI (https://example.test/loops) /Next 7 0 R >>',
      'latin1',
    ),
  ]);
}

/**
 * Annotations with BAKED appearance streams, which nothing else here has.
 *
 * `annotations.renderAppearances` only ever emits an annotation that carries
 * an `/AP`, and every annotation in the other two fixtures is drawn by the
 * viewer. So without this file the appearance reader returns an empty list
 * for every document in the corpus and a comparison of that list proves
 * nothing.
 *
 * What it has to separate is which annotations render ROTATION-STRIPPED. That
 * is a narrow rule and three of the annotations here are the near misses:
 *
 *   - A square with `/Rotation`, `/UnrotatedRect` and an `/AP /Matrix`. This
 *     one IS stripped: the raster draws flat and is placed by the logical box.
 *   - A free-text CALLOUT with both fields. Not stripped, because only its
 *     text box tilts, through an inline `cm` mid-stream, and the form
 *     `/Matrix` is identity.
 *   - A polygon with `/Rotation`. Not stripped: a vertex kind has its
 *     rotation baked into its geometry, so there is no matrix to remove.
 *
 * Plus a square carrying all three of `/N`, `/R` and `/D`, because the mode
 * filter has nothing to filter when every annotation has one appearance; a
 * highlight with no `/AP` at all, which must be skipped; and a widget with no
 * `/AP` key, which is the one case where the reader generates one.
 */
function annotationAppearances() {
  const content = Buffer.from('72 72 200 100 re\nf\n', 'latin1');

  // A form XObject. `extra` carries a /Matrix where one is wanted.
  const appearance = (width, height, paint, extra = '') =>
    stream(
      `/Type /XObject /Subtype /Form /FormType 1 /BBox [0 0 ${width} ${height}] ` +
        `/Resources << >>${extra ? ` ${extra}` : ''}`,
      Buffer.from(paint, 'latin1'),
    );

  // A 30-degree rotation about the form's own origin.
  const cos30 = '0.8660254';
  const sin30 = '0.5';

  const annots = [
    // Three modes on one annotation.
    '<< /Type /Annot /Subtype /Square /Rect [72 600 172 660] /F 4 /NM (a-three-mode-square) ' +
      '/AP << /N 5 0 R /R 6 0 R /D 7 0 R >> >>',
    // Print (bit 3) and NoView (bit 6) together: an annotation that prints
    // and never shows on screen. The one thing that tells a display
    // flatten from a print flatten, which are otherwise the same call with
    // a different number.
    '<< /Type /Annot /Subtype /Square /Rect [72 300 172 360] /F 36 ' +
      '/NM (a-print-only-square) /AP << /N 5 0 R >> >>',
    // The stripped case: a box kind whose rotation is in the /AP /Matrix.
    '<< /Type /Annot /Subtype /Square /Rect [220 580 350 680] /F 4 /NM (a-rotated-square) ' +
      '/AP << /N 8 0 R >> ' +
      '/EMBD_Metadata << /Rotation 30 /UnrotatedRect [240 600 340 660] >> >>',
    // The callout: both fields, and NOT stripped.
    // The /UnrotatedRect differs from the /Rect on purpose: if the two were
    // equal, stripping and not stripping would place the raster identically
    // and this annotation would not be a near miss at all.
    '<< /Type /Annot /Subtype /FreeText /Rect [72 450 240 530] /F 4 /NM (a-rotated-callout) ' +
      '/IT /FreeTextCallout /CL [60 430 100 470 72 480] /AP << /N 9 0 R >> ' +
      '/EMBD_Metadata << /Rotation 30 /UnrotatedRect [90 460 220 520] >> >>',
    // A box kind carrying the logical box and NO rotation. The pair is what
    // authorizes stripping, so half of it is not enough: this one is placed
    // by its /Rect like any ordinary annotation.
    '<< /Type /Annot /Subtype /Square /Rect [380 580 480 680] /F 4 ' +
      '/NM (a-square-with-a-box-but-no-angle) /AP << /N 11 0 R >> ' +
      '/EMBD_Metadata << /UnrotatedRect [400 600 460 660] >> >>',
    // The vertex kind: a rotation scalar and no unrotated box, so nothing to
    // strip even though the scalar is there.
    '<< /Type /Annot /Subtype /Polygon /Rect [300 420 420 520] /F 4 /NM (a-rotated-polygon) ' +
      '/Vertices [300 420 420 420 360 520] /AP << /N 10 0 R >> ' +
      '/EMBD_Metadata << /Rotation 45 >> >>',
    // A page-sized annotation. Its rect is what makes the per-appearance
    // pixel clamp reachable at a scale a test can afford: at 600 by 750 the
    // ceiling is crossed around scale 6, where a 100-point annotation would
    // need scale 50.
    '<< /Type /Annot /Subtype /Square /Rect [6 20 606 770] /F 4 /NM (a-page-sized-square) ' +
      '/AP << /N 13 0 R >> >>',
    // No /AP at all: skipped entirely, not emitted empty.
    '<< /Type /Annot /Subtype /Highlight /Rect [72 380 240 400] /F 4 /NM (a-bare-highlight) ' +
      '/QuadPoints [72 400 240 400 72 380 240 380] >>',
    // A widget with no /AP key at all, which is what the reader's generation
    // fallback exists for. It is still absent from every result, because the
    // available-modes guard skips it before the fallback can run.
    '<< /Type /Annot /Subtype /Widget /Rect [72 300 240 330] /F 4 /NM (a-bare-widget) ' +
      '/MK << /BC [0 0 0] /BG [1 1 0.8] >> /DA (0 0 0 rg /Helv 10 Tf) >>',
  ];

  return buildPdf([
    Buffer.from(
      '<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [12 0 R] >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> /Annots [' +
        annots.join(' ') +
        ' 12 0 R 14 0 R] >>',
      'latin1',
    ),
    stream('', content),

    // 5, 6, 7: the three modes, painted in different colours so a raster that
    // came from the wrong one is visible rather than merely different.
    appearance(100, 60, '1 0 0 rg\n0 0 100 60 re\nf\n'),
    appearance(100, 60, '0 1 0 rg\n0 0 100 60 re\nf\n'),
    appearance(100, 60, '0 0 1 rg\n0 0 100 60 re\nf\n'),
    // 8: the rotated square's appearance, whose /Matrix carries the angle.
    appearance(
      100,
      60,
      '0 0 1 rg\n0 0 100 60 re\nf\n',
      `/Matrix [${cos30} ${sin30} -${sin30} ${cos30} 0 0]`,
    ),
    // 9: the callout's. Identity /Matrix, with the tilt inline, which is what
    // makes it the near miss.
    appearance(
      168,
      80,
      `q\n${cos30} ${sin30} -${sin30} ${cos30} 0 0 cm\n1 1 0 rg\n0 0 168 80 re\nf\nQ\n`,
    ),
    // 10: the polygon's, already rotated into its geometry.
    appearance(120, 100, '0 1 1 rg\n0 0 120 100 re\nf\n'),
    // 11: the half-annotated square's.
    appearance(100, 100, '1 0 1 rg\n0 0 100 100 re\nf\n'),
    // 12: a merged field and widget, reachable from /AcroForm, with no /AP.
    // This is the shape PDFium can actually generate an appearance for, and
    // it STILL never gets one here, which is the point of having it.
    Buffer.from(
      '<< /Type /Annot /Subtype /Widget /Rect [72 240 240 270] /F 4 /NM (a-real-field-widget) ' +
        '/FT /Tx /T (name) /V (Ada) /DA (0 0 0 rg /Helv 10 Tf) ' +
        '/MK << /BC [0 0 0] /BG [1 1 1] >> >>',
      'latin1',
    ),
    // 13: the page-sized square's.
    appearance(600, 750, '0.5 0.5 0.5 rg\n0 0 600 750 re\nf\n'),
    // 14: an INDIRECT widget belonging to no field. It has an object number,
    // so unlike the direct one above it reaches the join's lookup, and the
    // lookup has to answer that it belongs to nothing. With only one field in
    // the document, a join that ignored the lookup and took the first field
    // would agree with the attached widget and be caught only here.
    Buffer.from(
      '<< /Type /Annot /Subtype /Widget /Rect [520 300 560 330] /F 4 ' +
        '/NM (an-orphan-widget) /MK << /BG [0 1 0] >> >>',
      'latin1',
    ),
  ]);
}

/**
 * The catalog's `/EmbeddedFiles` name tree, with the distinctions a listing
 * and a download have to make.
 *
 * Nothing in the repository had one. `annotation_kinds.pdf` embeds a file
 * through a FileAttachment annotation's `/FS`, which is the other home, and a
 * reader that only ever saw that one would never exercise the tree at all.
 *
 *   - A key that is NOT the display name. Keys are unique within the tree by
 *     construction while two `/UF` values can collide, so addressing by name
 *     is the mistake this entry exists to catch.
 *   - An entry with no `/Params`, no `/Desc` and no `/Subtype`, so every
 *     optional field of the projection is absent on one entry and present on
 *     another.
 *   - A zero-byte file, which decodes successfully to nothing. An empty file
 *     is a file, and a reader that treated the size as a failure signal would
 *     report it as broken.
 *   - A filespec with no `/EF` at all, which is what an extraction failure
 *     looks like from a document that is otherwise fine.
 *   - A tree entry whose value is not a filespec, sorted into the middle, so
 *     a reader that stopped at it rather than skipping it loses everything
 *     after it and still returns a plausible listing.
 *
 * Tree order is key order, which is what the name tree guarantees and what a
 * listing's index means. The keys here are deliberately out of order relative
 * to the object numbers so a reader that followed the objects rather than the
 * tree is visible.
 */
function attachments() {
  const notes = Buffer.from('the notes\n', 'latin1');
  const empty = Buffer.alloc(0);

  return buildPdf([
    Buffer.from(
      '<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [' +
        '(aaa-first) 4 0 R (bbb-not-a-filespec) 10 0 R (mmm-middle) 6 0 R ' +
        '(zzz-last) 8 0 R (zzz-no-stream) 9 0 R' +
        '] >> >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from('<< /Type /Page /Parent 2 0 R /Resources << >> >>', 'latin1'),

    // 4/5: every optional field present, and a key that is not the name.
    Buffer.from(
      '<< /Type /Filespec /F (notes.txt) /UF (notes.txt) /Desc (release notes) ' +
        '/EF << /F 5 0 R >> >>',
      'latin1',
    ),
    stream(
      '/Type /EmbeddedFile /Subtype /text#2Fplain /Params << ' +
        `/Size ${notes.length} /CheckSum <FEDCBA9876543210FEDCBA9876543210> ` +
        '/CreationDate (D:20240401090000Z) >>',
      notes,
    ),

    // 6/7: nothing optional at all.
    Buffer.from('<< /Type /Filespec /F (bare.bin) /UF (bare.bin) /EF << /F 7 0 R >> >>', 'latin1'),
    stream('/Type /EmbeddedFile', empty),

    // 8: a second reference to the first stream, under a different key. Two
    // entries can name one file, and a listing reports two.
    Buffer.from(
      '<< /Type /Filespec /F (notes.txt) /UF (also-notes.txt) /EF << /F 5 0 R >> >>',
      'latin1',
    ),

    // 9: a filespec with no /EF. The entry is in the tree and has a name, and
    // there is nothing to decode.
    Buffer.from('<< /Type /Filespec /F (missing.bin) /UF (missing.bin) >>', 'latin1'),

    // 10: a tree entry whose value is not a filespec at all. It sorts into
    // the MIDDLE of the tree on purpose: a reader that stopped at the first
    // entry it could not open, rather than skipping it, would lose every
    // entry after it and still return a plausible listing.
    Buffer.from('42', 'latin1'),
  ]);
}

/**
 * An AcroForm with one field of every family, plus the shapes the snapshot
 * reader has to tell apart.
 *
 * Nothing in the repository had a form. `annotation_appearances.pdf` has a
 * single merged text field, which is enough to pin the widget join and
 * nothing else.
 *
 *   - A radio group: ONE field, two widgets, one checked. The value lives on
 *     the field and the toggle state lives per widget, and a reader that put
 *     them in one place would still look right on a checkbox.
 *   - A checkbox with a non-standard export value, so the `Yes` default is
 *     not what makes the test pass.
 *   - A list box with two of three options selected, whose `/V` is therefore
 *     an ARRAY. Every other field here has a scalar one, and the lossless
 *     value entry is what keeps the difference.
 *   - A field with `/V` that is neither a string nor an array of them, which
 *     reads as present-and-unsupported rather than absent.
 *   - A field NOT listed in `/AcroForm /Fields`, reachable only through the
 *     page's `/Annots`. PDFium recovers it, and its origin says so.
 *   - A `/CO` calculation order naming one real field and one index that does
 *     not exist, because a malformed slot has to be preserved rather than
 *     dropped, or every slot after it shifts.
 *   - `/TU` and `/TM` on one field and absent on the others.
 *   - A text field with `/MaxLen` and the comb flag, beside one with neither.
 *   - A signature field, which this engine reads and never writes.
 */
function formFields() {
  // /Ff bits, 1-based per ISO 32000. Written as shifts so the numbers here
  // and the ones in the reader are the same numbers.
  const REQUIRED = 1 << 1;
  const MULTILINE = 1 << 12;
  const COMB = 1 << 24;
  const MULTI_SELECT = 1 << 21;
  // Bit 18 is Combo and bit 19 is Edit, which is one of the places a
  // 1-based table and a 0-based shift diverge. A choice field without Combo
  // is a LIST box however the other bits read.
  const COMBO = 1 << 17;
  const EDIT = 1 << 18;
  const NO_TOGGLE_TO_OFF = 1 << 14;

  // A minimal appearance so PDFium has something to attach; the widgets are
  // merged field/widget dictionaries, which is the ordinary AcroForm shape.
  const widget = (rect, extra) =>
    `/Type /Annot /Subtype /Widget /Rect [${rect}] /F 4 ${extra}`;

  return buildPdf([
    Buffer.from(
      // The last entry of /Fields is a DIRECT dictionary rather than a
      // reference. Legal, rare, and unwritable: PDFium's form write API
      // addresses a field by object number and a direct dictionary has
      // none, so the reader reports it by name and every writer refuses it.
      '<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 9 0 R ' +
        '10 0 R 11 0 R 12 0 R 13 0 R 17 0 R 18 0 R ' +
        '<< /FT /Tx /T (inline.direct) /V (no object number) >>] ' +
        '/NeedAppearances true /CO [4 0 R 99 0 R] >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Resources << >> /Annots [' +
        '4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R 13 0 R 14 0 R 17 0 R 18 0 R] >>',
      'latin1',
    ),

    // 4: a plain text field, with /TU, /TM, a /MaxLen and the comb flag, and
    // a /DV that differs from its /V.
    Buffer.from(
      `<< ${widget('72 700 300 730', '/FT /Tx /T (billing.name) /V (Ada Lovelace) ' +
        `/DV (unset) /TU (Your full name) /TM (name_export) /MaxLen 40 /Ff ${COMB | REQUIRED} ` +
        '/DA (0 0 0 rg /Helv 10 Tf)')} >>`,
      'latin1',
    ),
    // 5: a multiline text field with nothing optional at all.
    Buffer.from(
      `<< ${widget('72 620 300 690', `/FT /Tx /T (notes) /Ff ${MULTILINE}`)} >>`,
      'latin1',
    ),
    // 6: a checkbox whose export value is not the conventional /Yes.
    Buffer.from(
      `<< ${widget('72 580 92 600', '/FT /Btn /T (subscribe) /V /Agree ' +
        '/AP << /N << /Agree 15 0 R /Off 16 0 R >> >> /AS /Agree')} >>`,
      'latin1',
    ),
    // 7 and 8: the two widgets of ONE radio field (9), which owns them.
    Buffer.from(
      `<< ${widget('72 540 92 560', '/Parent 9 0 R ' +
        '/AP << /N << /Red 15 0 R /Off 16 0 R >> >> /AS /Off')} >>`,
      'latin1',
    ),
    Buffer.from(
      `<< ${widget('102 540 122 560', '/Parent 9 0 R ' +
        '/AP << /N << /Blue 15 0 R /Off 16 0 R >> >> /AS /Blue')} >>`,
      'latin1',
    ),
    // 9: the radio field itself, which has no rect of its own.
    Buffer.from(
      `<< /FT /Btn /T (colour) /V /Blue /Ff ${(1 << 15) | NO_TOGGLE_TO_OFF} /Kids [7 0 R 8 0 R] >>`,
      'latin1',
    ),
    // 10: a combo box with the edit flag, so free text is allowed.
    Buffer.from(
      `<< ${widget('320 700 500 730', '/FT /Ch /T (country) /V (FR) ' +
        `/Opt [[(FR) (France)] [(DE) (Germany)] [(JP) (Japan)]] /Ff ${COMBO | EDIT}`)} >>`,
      'latin1',
    ),
    // 11: a multi-select list box whose /V is an ARRAY.
    Buffer.from(
      `<< ${widget('320 600 500 680', '/FT /Ch /T (languages) /V [(en) (fr)] ' +
        `/Opt [[(en) (English)] [(fr) (French)] [(ja) (Japanese)]] /Ff ${MULTI_SELECT}`)} >>`,
      'latin1',
    ),
    // 12: a push button, which holds no value and is never exported.
    Buffer.from(
      `<< ${widget('320 540 400 570', '/FT /Btn /T (submit) ' +
        `/Ff ${1 << 16} /A << /S /JavaScript /JS (this.submitForm\\('u'\\);) >>`)} >>`,
      'latin1',
    ),
    // 13: a field whose /V is a number, which is neither a string nor an
    // array of them.
    Buffer.from(
      `<< ${widget('320 480 500 510', '/FT /Tx /T (quantity) /V 42')} >>`,
      'latin1',
    ),
    // 14: NOT in /AcroForm /Fields. Only the page's /Annots reaches it, which
    // is a common producer bug and which PDFium recovers.
    Buffer.from(
      `<< ${widget('72 460 300 490', '/FT /Tx /T (orphan) /V (recovered)')} >>`,
      'latin1',
    ),

    // 15/16: the on and off appearances the toggles share.
    stream(
      '/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources << >>',
      Buffer.from('0 0 0 rg\n2 2 16 16 re\nf\n', 'latin1'),
    ),
    stream(
      '/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources << >>',
      Buffer.from('1 1 1 rg\n2 2 16 16 re\nf\n', 'latin1'),
    ),

    // 17: a signature field, which this engine reads and never writes. Last
    // in the array on purpose: the appearance streams above are referenced by
    // number, so inserting anything before them renumbers them.
    Buffer.from(
      `<< ${widget('320 420 500 450', '/FT /Sig /T (signature)')} >>`,
      'latin1',
    ),
    // 18: an /FT no version of the spec defines, which is the forward
    // compatibility case. It reads as the unknown family with its /V kept as
    // text, the way an unrecognized annotation subtype does.
    Buffer.from(
      `<< ${widget('320 360 500 390', '/FT /Bogus /T (from.the.future) /V (something)')} >>`,
      'latin1',
    ),
  ]);
}

/**
 * `/PieceInfo` on both of its holders, with one value of every shape the
 * read vocabulary knows and several it does not.
 *
 * The vocabulary is five shapes over an unconstrained dictionary, so what
 * matters is the boundary: which values come back tagged and which come back
 * as present-and-unreadable. A reader that dropped the second kind, or that
 * guessed at it, would look right on every document written by this engine
 * and wrong on every document written by anything else.
 *
 * The tag also carries a distinction the characters do not. `/Draft` and
 * `(Draft)` are a name and a string, they read as the same text, and only the
 * tag says which the file held.
 *
 * Two applications on the catalog and THREE on the page, because the listing
 * is per holder and a reader that read the catalog's while claiming the
 * page's would agree with itself. More on the page than on the catalog
 * specifically: with fewer, a listing that used the wrong count would run
 * past the end, find nothing, and produce the right answer by accident.
 */
function pieceInfo() {
  // The unreadable shapes, each for its own reason: a dictionary and a
  // stream are not in the vocabulary at all, and a mixed array is an array
  // that is not a list of text strings.
  const unreadable =
    '/aDictionary << /nested (value) >> /aMixedArray [(text) 42] /aNull null';

  const privateEntries =
    '/aString (hello) /aName /Draft /aNumber 42.5 /aTrue true /aFalse false ' +
    `/aStringArray [(one) (two) (three)] /anEmptyArray [] ${unreadable}`;

  return buildPdf([
    Buffer.from(
      '<< /Type /Catalog /Pages 2 0 R /PieceInfo << ' +
        `/com.embedpdf << /Private << ${privateEntries} >> ` +
        '/LastModified (D:20240501120000Z) >> ' +
        '/OtherApp << /Private << /only (one key) >> >> >> >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    // The page's own data is DIFFERENT from the catalog's, so a reader that
    // mixed the two up produces the wrong answer rather than the same one.
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Resources << >> /PieceInfo << ' +
        '/com.embedpdf << /Private << /pageOnly (page value) /aNumber 7 >> ' +
        '/LastModified (D:20240502130000Z) >> ' +
        // Two more, so the page carries MORE applications than the catalog.
        // With fewer, a listing that used the catalog's count for the page
        // would run past the end, find nothing, and produce the right answer
        // by accident.
        '/PageOnlyApp << /Private << /k (v) >> >> ' +
        '/ZebraApp << /Private << /k (v) >> >> >> >>',
      'latin1',
    ),
  ]);
}

/**
 * A document whose Info dictionary exercises every metadata distinction.
 *
 * `metadata.read` has to tell three states apart: a key that is absent, a key
 * that is present and empty, and a key with a value. No fixture in the
 * repository had all three, and the difference is what a patch operates on, so
 * a reader that collapsed absent into empty would look correct on every
 * document here.
 *
 * Also carries a UTF-16 title with characters outside Latin-1, two custom
 * keys, and dates in both the offset and the Z forms PDF permits.
 */
function richMetadata() {
  // A UTF-16BE text string, which is how a PDF carries anything outside
  // PDFDocEncoding. The BOM is what marks it as such.
  const utf16 = (text) => {
    const units = [0xfeff, ...Array.from(text, (c) => c.codePointAt(0))].flatMap((cp) =>
      cp > 0xffff
        ? [0xd800 + ((cp - 0x10000) >> 10), 0xdc00 + ((cp - 0x10000) & 0x3ff)]
        : [cp],
    );
    return `<${units.map((u) => u.toString(16).padStart(4, '0')).join('')}>`;
  };

  const info = [
    `/Title ${utf16('Sm\u00f8rrebr\u00f8d \u4e2d\u6587 \u{1F600}')}`,
    '/Author (Ada Lovelace)',
    // Present and empty, which must not read as absent.
    '/Subject ()',
    // /Keywords is deliberately absent.
    '/Producer (EmbedPDF conformance fixtures)',
    '/Creator (generate.mjs)',
    // An offset date and a Z date, the two shapes PDF permits.
    "/CreationDate (D:20240115093000+01'00')",
    '/ModDate (D:20240220120000Z)',
    '/Trapped /False',
    // Custom keys, which the reader reports separately from the reserved ones.
    '/EmbedPDFCustom (a custom value)',
    '/EmbedPDFEmpty ()',
  ].join(' ');

  const objects = [
    Buffer.from('<< /Type /Catalog /Pages 2 0 R >>', 'latin1'),
    Buffer.from(
      '<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>',
      'latin1',
    ),
    Buffer.from(
      '<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> >>',
      'latin1',
    ),
    stream('', Buffer.from('72 72 100 100 re\nf\n', 'latin1')),
    Buffer.from(`<< ${info} >>`, 'latin1'),
  ];

  return buildPdf(objects, ' /Info 5 0 R');
}

/**
 * The permission words the encrypted fixtures declare.
 *
 * Bits 1 and 2 are reserved as zero, so a word is built by clearing the bits
 * to deny from `0xFFFFFFFC`. `| 0` because `/P` is written as a signed
 * integer in the file.
 */
const ALL_ALLOWED = 0xfffffffc;
const clear = (...bits) => (bits.reduce((word, b) => word & ~(1 << (b - 1)), ALL_ALLOWED) | 0);

const font = readFont();
const outputs = [
  ['embedded_orientations.pdf', embeddedOrientations(font)],
  ['rotated_pages.pdf', rotatedPages(font)],
  ['non_identity_text.pdf', nonIdentityText(font)],
  ['rich_metadata.pdf', richMetadata()],
  ['page_actions.pdf', pageActions()],
  ['annotation_base.pdf', annotationBase()],
  ['annotation_kinds.pdf', annotationKinds()],
  ['annotation_appearances.pdf', annotationAppearances()],
  ['attachments.pdf', attachments()],
  ['form_fields.pdf', formFields()],
  ['piece_info.pdf', pieceInfo()],
  ['document_actions.pdf', documentActions()],
  ['reverse_and_gapped_text.pdf', reverseAndGappedText(font)],

  // Revision 2, RC4-40. The oldest handler still in the wild.
  [
    'encrypted-r2-rc4-40.pdf',
    encryptedDocument({
      revision: 2,
      keyLength: 40,
      userPassword: 'user',
      ownerPassword: 'owner',
      // Printing and copying denied, so the permission word is not all-ones
      // and a reader that ignored /P would be caught.
      permissions: clear(3, 5, 12),
    }),
  ],

  // Revision 3, RC4-128.
  [
    'encrypted-r3-rc4-128.pdf',
    encryptedDocument({
      revision: 3,
      keyLength: 128,
      userPassword: 'user',
      ownerPassword: 'owner',
      permissions: clear(4, 6, 11),
    }),
  ],

  // Revision 4, AES-128. Different crypt filter, same key derivation.
  [
    'encrypted-r4-aes-128.pdf',
    encryptedDocument({
      revision: 4,
      keyLength: 128,
      userPassword: 'user',
      ownerPassword: 'owner',
      aes: true,
      permissions: clear(5),
    }),
  ],

  // Revision 6, AES-256. What a document produced today actually uses, and a
  // separate derivation: the passwords unlock the file key rather than
  // deriving it.
  [
    'encrypted-r6-aes-256.pdf',
    encryptedDocument({
      revision: 6,
      userPassword: 'user',
      ownerPassword: 'owner',
      permissions: clear(3, 4, 5, 6, 9, 11, 12),
    }),
  ],

  // Every permission granted, so the all-allowed rule has an encrypted
  // document to be true on rather than only the unencrypted path.
  [
    'encrypted-r4-all-allowed.pdf',
    encryptedDocument({
      revision: 4,
      keyLength: 128,
      userPassword: 'user',
      ownerPassword: 'owner',
      aes: true,
      permissions: ALL_ALLOWED | 0,
    }),
  ],

  // An empty user password: the document opens with no password at all and is
  // still encrypted, which is the case most likely to be mistaken for
  // unencrypted.
  [
    'encrypted-r4-empty-user-password.pdf',
    encryptedDocument({
      revision: 4,
      keyLength: 128,
      userPassword: '',
      ownerPassword: 'owner',
      aes: true,
      permissions: clear(3, 5),
    }),
  ],

  ['unsupported-security.pdf', unsupportedSecurityDocument()],
];
for (const [name, bytes] of outputs) {
  writeFileSync(resolve(here, name), bytes);
  process.stdout.write(`  written ${name} (${bytes.length} bytes)\n`);
}
