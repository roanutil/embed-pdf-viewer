/**
 * Drives the Stage A geometry export inside the linked WASM module and checks
 * it against the frozen vectors.
 *
 * This is the one gate that needs Rust and PDFium in the same Emscripten
 * module. `crates/epdf-wasm/tests/borrowed_page.rs` already proves the C ABI
 * and the ownership contract on the host; what only this can prove is that the
 * page pointer PDFium hands out is the same pointer Rust can read, because
 * they share one linear memory.
 *
 * Build the module first, which is deliberately not automatic:
 *
 *   export EMSDK=packages/engine/runtime/runtime-src/third_party/emsdk
 *   export PATH="$EMSDK/upstream/emscripten:$PATH"
 *   cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release
 *   cd packages/engine/runtime
 *   node build/generate-functions.mjs wasm32-eh
 *   node build/generate-runtime-methods.mjs build/generated
 *   EMBEDPDF_RUST_STATICLIB=../../../target/wasm32-unknown-emscripten/release/libepdf_wasm.a \
 *     PDF_RUNTIME_TARGET=wasm32-eh PDF_RUNTIME_WASM_OUT_DIR=/tmp/eh-rust \
 *     bash build/compile.esm.sh
 *
 * Then, from the repository root:
 *
 *   node --experimental-strip-types \
 *     tooling/engine-conformance/src/verify-wasm-bridge.mts /tmp/eh-rust
 *
 * Plain Node rather than the workspace test runner: the module under test is
 * built on demand into a path the caller chooses, and none of the suites have
 * anywhere to put that.
 */
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { QUANTIZATION_SCALE, quantizeDeep } from './quantize.ts';

const repoRoot = resolve(import.meta.dirname, '../../..');

interface Envelope {
  readonly result?: { readonly runs: { readonly glyphs: unknown[] }[] };
  readonly error?: { readonly code: string; readonly message: string };
}

/** The `Module` surface this uses. Narrow on purpose. */
interface RuntimeModule {
  ccall(name: string, ret: string | null, argTypes: string[], args: unknown[]): number;
  _malloc(bytes: number): number;
  _free(ptr: number): void;
  HEAPU8: Uint8Array;
  HEAPU32: Uint32Array;
}

function fail(message: string): never {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

/** One crossing: lend the page, take the buffer, hand the buffer back. */
function readGeometry(mod: RuntimeModule, page: number): { envelope: Envelope; bytes: number } {
  const lenPtr = mod._malloc(4);
  try {
    const outPtr = mod.ccall('epdf_page_geometry', 'number', ['number', 'number'], [page, lenPtr]);
    if (outPtr === 0) fail('epdf_page_geometry returned null for a non-null out_len');
    const len = mod.HEAPU32[lenPtr >> 2];
    const json = new TextDecoder().decode(mod.HEAPU8.subarray(outPtr, outPtr + len));
    mod.ccall('epdf_buffer_free', null, ['number', 'number'], [outPtr, len]);
    return { envelope: JSON.parse(json) as Envelope, bytes: len };
  } finally {
    mod._free(lenPtr);
  }
}

/**
 * First divergence between two quantized trees, as a path. The twin of
 * `test_support::corpus::first_difference`, kept small rather than shared
 * because the Rust side is the one that runs in CI.
 */
function firstDifference(path: string, expected: unknown, actual: unknown): string | undefined {
  if (Array.isArray(expected) || Array.isArray(actual)) {
    if (!Array.isArray(expected) || !Array.isArray(actual)) {
      return `${path}: one side is an array and the other is not`;
    }
    if (expected.length !== actual.length) {
      return `${path}: expected ${expected.length} items, got ${actual.length}`;
    }
    for (let i = 0; i < expected.length; i++) {
      const found = firstDifference(`${path}[${i}]`, expected[i], actual[i]);
      if (found) return found;
    }
    return undefined;
  }
  if (expected && actual && typeof expected === 'object' && typeof actual === 'object') {
    const keys = new Set([...Object.keys(expected), ...Object.keys(actual)]);
    for (const key of keys) {
      const found = firstDifference(
        `${path}.${key}`,
        (expected as Record<string, unknown>)[key],
        (actual as Record<string, unknown>)[key],
      );
      if (found) return found;
    }
    return undefined;
  }
  return Object.is(expected, actual)
    ? undefined
    : `${path}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`;
}

async function main(): Promise<void> {
  const moduleDir = process.argv[2];
  if (!moduleDir) fail('usage: verify-wasm-bridge.mts <directory holding embedpdf.node.js>');

  const factory = (await import(resolve(moduleDir, 'embedpdf.node.js'))).default as (
    options: Record<string, unknown>,
  ) => Promise<RuntimeModule>;
  const mod = await factory({
    locateFile: (file: string) => resolve(moduleDir, file),
    print: () => {},
    printErr: (message: string) => process.stderr.write(`[pdfium] ${message}\n`),
  });
  mod.ccall('FPDF_InitLibrary', null, [], []);

  const vectors = JSON.parse(
    await readFile(
      resolve(repoRoot, 'tooling/engine-conformance/vectors/pages.geometry/report.json'),
      'utf8',
    ),
  ) as { fixture: string; pages: { index: number; snapshot: unknown }[] };

  const bytes = new Uint8Array(await readFile(resolve(repoRoot, vectors.fixture)));
  const input = mod._malloc(bytes.length);
  mod.HEAPU8.set(bytes, input);
  const doc = mod.ccall(
    'FPDF_LoadMemDocument',
    'number',
    ['number', 'number', 'string'],
    [input, bytes.length, null],
  );
  if (doc === 0) fail(`FPDF_LoadMemDocument refused ${vectors.fixture}`);

  const failures: string[] = [];
  for (const page of vectors.pages) {
    const handle = mod.ccall('FPDF_LoadPage', 'number', ['number', 'number'], [doc, page.index]);
    if (handle === 0) fail(`FPDF_LoadPage refused page ${page.index}`);
    try {
      const started = performance.now();
      const { envelope, bytes: payload } = readGeometry(mod, handle);
      const elapsed = performance.now() - started;
      if (envelope.error) {
        failures.push(`page ${page.index}: Rust returned ${envelope.error.code}`);
        continue;
      }
      const runs = envelope.result!.runs;
      const glyphs = runs.reduce((n, run) => n + run.glyphs.length, 0);
      const difference = firstDifference(
        `page ${page.index}`,
        quantizeDeep(page.snapshot),
        quantizeDeep(envelope.result),
      );
      if (difference) failures.push(difference);
      process.stdout.write(
        `  ${difference ? 'DIFFERS' : 'ok     '} page ${page.index}: ` +
          `${runs.length} runs, ${glyphs} glyphs, ${payload} bytes, ${elapsed.toFixed(1)}ms\n`,
      );

      // Stage A's rule, checked rather than assumed: the page is still the
      // host's after Rust has read it.
      const width = mod.ccall('FPDF_GetPageWidthF', 'number', ['number'], [handle]);
      if (!(width > 0)) failures.push(`page ${page.index}: unusable by its owner after the borrow`);
    } finally {
      mod.ccall('FPDF_ClosePage', null, ['number'], [handle]);
    }
  }

  // The error path crosses the same boundary and must not be a crash.
  const { envelope: refused } = readGeometry(mod, 0);
  if (refused.error?.code !== 'InvalidArg') {
    failures.push(`a null page should report InvalidArg, got ${JSON.stringify(refused.error)}`);
  }

  mod.ccall('FPDF_CloseDocument', null, ['number'], [doc]);
  mod._free(input);

  if (failures.length > 0) {
    fail(`\n${failures.length} check(s) failed:\n  ${failures.join('\n  ')}`);
  }
  process.stdout.write(
    `  ok      null page refused, geometry frozen at 1/${QUANTIZATION_SCALE} of a point\n`,
  );
}

await main();
