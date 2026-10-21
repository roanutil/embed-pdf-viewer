/**
 * Emits `vectors/signature/protection.json`: what `deriveProtection` and
 * `protectedCapabilities` answer for a document's signatures.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection
 *   pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection:check
 *
 * `crates/epdf-engine/tests/signature_protection.rs` replays it against the
 * Rust port in `features/signature/protection.rs`.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  deriveProtection,
  protectedCapabilities,
} from '../../../packages/engine/core/src/signature/protection';
import type {
  DocMdpPermission,
  FieldLockSpec,
  SignatureDTO,
} from '../../../packages/engine/core/src/signature/types';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'signature');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface SignatureShape {
  signed: boolean;
  docMdp?: DocMdpPermission;
  catalogCertification?: boolean;
  fieldMdp?: FieldLockSpec;
  lock?: FieldLockSpec;
}

/**
 * A signature as `signatures.list` reads one. The field ref's
 * `fieldObjectNumber` is `objectNumber` since 595bdc6c7 ("Engine API one
 * name for object numbers"), and the signer's `claimedTime` is `signedAt`
 * since fd175ba50 ("One date type IsoDateTime"). Neither is read by
 * `deriveProtection`.
 */
function signature(index: number, shape: SignatureShape): SignatureDTO {
  return {
    index,
    field: { kind: 'objectNumber', objectNumber: 10 + index },
    fieldName: `Sig${index}`,
    widget: null,
    signed: shape.signed,
    kind: 'signature',
    filter: shape.signed ? 'Adobe.PPKLite' : null,
    subFilter: shape.signed ? 'ETSI.CAdES.detached' : null,
    byteRange: shape.signed ? [0, 100, 200, 50] : null,
    contentsSize: shape.signed ? 8192 : 0,
    coverage: shape.signed ? 'whole-revision' : null,
    revisionIndex: shape.signed ? index : null,
    signer: { name: null, reason: null, location: null, contactInfo: null, signedAt: null },
    docMdp: shape.docMdp ?? null,
    catalogCertification: shape.catalogCertification ?? false,
    fieldMdp: shape.fieldMdp ?? null,
    lock: shape.lock ?? null,
    seedValue: null,
  };
}

const CASES: ReadonlyArray<{ name: string; covers: string; signatures: SignatureShape[] }> = [
  { name: 'empty', covers: 'No signature fields at all.', signatures: [] },
  {
    name: 'unsigned-field',
    covers: 'An unsigned field protects nothing, even with a /Lock.',
    signatures: [{ signed: false, lock: { action: 'all', fields: [], permission: 1 } }],
  },
  { name: 'one-approval', covers: 'An approval declares nothing.', signatures: [{ signed: true }] },
  {
    name: 'two-approvals',
    covers: 'Approvals stay undeclared however many there are.',
    signatures: [{ signed: true }, { signed: true }],
  },
  ...([1, 2, 3] as const).map((p) => ({
    name: `certified-p${p}`,
    covers: `A catalog certification with P=${p}.`,
    signatures: [{ signed: true, docMdp: p, catalogCertification: true }],
  })),
  {
    name: 'docmdp-without-catalog',
    covers: 'A /Reference DocMDP the catalog does not name is not a certification.',
    signatures: [{ signed: true, docMdp: 1 }],
  },
  {
    name: 'second-certification-ignored',
    covers: 'Only the first catalog certification counts.',
    signatures: [
      { signed: true, docMdp: 3, catalogCertification: true },
      { signed: true, docMdp: 1, catalogCertification: true },
    ],
  },
  {
    name: 'fieldmdp-include',
    covers: 'A FieldMDP transform becomes a field lock and declares nothing.',
    signatures: [{ signed: true, fieldMdp: { action: 'include', fields: ['a', 'b.c'] } }],
  },
  {
    name: 'lock-with-permission',
    covers: 'A signed field /Lock with /P declares a level.',
    signatures: [{ signed: true, lock: { action: 'exclude', fields: ['x'], permission: 2 } }],
  },
  {
    name: 'lock-tightens-certification',
    covers: 'The lowest declared level wins.',
    signatures: [
      { signed: true, docMdp: 3, catalogCertification: true },
      { signed: true, lock: { action: 'all', fields: [], permission: 1 } },
    ],
  },
  {
    name: 'fieldmdp-and-lock',
    covers: 'Both lock sources from one signature, in order.',
    signatures: [
      {
        signed: true,
        fieldMdp: { action: 'all', fields: [] },
        lock: { action: 'all', fields: [] },
      },
    ],
  },
];

function serialize(kind: string, entries: readonly unknown[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, kind }, null, 2).replace(
    /\n}$/,
    ',',
  );
  const body = entries.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      signature/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS signature/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written signature/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const entries = CASES.map((c) => {
    const signatures = c.signatures.map((s, i) => signature(i, s));
    const protection = deriveProtection(signatures);
    return {
      name: c.name,
      covers: c.covers,
      signatures,
      protection,
      protectedCapabilities: [...protectedCapabilities(protection)].sort(),
    };
  });
  const differed = await emit('protection.json', serialize('signature.protection', entries), check);
  if (check && differed) {
    process.stderr.write(
      '\nSignature protection vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-signature-protection-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
