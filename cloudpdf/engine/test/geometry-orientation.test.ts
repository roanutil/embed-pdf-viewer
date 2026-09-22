import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import { dirname, resolve } from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import {
  runPageGeometryOrientationConformance,
  type ConformanceTestRunner,
  type PageGeometryOrientationFixture,
} from '@embedpdf/engine-core/conformance';
import { createCloudEngine } from '../src/index';
import {
  buildDbSeededFixture,
  docScopedToken,
  seedDocumentFromBytes,
  teardownDbSeededFixture,
  type DbSeededFixture,
} from './_helpers/db-seeded-app';

// The cloud half of packages/engine/main/test/geometry-orientation.test.ts.
// Same five fixtures, same expectations, against the native PDFium worker pool
// over HTTP instead of a WASM runtime in this process.
//
// multiplatform-plan.md Section 9 names this suite as one of three that the
// engine-local runner has and the cloud runner does not, and G2's gate depends
// on it: "run against both backends" was not yet true for it. It is now.

const here = dirname(fileURLToPath(import.meta.url));
const fixturesRoot = resolve(
  here,
  '..',
  '..',
  '..',
  'packages',
  'engine',
  'main',
  'test',
  'fixtures',
);

const runner: ConformanceTestRunner = {
  describe,
  test,
  beforeAll,
  afterAll,
  expect: expect as unknown as ConformanceTestRunner['expect'],
};

const TENANT_ID = 'cloud-geometry-orientation-tenant';

// Every one of the five is a single-page document whose page is indirect
// object 3, verified against the WASM runtime. They come from the PDFium
// fork's test corpus (runtime-src/testing/resources), which is also where the
// native embeddertests pin them.
const PAGE_OBJECT_NUMBER = 3;
const PAGE_COUNT = 1;

interface Case {
  readonly docId: string;
  readonly file: string;
  readonly expectation: PageGeometryOrientationFixture['expectation'];
}

const cases: Case[] = [
  {
    // 45-degree-family rotated text in all four quadrants (0.7071 matrices).
    docId: 'georot001',
    file: 'rotated_text.pdf',
    expectation: {
      kind: 'rotated',
      rotations: [Math.PI / 4, -Math.PI / 4, (3 * Math.PI) / 4, (-3 * Math.PI) / 4],
      ascentFlip: false,
      minRotatedRuns: 4,
    },
  },
  {
    // Fake italic: shear-only matrix [1 0 0.5 1]. Rotation 0, parallelogram
    // cells, the case an AABB cannot express.
    docId: 'geoshear01',
    file: 'sheared_text.pdf',
    expectation: { kind: 'rotated', rotations: [0], ascentFlip: false, sheared: true },
  },
  {
    // Horizontal mirror [-1 0 0 1]: baseline angle pi with a flipped ascent.
    docId: 'geomirror1',
    file: 'mirrored_text.pdf',
    expectation: { kind: 'rotated', rotations: [Math.PI], ascentFlip: true },
  },
  {
    // Vertical CJK writing uses an upright matrix. The guard that vertical
    // text stays on the unchanged upright path.
    docId: 'geovert001',
    file: 'vertical_text.pdf',
    expectation: { kind: 'upright' },
  },
  {
    // `/F1 0 Tf`: degenerate glyphs only, the fork's crash regression.
    docId: 'geozerofs1',
    file: 'font_size_zero.pdf',
    expectation: { kind: 'empty-only' },
  },
];

let fx: DbSeededFixture | undefined;

beforeAll(async () => {
  fx = await buildDbSeededFixture({ secret: 'cloud-geometry-orientation-secret' });
  for (const entry of cases) {
    await seedDocumentFromBytes(
      fx,
      TENANT_ID,
      entry.docId,
      resolve(fixturesRoot, entry.file),
      PAGE_COUNT,
    );
  }
});

afterAll(async () => {
  await teardownDbSeededFixture(fx);
});

for (const entry of cases) {
  runPageGeometryOrientationConformance(runner, {
    label: 'cloud engine (HTTP -> @cloudpdf/server, native runtime, versioned URLs)',
    openKind: 'id',
    fixture: {
      id: entry.docId,
      // The cloud engine opens by id; the bytes are already in the object
      // store. The loader stays because the fixture type requires one, and
      // reading the real file here would be a second copy of the seed.
      bytes: async () => new Uint8Array(await readFile(resolve(fixturesRoot, entry.file))),
      expected: {},
      pageObjectNumber: PAGE_OBJECT_NUMBER,
      expectation: entry.expectation,
    },
    makeEngine: () => {
      if (!fx) throw new Error('fixture not initialised');
      return createCloudEngine({
        baseUrl: fx.baseUrl,
        token: docScopedToken(fx, TENANT_ID, entry.docId),
      });
    },
  });
}
