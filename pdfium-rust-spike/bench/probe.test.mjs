import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

const here = import.meta.dirname;
const PDF = resolve(here, '../../examples/react/public/ebook.pdf');

test('a Rust function linked into the PDFium wasm module returns the same page size as PDFium', async () => {
  const { default: createProbe } = await import('../build/out/probe.mjs');
  const m = await createProbe();

  m.cwrap('FPDF_InitLibrary', null, [])();

  const bytes = readFileSync(PDF);
  const buf = m._malloc(bytes.length);
  m.HEAPU8.set(bytes, buf);
  const doc = m.cwrap('FPDF_LoadMemDocument', 'number', ['number', 'number', 'number'])(
    buf,
    bytes.length,
    0,
  );
  assert.notEqual(doc, 0, 'FPDF_LoadMemDocument returned null');

  const out = m._malloc(8);

  const direct = m.cwrap('EPDF_GetPageSizeByIndexNormalized', 'number', [
    'number',
    'number',
    'number',
  ])(doc, 0, out);
  assert.equal(direct, 1);
  const directW = m.getValue(out, 'float');
  const directH = m.getValue(out + 4, 'float');

  const viaRust = m.cwrap('rs_probe_page_size', 'number', ['number', 'number', 'number'])(
    doc,
    0,
    out,
  );
  assert.equal(viaRust, 1, 'rs_probe_page_size failed');
  assert.equal(m.getValue(out, 'float'), directW);
  assert.equal(m.getValue(out + 4, 'float'), directH);

  assert.ok(directW > 0 && directH > 0, `implausible page size ${directW}x${directH}`);

  m._free(out);
  m._free(buf);
  m.cwrap('FPDF_CloseDocument', null, ['number'])(doc);
});
