/**
 * Writes vectors/report-page4.json and vectors/report-p4-scale0.25.bgra ONCE,
 * from the shipping TypeScript path, and is then never run again by a test.
 * That is the whole point: a Rust, Swift, Kotlin or JS host that drifts from
 * these numbers fails a test instead of being discovered in a browser. Same
 * mechanism as viewer-rust-port-benchmarks/vectors/anchor-vectors.json.
 *
 * Requires `pnpm --filter @embedpdf/engine-runtime build` first.
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createPdfRuntime } from '../../packages/engine/runtime/dist/index.node.js';
import { engineRuntimeBuildId } from '../../packages/engine/runtime/dist/build-id.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const PAGE_INDEX = 4;
const SCALE = 0.25;
const QUERY = 'the';

const pdfBytes = readFileSync(resolve(root, 'fixtures/report.pdf'));
const rt = await createPdfRuntime({ prefer: 'native' });
// createPdfRuntime({ prefer: 'native' }) still falls through to
// createWasmRuntime() when resolveRuntimeTarget() returns null or
// 'wasm32' (see packages/engine/runtime/src/index.node.ts) — the
// 'native' preference only guards the catch around a *failed* native
// load, not that fall-through. The golden buffer's entire purpose is to
// pin native PDFium rasterization, so a wasm-backed regeneration must
// never write over it silently.
if (rt.kind !== 'native') {
  throw new Error(
    `generate-vectors.mjs must run against the native runtime to produce ` +
      `the golden buffer, but createPdfRuntime({ prefer: 'native' }) resolved ` +
      `kind=${rt.kind} platform=${rt.platform}. Run this on a platform with a ` +
      `native PDFium addon (see resolveRuntimeTarget in ` +
      `packages/engine/runtime/src/core/platform.node.ts).`,
  );
}
const { fn, mem } = rt;
const runtimeProvenance = { kind: rt.kind, platform: rt.platform, buildId: engineRuntimeBuildId() };

fn.FPDF_InitLibrary();

const buf = mem.alloc(pdfBytes.length);
mem.writeBytes(buf, pdfBytes);
// FPDF_LoadMemDocument's third parameter is a password string (cwrap
// "cstring"), not a Ptr. The fixture is not encrypted, so pass ''.
const doc = fn.FPDF_LoadMemDocument(buf, pdfBytes.length, '');
if (!doc) throw new Error('FPDF_LoadMemDocument failed');

const pageCount = fn.FPDF_GetPageCount(doc);
const page = fn.FPDF_LoadPage(doc, PAGE_INDEX);
if (!page) throw new Error(`FPDF_LoadPage failed for index ${PAGE_INDEX}`);

const size = {
  width: fn.FPDF_GetPageWidthF(page),
  height: fn.FPDF_GetPageHeightF(page),
};

const textPage = fn.FPDFText_LoadPage(page);
if (!textPage) throw new Error('FPDFText_LoadPage failed');
const charCount = fn.FPDFText_CountChars(textPage);

// EPDFText_GetTextFull follows PDFium's two-call contract: called with a
// null buffer and buffer_len 0 it returns the required UTF-16 unit count,
// INCLUDING the two-byte NUL terminator (see
// packages/engine/runtime/runtime-src/fpdfsdk/fpdf_text.cpp). The second
// call fills a buffer of exactly that many units. This mirrors
// packages/engine/services/src/features/text/PageTextReader.ts, the
// shipping TS path, including its manual UTF-16LE decode (rather than
// mem.readU16String, which stops at the first NUL and would give the same
// result here only by assuming the page text has none).
const needed = fn.EPDFText_GetTextFull(textPage, 0n, 0);
const textBuf = mem.alloc(needed * 2);
fn.EPDFText_GetTextFull(textPage, textBuf, needed);
const text = decodeUtf16Le(mem.readBytes(textBuf, (needed - 1) * 2));
mem.free(textBuf);

function decodeUtf16Le(bytes) {
  const units = new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength >> 1);
  let out = '';
  const CHUNK = 0x2000;
  for (let i = 0; i < units.length; i += CHUNK) {
    out += String.fromCharCode(...units.subarray(i, Math.min(i + CHUNK, units.length)));
  }
  return out;
}

const search = (() => {
  const queryBuf = mem.writeU16String(QUERY);
  const handle = fn.FPDFText_FindStart(textPage, queryBuf, 0, 0);
  const firstHits = [];
  let hitCount = 0;
  while (fn.FPDFText_FindNext(handle)) {
    const charIndex = fn.FPDFText_GetSchResultIndex(handle);
    const count = fn.FPDFText_GetSchCount(handle);
    if (firstHits.length < 3) {
      const rectCount = fn.FPDFText_CountRects(textPage, charIndex, count);
      if (rectCount < 0) {
        throw new Error(`FPDFText_CountRects failed for charIndex ${charIndex}, count ${count}`);
      }
      const rects = [];
      for (let i = 0; i < rectCount; i += 1) {
        const out = mem.alloc(8 * 4);
        // FPDFText_GetRect takes four distinct double* out-params; Ptr is
        // bigint, so BigInt arithmetic is required for the pointer
        // arguments themselves.
        const ok = fn.FPDFText_GetRect(textPage, i, out, out + 8n, out + 16n, out + 24n);
        if (!ok) {
          throw new Error(`FPDFText_GetRect failed for rect ${i} of charIndex ${charIndex}`);
        }
        rects.push([
          Number(mem.peek(out, 'f64', 0)),
          Number(mem.peek(out, 'f64', 8)),
          Number(mem.peek(out, 'f64', 16)),
          Number(mem.peek(out, 'f64', 24)),
        ].map((n) => Number(n.toFixed(4))));
        mem.free(out);
      }
      firstHits.push({ charIndex, charCount: count, rects });
    }
    hitCount += 1;
  }
  fn.FPDFText_FindClose(handle);
  mem.free(queryBuf);
  // The third argument to FPDFText_FindStart is the FPDF_FIND_OPTIONS
  // flags bitmask. 0 means neither FPDF_MATCHCASE nor FPDF_CONSECUTIVE:
  // the search is case-insensitive, and overlapping matches are skipped
  // rather than all reported.
  return { query: QUERY, caseSensitive: false, flags: 0, hitCount, firstHits };
})();

const width = Math.max(1, Math.round(size.width * SCALE));
const height = Math.max(1, Math.round(size.height * SCALE));
const stride = width * 4;
const pixels = mem.alloc(stride * height);
const bitmap = fn.FPDFBitmap_CreateEx(width, height, 4 /* FPDFBitmap_BGRA */, pixels, stride);
if (!bitmap) {
  throw new Error(`FPDFBitmap_CreateEx failed for ${width}x${height} stride ${stride}`);
}
fn.FPDFBitmap_FillRect(bitmap, 0, 0, width, height, 0xffffffff);
fn.FPDF_RenderPageBitmap(bitmap, page, 0, 0, width, height, 0, 0);
const golden = Buffer.from(mem.readBytes(pixels, stride * height));
fn.FPDFBitmap_Destroy(bitmap);
mem.free(pixels);

fn.FPDFText_ClosePage(textPage);
fn.FPDF_ClosePage(page);
fn.FPDF_CloseDocument(doc);
mem.free(buf);
fn.FPDF_DestroyLibrary();
await rt.destroy();

mkdirSync(resolve(root, 'vectors'), { recursive: true });
writeFileSync(resolve(root, 'vectors/report-p4-scale0.25.bgra'), golden);
writeFileSync(
  resolve(root, 'vectors/report-page4.json'),
  JSON.stringify(
    {
      fixture: 'fixtures/report.pdf',
      fixtureSha256: createHash('sha256').update(pdfBytes).digest('hex'),
      forkSha: 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6',
      generatedBy: 'build/generate-vectors.mjs',
      pageIndex: PAGE_INDEX,
      pageCount,
      size: { width: Number(size.width.toFixed(4)), height: Number(size.height.toFixed(4)) },
      charCount,
      text,
      search,
      render: {
        scale: SCALE,
        width,
        height,
        stride,
        golden: 'report-p4-scale0.25.bgra',
        // Both bounds, and both are required reading. Five suites read
        // maxAbsDiffTolerance with no fallback -- crates/test-support/src/lib.rs:71
        // unwraps, VectorsTests.swift:56 force-casts, VectorsTest.kt:75 calls
        // getInt, packages/{node,web}/vectors.test.mjs and apps/node-cli read
        // it straight -- so a vectors file regenerated without it takes all
        // five down at once.
        meanAbsDiffTolerance: 2.0,
        maxAbsDiffTolerance: 16,
        // FPDFBitmap_FillRect's fill color, FPDF_RenderPageBitmap's
        // rotate/flags arguments, and the fact that FPDF_FFLDraw is never
        // called (no form rendering): a reimplementation that gets any of
        // those four wrong changes a pixel on this page, so the golden
        // comparison catches it. `format` does not: page 4 is entirely
        // grayscale (B == G == R in every pixel, alpha 255 throughout), so
        // a reimplementation that returns RGBA, or any other permutation of
        // B, G and R, produces byte-identical output against this golden.
        // See packages/swift/EpdfOps/Tests/EpdfOpsTests/VectorsTests.swift
        // and packages/kotlin/epdf-ops/src/test/kotlin/com/embedpdf/scaffold/VectorsTest.kt
        // for the synthetic, non-grayscale tests that actually cover
        // channel order, plus apps/web-demo/bgra-to-rgba.test.mjs.
        format: 4, // FPDFBitmap_BGRA
        fillColor: 4294967295, // 0xFFFFFFFF, passed to FPDFBitmap_FillRect
        rotate: 0, // FPDF_RenderPageBitmap's rotate argument
        flags: 0, // FPDF_RenderPageBitmap's flags argument
        drawForms: false, // FPDF_FFLDraw is deliberately never called
      },
      // Provenance: the golden buffer only means anything if it was
      // produced by the native runtime (see the rt.kind check above).
      // buildId is `version:target` from
      // packages/engine/runtime/src/build-id.ts#engineRuntimeBuildId.
      runtime: runtimeProvenance,
    },
    null,
    2,
  ) + '\n',
);

console.error(`wrote vectors: ${pageCount} pages, ${charCount} chars, ${search.hitCount} hits, ${width}x${height} render`);
