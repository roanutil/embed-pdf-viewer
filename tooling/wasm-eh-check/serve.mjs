#!/usr/bin/env node
/**
 * Serves the module under test, a fixture PDF and one page that exercises
 * them. Nothing else, and nothing from the repository's build graph: the
 * question is whether a browser will run this wasm, and anything else in the
 * page is a second thing that could fail.
 *
 * `EH_MODULE_DIR` is the directory holding `embedpdf.browser.js` and
 * `embedpdf.wasm`. Built per `tooling/engine-conformance/README.md`.
 *
 * Cross-origin isolation headers are set because a shared-memory build would
 * need them and a future one might; they cost nothing here.
 */
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve } from 'node:path';

const moduleDir = resolve(process.env.EH_MODULE_DIR ?? '/tmp/eh-rust');
const repoRoot = resolve(import.meta.dirname, '../..');
const fixture = resolve(repoRoot, 'pdfium-multiplatform-prototype/fixtures/report.pdf');
const port = Number(process.env.EH_PORT ?? 5311);

for (const required of [join(moduleDir, 'embedpdf.browser.js'), join(moduleDir, 'embedpdf.wasm')]) {
  if (!existsSync(required)) {
    console.error(
      `missing ${required}\n` +
        'Build the module first; the recipe is in tooling/engine-conformance/README.md.',
    );
    process.exit(1);
  }
}

const TYPES = {
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.wasm': 'application/wasm',
  '.pdf': 'application/pdf',
  '.html': 'text/html',
};

const PAGE = `<!doctype html>
<meta charset="utf-8">
<title>wasm32-eh check</title>
<body>
<pre id="out">running…</pre>
<script type="module">
// Deliberately plain: no bundler, no framework, no viewer. If this page fails,
// the wasm failed.
const report = { ok: false, steps: [], error: null };
const step = (name, value) => report.steps.push({ name, value });
try {
  const factory = (await import('./embedpdf.browser.js')).default;
  const mod = await factory({
    locateFile: (f) => \`./\${f}\`,
    print: () => {},
    printErr: (m) => step('stderr', String(m)),
  });
  step('instantiated', true);

  mod.ccall('FPDF_InitLibrary', null, [], []);
  const bytes = new Uint8Array(await (await fetch('./report.pdf')).arrayBuffer());
  const buf = mod._malloc(bytes.length);
  mod.HEAPU8.set(bytes, buf);

  const doc = mod.ccall('FPDF_LoadMemDocument', 'number', ['number','number','string'], [buf, bytes.length, null]);
  step('documentOpened', doc !== 0);
  step('pageCount', mod.ccall('FPDF_GetPageCount', 'number', ['number'], [doc]));

  const page = mod.ccall('FPDF_LoadPage', 'number', ['number','number'], [doc, 0]);
  const tp = mod.ccall('FPDFText_LoadPage', 'number', ['number'], [page]);
  step('charCount', mod.ccall('FPDFText_CountChars', 'number', ['number'], [tp]));
  mod.ccall('FPDFText_ClosePage', null, ['number'], [tp]);

  // The raster path reaches fxcodec, whose libpng and libjpeg error handling is
  // the setjmp/longjmp this build lowers through wasm exception handling. If
  // -sSUPPORT_LONGJMP=wasm were wrong, this is where it would show.
  const w = 200, h = 260;
  const bmp = mod.ccall('FPDFBitmap_Create', 'number', ['number','number','number'], [w, h, 1]);
  mod.ccall('FPDFBitmap_FillRect', null, ['number','number','number','number','number','number'], [bmp, 0, 0, w, h, 0xFFFFFFFF]);
  mod.ccall('FPDF_RenderPageBitmap', null, ['number','number','number','number','number','number','number','number'], [bmp, page, 0, 0, w, h, 0, 0]);
  const px = mod.HEAPU8.subarray(mod.ccall('FPDFBitmap_GetBuffer', 'number', ['number'], [bmp]),
    mod.ccall('FPDFBitmap_GetBuffer', 'number', ['number'], [bmp]) + mod.ccall('FPDFBitmap_GetStride', 'number', ['number'], [bmp]) * h);
  let nonWhite = 0;
  for (let i = 0; i < px.length; i += 4) if (px[i] !== 255 || px[i+1] !== 255 || px[i+2] !== 255) nonWhite++;
  step('renderedNonWhitePixels', nonWhite);
  mod.ccall('FPDFBitmap_Destroy', null, ['number'], [bmp]);
  mod.ccall('FPDF_ClosePage', null, ['number'], [page]);

  // The C++ exception path: a wrong password is a thrown-and-caught failure
  // inside PDFium, which is what -fwasm-exceptions has to carry.
  const encrypted = new Uint8Array(await (await fetch('./encrypted.pdf')).arrayBuffer());
  const ebuf = mod._malloc(encrypted.length);
  mod.HEAPU8.set(encrypted, ebuf);
  const refused = mod.ccall('FPDF_LoadMemDocument', 'number', ['number','number','string'], [ebuf, encrypted.length, null]);
  step('wrongPasswordRefused', refused === 0);
  step('lastError', mod.ccall('FPDF_GetLastError', 'number', [], []));
  mod._free(ebuf);

  mod.ccall('FPDF_CloseDocument', null, ['number'], [doc]);
  mod._free(buf);

  // The Rust core, when this module was linked with it.
  const hasRust = typeof mod._epdf_page_geometry === 'function';
  step('rustCoreLinked', hasRust);
  if (hasRust) {
    const doc2 = (() => {
      const b = mod._malloc(bytes.length);
      mod.HEAPU8.set(bytes, b);
      return { doc: mod.ccall('FPDF_LoadMemDocument', 'number', ['number','number','string'], [b, bytes.length, null]), b };
    })();
    const p = mod.ccall('FPDF_LoadPage', 'number', ['number','number'], [doc2.doc, 0]);
    const lenPtr = mod._malloc(4);
    const out = mod.ccall('epdf_page_geometry', 'number', ['number','number'], [p, lenPtr]);
    const len = mod.HEAPU32[lenPtr >> 2];
    const json = JSON.parse(new TextDecoder().decode(mod.HEAPU8.subarray(out, out + len)));
    mod.ccall('epdf_buffer_free', null, ['number','number'], [out, len]);
    mod._free(lenPtr);
    step('rustGeometryRuns', json.result ? json.result.runs.length : null);
    step('rustGeometryGlyphs', json.result ? json.result.runs.reduce((n, r) => n + r.glyphs.length, 0) : null);
    mod.ccall('FPDF_ClosePage', null, ['number'], [p]);
    mod.ccall('FPDF_CloseDocument', null, ['number'], [doc2.doc]);
    mod._free(doc2.b);
  }

  report.ok = true;
} catch (error) {
  report.error = error && error.stack ? error.stack : String(error);
}
window.__ehResult = report;
document.getElementById('out').textContent = JSON.stringify(report, null, 2);
</script>
`;

createServer((req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  const headers = {
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
  };
  if (url.pathname === '/favicon.ico') {
    // Browsers ask unprompted, and a 404 reaches the page's console, where the
    // test treats console errors as failures. Answering with nothing is
    // cheaper than teaching the test to ignore one specific message.
    res.writeHead(204, headers);
    res.end();
    return;
  }
  if (url.pathname === '/' || url.pathname === '/index.html') {
    res.writeHead(200, { ...headers, 'Content-Type': 'text/html' });
    res.end(PAGE);
    return;
  }
  const file =
    url.pathname === '/report.pdf'
      ? fixture
      : url.pathname === '/encrypted.pdf'
        ? resolve(repoRoot, 'pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf')
        : join(moduleDir, url.pathname.replace(/^\/+/, ''));
  if (!existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, headers);
    res.end('not found');
    return;
  }
  res.writeHead(200, {
    ...headers,
    'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream',
  });
  createReadStream(file).pipe(res);
}).listen(port, '127.0.0.1', () => {
  console.log(`wasm-eh-check serving ${moduleDir} on http://127.0.0.1:${port}`);
});
