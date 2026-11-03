import fs from 'node:fs';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
const hash = (p) => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const out = 'tooling/dotnet-parity/fixtures/crop-rotations.pdf';
const objects = ['<< /Type /Catalog /Pages 2 0 R >>', ''];
const kids = [];
for (const rotation of [0, 90, 180, 270]) {
  const pageId = objects.length + 1,
    streamId = pageId + 1;
  kids.push(pageId + ' 0 R');
  objects.push(
    `<< /Type /Page /Parent 2 0 R /MediaBox [10 20 310 220] /CropBox [30 40 360 260] /Rotate ${rotation} /UserUnit 1.25 /Resources << /Font << /F1 11 0 R >> >> /Contents ${streamId} 0 R >>`,
  );
  const content =
    'BT /F1 12 Tf 1 0 0 1 40 190 Tm ("quote" "quote" A  B) Tj ET\nBT /F1 12 Tf 0 1 -1 0 80 70 Tm (vertical) Tj ET\nBT /F1 12 Tf 1 0 0.25 1 100 100 Tm (sheared) Tj ET\nq 1 0 0 rg 30 40 20 20 re f Q\n';
  objects.push(`<< /Length ${Buffer.byteLength(content)} >>\nstream\n${content}endstream`);
}
objects[1] = `<< /Type /Pages /Kids [${kids.join(' ')}] /Count 4 >>`;
objects.push('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
let pdf = '%PDF-1.7\n',
  offsets = [0];
objects.forEach((s, i) => {
  offsets.push(Buffer.byteLength(pdf));
  pdf += `${i + 1} 0 obj\n${s}\nendobj\n`;
});
const start = Buffer.byteLength(pdf);
pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
pdf += offsets
  .slice(1)
  .map((n) => String(n).padStart(10, '0') + ' 00000 n \n')
  .join('');
pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${start}\n%%EOF\n`;
fs.writeFileSync(out, pdf);
const probes = [{ id: 'pages', operation: 'pages' }];
for (let page = 0; page < 4; page++) {
  for (const operation of ['text', 'geometry'])
    probes.push({ id: `p${page}-${operation}`, operation, page });
  for (const [start, count] of [
    [0, 0],
    [0, 3],
    [2, 5],
    [0, 12],
  ])
    probes.push({
      id: `p${page}-segments-${start}-${count}`,
      operation: 'segments',
      page,
      start,
      count,
    });
  for (const rotation of [0, 90, 180, 270]) {
    probes.push({
      id: `p${page}-render-r${rotation}`,
      operation: 'render',
      page,
      rotation,
      scale: 0.5,
      region: { x: 0.00005, y: 0, width: 50, height: 40 },
    });
    probes.push({
      id: `p${page}-transform-r${rotation}`,
      operation: 'transform',
      page,
      input: {
        pageSize: { width: 280, height: 180 },
        rotation,
        scale: 1.333333333333,
        dpr: 1.25,
        baseScale: 1.666666666666,
      },
      pagePoints: [
        { x: 0, y: 0 },
        { x: 0.00005, y: -0.00005 },
        { x: 280, y: 180 },
      ],
      pageRects: [{ x: -0.00005, y: 0.00005, width: 280, height: 180 }],
      viewPoints: [
        { x: 0, y: 0 },
        { x: 373, y: 240 },
      ],
      viewRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    });
  }
}
const charmap = JSON.parse(fs.readFileSync('tooling/engine-conformance/vectors/text/charmap.json'));
const helperProbes = charmap.cases.map((c) => ({
  id: 'charmap-' + c.name,
  operation: 'helperText',
  text: {
    text: c.text,
    charCount: c.charCount,
    ...(c.charMap ? { charMap: c.charMap.map(([char, text]) => ({ char, text })) } : {}),
  },
  ranges: c.slices
    .filter((r) => r.start >= 0 && r.end >= r.start && r.end <= c.charCount)
    .map((r) => ({ start: r.start, count: r.end - r.start })),
}));
const simple = [
  { id: 'pages', operation: 'pages' },
  { id: 'text', operation: 'text', page: 0 },
  { id: 'geometry', operation: 'geometry', page: 0 },
  { id: 'empty-segments', operation: 'segments', start: 0, count: 0 },
  { id: 'first-segment', operation: 'segments', start: 0, count: 1 },
  { id: 'transparent-render', operation: 'render', rotation: 0, scale: 0.5 },
];
const cases = [
  { id: 'crop-rotations-transparent-repeated-quotes-empty-cells', pdf: out, probes },
  ...['embedded_orientations', 'non_identity_text', 'reverse_and_gapped_text'].map((name) => ({
    id: name,
    pdf: `tooling/engine-conformance/fixtures/${name}.pdf`,
    probes: simple,
  })),
  {
    id: 'surrogate-dropped-combining-rtl-charmaps',
    pdf: out,
    probes: [...simple, ...helperProbes],
  },
];
execFileSync(process.execPath, ['tooling/dotnet-parity/fixtures/write-abi-input.mjs'], {
  stdio: 'inherit',
});
const manifest = {
  schemaVersion: 1,
  sourceCommit: '91fcaccc53f6a38420123fb6a9ede4be44aab8d6',
  imageId: 'sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703',
  environment: {
    os: process.platform,
    arch: process.arch,
    node: process.version,
    dotnet: execFileSync('/opt/dotnet/dotnet', ['--version'], { encoding: 'utf8' }).trim(),
    libc: execFileSync('getconf', ['GNU_LIBC_VERSION'], { encoding: 'utf8' }).trim(),
    playwright: JSON.parse(
      fs.readFileSync('tooling/viewer-e2e/node_modules/@playwright/test/package.json'),
    ).version,
  },
  runtimeFork: '1461b0c5105137954b77dea0a6e2c15668518651',
  runtimeBuildFlags: { EMBEDPDF_RUST_CORE: '0' },
  runtime: {
    ...(fs.existsSync('packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm')
      ? {
          wasm: {
            path: 'packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm',
            sha256: hash('packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm'),
          },
          wasmBindings: {
            path: 'tooling/dotnet-parity/fixtures/.local/functions.json',
            sha256: hash('tooling/dotnet-parity/fixtures/.local/functions.json'),
          },
        }
      : {}),
    bindingSource: {
      path: 'packages/engine/runtime/src/core/pdf-functions.generated.ts',
      sha256: hash('packages/engine/runtime/src/core/pdf-functions.generated.ts'),
    },
    nativeMetadata: {
      path: '/opt/pdfium/linux-arm64/BUILD-METADATA.json',
      sha256: hash('/opt/pdfium/linux-arm64/BUILD-METADATA.json'),
    },
    native: {
      path: '/opt/pdfium/linux-arm64/lib/libembedpdf.so',
      sha256: hash('/opt/pdfium/linux-arm64/lib/libembedpdf.so'),
    },
  },
  viewer: {
    package: '@embedpdf/viewer-react',
    backend: 'typescript',
    flags: { rustReads: false, rustSession: false },
    packageArtifacts: [],
    packedFiles: [],
    packedDirectory: null,
  },
  coordinateContract: {
    pageIndexBase: 0,
    unit: 'PDF points',
    frame: 'top-left of unrotated visible CropBox',
    yDirection: 'down',
    rounding: { places: 4, mode: 'AwayFromZero', source: 'serialized decimal number tokens' },
  },
  raster: { comparison: 'exact', tolerance: null, approval: null },
  fonts: [
    {
      path: '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
      sha256: hash('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'),
      key: 'parity-dejavu',
      license: {
        path: '/usr/share/doc/fonts-dejavu-core/copyright',
        sha256: hash('/usr/share/doc/fonts-dejavu-core/copyright'),
      },
      familyName: 'DejaVu Sans',
      weight: 400,
      italic: false,
      fallback: true,
    },
  ],
  deploymentFontBundle: { status: 'not-supplied', compatibility: 'unvalidated' },
  corpusInputs: [
    'tooling/engine-conformance/vectors/text/charmap.json',
    'tooling/engine-conformance/vectors/text/layout.json',
  ].map((path) => ({ path, sha256: hash(path) })),
  cases: cases.map((c) => ({ ...c, sha256: hash(c.pdf) })),
};
fs.writeFileSync('tooling/dotnet-parity/manifest.json', JSON.stringify(manifest, null, 2) + '\n');
