/**
 * Fails when the built WASM payload has no Rust engine core.
 *
 * The suites that exercise the core skip themselves on a runtime without it,
 * which is the right behaviour for a developer on a stock payload and the
 * wrong thing to rely on in CI: a payload that quietly lost the core would
 * turn every one of them green while running nothing. This is the assertion
 * that makes those skips safe.
 *
 * Checks the exports rather than the file size, because the interesting
 * failure is a link that succeeded and dropped the symbols.
 */
import { createPdfRuntime } from '@embedpdf/engine-runtime';

const runtime = await createPdfRuntime({ prefer: 'wasm' });
try {
  const missing = [];
  if (!runtime.rust) {
    missing.push('the whole core (no epdf_page_geometry export)');
  } else {
    if (typeof runtime.rust.pageGeometry !== 'function') missing.push('pageGeometry');
    if (typeof runtime.rust.createHost !== 'function') missing.push('createHost');
  }

  if (missing.length > 0) {
    process.stderr.write(
      `The WASM payload is missing: ${missing.join(', ')}.\n` +
        'Build it with the Rust core:\n' +
        '  pnpm --filter @embedpdf/engine-runtime build:target wasm32\n' +
        'which builds crates/epdf-wasm and links it. EMBEDPDF_RUST_CORE=0 skips that,\n' +
        'and a payload built that way cannot serve any Rust path.\n',
    );
    process.exitCode = 1;
  } else {
    process.stdout.write('  ok      the WASM payload carries the Rust engine core\n');
  }
} finally {
  await runtime.destroy();
}
