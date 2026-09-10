/**
 * Freezes the TypeScript reference behavior into ../vectors/anchor-vectors.json.
 * Run deliberately; never as part of a test.
 */
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
const here = dirname(fileURLToPath(import.meta.url));
// Node 24 strips types natively, and the workspace symlinks resolve the bare
// specifiers to packages whose `exports` point straight at ./src/index.ts.
const { projectQuad, anchorFactors } = await import('@poc/shapes');
const { rectQuad, pointInQuad } = await import('@poc/geometry');

const round = (n) => Math.round(n * 1e6) / 1e6;
const q6 = (q) => q.map((p) => [round(p.x), round(p.y)]);

const shapes = [
  { rect: { x: 100, y: 60, width: 80, height: 40 }, rot: 0 },
  { rect: { x: 100, y: 60, width: 80, height: 40 }, rot: 30 },
  { rect: { x: -20, y: 12.5, width: 33, height: 77 }, rot: 90 },
];
const views = [
  { zoom: 1, rotation: 0 },
  { zoom: 0.25, rotation: 0 },
  { zoom: 2, rotation: 0 },
  { zoom: 2, rotation: 90 },
  { zoom: 4, rotation: 270 },
  { zoom: 1, rotation: 180 },
];

const projection = [];
for (const [si, s] of shapes.entries()) {
  for (const [vi, view] of views.entries()) {
    for (const anchored of [false, true]) {
      const base = rectQuad(s.rect, s.rot);
      projection.push({
        name: `shape${si}/view${vi}/${anchored ? 'anchored' : 'plain'}`,
        rect: s.rect,
        rot: s.rot,
        view,
        anchored,
        factors: anchorFactors(anchored, view),
        quad: q6(projectQuad(base, view, anchored)),
      });
    }
  }
}

const hit = [];
for (const [si, s] of shapes.entries()) {
  const q = rectQuad(s.rect, s.rot);
  const b = q.reduce(
    (acc, p) => ({
      minX: Math.min(acc.minX, p.x),
      minY: Math.min(acc.minY, p.y),
      maxX: Math.max(acc.maxX, p.x),
      maxY: Math.max(acc.maxY, p.y),
    }),
    { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity },
  );
  const cx = (b.minX + b.maxX) / 2;
  const cy = (b.minY + b.maxY) / 2;
  const probes = [
    { x: cx, y: cy },
    { x: b.minX - 1, y: cy },
    { x: b.maxX + 1, y: cy },
    { x: cx, y: b.minY - 1 },
    { x: b.minX + 0.5, y: b.minY + 0.5 },
  ];
  for (const [pi, p] of probes.entries()) {
    hit.push({
      name: `shape${si}/probe${pi}`,
      rect: s.rect,
      rot: s.rot,
      point: { x: round(p.x), y: round(p.y) },
      inside: pointInQuad(p, q),
    });
  }
}

const out = resolve(here, '../../vectors/anchor-vectors.json');
writeFileSync(
  out,
  JSON.stringify({ generatedFrom: 'typescript-baseline TypeScript reference', projection, hit }, null, 2) + '\n',
);
console.log(`wrote ${projection.length} projection + ${hit.length} hit vectors to ${out}`);
