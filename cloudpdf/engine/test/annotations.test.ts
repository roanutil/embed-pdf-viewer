import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import { dirname, resolve } from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import {
  runAnnotationReadConformance,
  type ConformanceTestRunner,
} from '@embedpdf/engine-core/conformance';
import { createCloudEngine } from '../src/index';
import {
  buildDbSeededFixture,
  docScopedToken,
  seedDocumentFromBytes,
  teardownDbSeededFixture,
  type DbSeededFixture,
} from './_helpers/db-seeded-app';

// The cloud half of packages/engine/main/test/annotations.test.ts: the same
// annotation read conformance, against the native PDFium worker pool over
// HTTP. multiplatform-plan.md Section 9 names this as one of three suites the
// cloud runner was missing, and it is the gate G7 depends on.

const here = dirname(fileURLToPath(import.meta.url));
const fixturePath = resolve(
  here,
  '..',
  '..',
  '..',
  'examples',
  'engine-runtime-demo',
  'public',
  'annotations.pdf',
);

const runner: ConformanceTestRunner = {
  describe,
  test,
  beforeAll,
  afterAll,
  expect: expect as unknown as ConformanceTestRunner['expect'],
};

// One page, indirect object 3, 14 annotations; the same counts the
// engine-local suite asserts against a WASM PDFium.
const TENANT_ID = 'cloud-annotation-read-tenant';
const DOC_ID = 'annotations-pdf-reads-cloud';
const PAGE_OBJECT_NUMBER = 3;
const PAGE_COUNT = 1;

let fx: DbSeededFixture | undefined;

beforeAll(async () => {
  fx = await buildDbSeededFixture({ secret: 'cloud-annotation-read-secret' });
  await seedDocumentFromBytes(fx, TENANT_ID, DOC_ID, fixturePath, PAGE_COUNT);
});

afterAll(async () => {
  await teardownDbSeededFixture(fx);
});

runAnnotationReadConformance(runner, {
  label: 'cloud engine (HTTP -> @cloudpdf/server, native runtime, versioned URLs)',
  openKind: 'id',
  fixture: {
    id: DOC_ID,
    bytes: async () => new Uint8Array(await readFile(fixturePath)),
    expected: { trapped: 'unknown' },
    pageObjectNumber: PAGE_OBJECT_NUMBER,
    expectedAnnotationCount: 14,
    minHighlightCount: 4,
    minUnsupportedCount: 1,
    minCircleCount: 1,
    minSquareCount: 1,
    minPolygonCount: 1,
    minPolylineCount: 1,
    minLineCount: 1,
    minInkCount: 1,
    minFreeTextCount: 2,
    minCaretCount: 1,
    expectsWeakAnnotation: true,
  },
  makeEngine: () => {
    if (!fx) throw new Error('fixture not initialised');
    return createCloudEngine({
      baseUrl: fx.baseUrl,
      token: docScopedToken(fx, TENANT_ID, DOC_ID),
    });
  },
});
