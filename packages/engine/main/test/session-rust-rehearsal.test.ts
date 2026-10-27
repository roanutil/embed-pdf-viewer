import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';
import { createPdfRuntime } from '@embedpdf/engine-runtime';
import { LocalEngine } from '../src/index';
import { InlineTransport } from '../src/transport/InlineTransport';

/**
 * G3w: a Rust-owned document session, under the real transport.
 *
 * `crates/epdf-engine/tests/host_sessions.rs` covers the same lifecycle
 * inside Rust. This covers what that cannot: the transport between the two.
 * The bytes cross as a payload, the security probe comes back through the
 * envelope, and the close reaches the other side.
 *
 * The rehearsal routes NO reads. That is the whole point of it: G10 moves
 * session authority for every document, and if that were the first time a
 * Rust-owned session ran on web then a transport failure, a framing failure
 * and a lifetime failure would all surface at once in a change that also
 * moved every read. So this moves the session alone, and a read that reaches
 * one is asserted to be reported as a routing bug rather than served.
 *
 * Skipped unless the runtime was built with the Rust core linked in. Build
 * one with the recipe in `tooling/engine-conformance/README.md` and point
 * `EMBEDPDF_WASM_URL` at it.
 */
const here = dirname(fileURLToPath(import.meta.url));
const PLAIN = resolve(here, '../../../../tooling/engine-conformance/fixtures/annotation_kinds.pdf');
const ENCRYPTED = resolve(
  here,
  '../../../../tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf',
);

async function makeEngine() {
  const runtime = await createPdfRuntime({ prefer: 'wasm' });
  return LocalEngine.fromTransport({
    transport: new InlineTransport(runtime, { rustSession: true }),
  });
}

const hasDispatcher = await (async () => {
  const runtime = await createPdfRuntime({ prefer: 'wasm' });
  const present = runtime.rust?.createHost !== undefined;
  await runtime.destroy();
  return present;
})();

describe.skipIf(!hasDispatcher)('G3w: the session on the Rust side', () => {
  test('a document opens and closes without a single read', async () => {
    // The gate, verbatim. Eight rounds rather than one, because a leak that
    // costs one session per open is invisible in a single pass and obvious
    // in eight.
    const engine = await makeEngine();
    const bytes = new Uint8Array(await readFile(PLAIN));
    try {
      for (let round = 0; round < 8; round++) {
        const doc = await engine.open({ kind: 'bytes', id: `g3w-${round}`, bytes });
        expect(doc.security.state.encryption.state, `round ${round}`).toBe('none');
        await doc.close();
      }
    } finally {
      await engine.destroy();
    }
  });

  test('a password comes back as a state rather than a failure', async () => {
    // The client handle comes up locked and a later unlock loads the parked
    // bytes. An open that threw here would make the caller tear down a
    // session that is waiting for a prompt.
    const engine = await makeEngine();
    const bytes = new Uint8Array(await readFile(ENCRYPTED));
    try {
      const doc = await engine.open({ kind: 'bytes', id: 'g3w-locked', bytes });
      expect(doc.security.state.encryption.state).toBe('encrypted');
      expect(doc.security.state.encryption.requiresPassword).toBe(true);
      await doc.close();
    } finally {
      await engine.destroy();
    }
  });

  test('a read is reported as a routing bug rather than served', async () => {
    // The rehearsal's boundary, asserted rather than assumed. Reporting
    // "document not open" would be true and useless: the document IS open,
    // on the other side, and the routing is what is wrong.
    const engine = await makeEngine();
    const bytes = new Uint8Array(await readFile(PLAIN));
    try {
      const doc = await engine.open({ kind: 'bytes', id: 'g3w-read', bytes });
      await expect(doc.pages.list()).rejects.toMatchObject({ code: 'NotImplemented' });
      await doc.close();
    } finally {
      await engine.destroy();
    }
  });

  test('the flag off leaves the session where it was', async () => {
    // The profile guarantee. The rehearsal is internal and must not change
    // what a caller who did not ask for it gets, so the same document under
    // the default options reads normally.
    const runtime = await createPdfRuntime({ prefer: 'wasm' });
    const engine = LocalEngine.fromTransport({ transport: new InlineTransport(runtime) });
    const bytes = new Uint8Array(await readFile(PLAIN));
    try {
      const doc = await engine.open({ kind: 'bytes', id: 'g3w-off', bytes });
      const { pages } = await doc.pages.list();
      expect(pages.length).toBeGreaterThan(0);
      await doc.close();
    } finally {
      await engine.destroy();
    }
  });
});
