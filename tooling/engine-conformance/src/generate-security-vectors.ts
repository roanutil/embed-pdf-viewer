/**
 * Emits `vectors/security/state.json`: what `buildSecurityInfo`,
 * `openedAsFromCode`, `advisoryFromPdfBits` and `securityStateFromProbe`
 * produce.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run security
 *   pnpm --filter @embedpdf/tooling-engine-conformance run security:check
 *
 * Part of G3's gate. `crates/epdf-engine/tests/security_state.rs` replays it.
 * This is the derivation half, which needs no document; the half that reads a
 * real encrypted file is separate.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { decodePdfBits } from '../../../packages/engine/core/src/auth/scope/pdf-bits';
import {
  advisoryFromPdfBits,
  securityStateFromProbe,
} from '../../../packages/engine/core/src/engine/document-security-state';
import type { DocumentSecurityProbeInfo } from '../../../packages/engine/core/src/wire/worker-protocol';
import {
  buildSecurityInfo,
  openedAsFromCode,
} from '../../../packages/engine/services/src/features/security/internal/buildSecurityInfo';
import { hasAllStandardSecurityPermissions } from '../../../packages/engine/services/src/shared/securityPermissions';

import {
  ADVISORY_CASES,
  ALL_ALLOWED_CASES,
  BUILD_CASES,
  OPENED_AS_CODE_CASES,
  PROBED_AT,
  STATE_CASES,
} from './security-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'security');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/** The three shapes `buildSecurityInfo` does not construct. */
function unbuiltProbe(name: string): DocumentSecurityProbeInfo {
  const base = {
    securityHandlerRevision: null,
    pdfPermissionsBits: null,
    pdfPermissionsAllAllowed: null,
    pdfOpenedAs: null,
    securityProbedAt: PROBED_AT,
  } as const;
  switch (name) {
    case 'password-blocked':
      return { ...base, encryptionState: 'encrypted', encryptionRequiresPassword: true };
    case 'unsupported':
      return { ...base, encryptionState: 'unsupported', encryptionRequiresPassword: null };
    case 'unknown':
      return { ...base, encryptionState: 'unknown', encryptionRequiresPassword: null };
    default:
      throw new Error(`unknown unbuilt probe ${name}`);
  }
}

const BUILT = new Map(
  BUILD_CASES.map((c) => [
    c.name,
    buildSecurityInfo({
      openedAs: c.openedAs,
      permissionsBits: c.permissionsBits,
      securityHandlerRevision: c.securityHandlerRevision,
      probedAt: PROBED_AT,
    }),
  ]),
);

function probeByName(name: string): DocumentSecurityProbeInfo {
  return BUILT.get(name) ?? unbuiltProbe(name);
}

function serialize(payload: unknown): string {
  return `${JSON.stringify(payload, null, 2)}\n`;
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
    process.stdout.write(`  ok      security/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS security/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written security/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const payload = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'security.state',
    probedAt: PROBED_AT,
    allStandardSecurityPermissions: 0xfffffffc,

    // `buildSecurityInfo`, plus the all-allowed predicate on the same word so
    // a Rust failure names which of the two disagreed.
    build: BUILD_CASES.map((c) => ({
      ...c,
      allAllowed: hasAllStandardSecurityPermissions(c.permissionsBits),
      info: BUILT.get(c.name),
    })),

    // `hasAllStandardSecurityPermissions`, over words in both signs.
    allAllowed: ALL_ALLOWED_CASES.map((c) => ({
      ...c,
      normalized: c.raw >>> 0,
      allAllowed: hasAllStandardSecurityPermissions(c.raw),
    })),

    // `openedAsFromCode`.
    openedAsFromCode: OPENED_AS_CODE_CASES.map((c) => ({ ...c, openedAs: openedAsFromCode(c.code) })),

    // `advisoryFromPdfBits`, with the decoded bits alongside so a divergence
    // separates a decoder disagreement from an advisory one.
    advisory: ADVISORY_CASES.map((c) => {
      const bits = decodePdfBits(c.raw);
      return { ...c, bits, advisory: advisoryFromPdfBits(bits) };
    }),

    // `securityStateFromProbe`.
    state: STATE_CASES.map((c) => ({
      ...c,
      probeInfo: probeByName(c.probe),
      state: securityStateFromProbe(probeByName(c.probe), {
        ...(c.accessEndpoint !== undefined && { accessEndpoint: c.accessEndpoint }),
        ...(c.cdnRequired !== undefined && { cdnRequired: c.cdnRequired }),
      }),
    })),
  };

  const differed = await emit('state.json', serialize(payload), check);
  if (check && differed) {
    process.stderr.write(
      '\nSecurity vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run security\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-security-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
