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
