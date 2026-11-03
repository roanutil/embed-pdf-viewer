import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const directory = path.resolve(process.argv[2]);
const relative = (p) => path.relative(process.cwd(), p);
const file = (p) => {
  const bytes = fs.readFileSync(p);
  return {
    path: relative(p),
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    bytes: bytes.length,
  };
};
const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'manifest.input.json')));
let renders = 0,
  totalBytes = 0,
  differingChannels = 0;
for (const item of manifest.cases) {
  const viewerPath = path.join(directory, item.id + '.viewer.json');
  const nativePath = path.join(directory, item.id + '.native.json');
  if (!fs.existsSync(viewerPath) || !fs.existsSync(nativePath)) continue;
  const viewer = JSON.parse(fs.readFileSync(viewerPath)),
    native = JSON.parse(fs.readFileSync(nativePath));
  const probes = [];
  for (const probe of viewer.filter((p) => p.operation === 'render')) {
    const other = native.find((p) => p.id === probe.id && p.operation === probe.operation);
    if (!other) throw new Error('Native render missing: ' + probe.id);
    const a = Buffer.from(probe.value.pixels, 'base64'),
      b = Buffer.from(other.value.pixels, 'base64');
    let channels = 0,
      pixels = 0,
      maximumChannelDelta = 0;
    for (let index = 0; index < Math.max(a.length, b.length); index += 4) {
      let changed = false;
      for (let channel = 0; channel < 4; channel++) {
        const offset = index + channel;
        if (a[offset] !== b[offset]) {
          channels++;
          changed = true;
          maximumChannelDelta = Math.max(
            maximumChannelDelta,
            Math.abs((a[offset] ?? 0) - (b[offset] ?? 0)),
          );
        }
      }
      if (changed) pixels++;
    }
    probes.push({
      id: probe.id,
      bytes: a.length,
      nativeBytes: b.length,
      differingChannels: channels,
      differingPixels: pixels,
      maximumChannelDelta,
    });
    renders++;
    totalBytes += a.length;
    differingChannels += channels;
  }
  const recordPath = path.join(directory, item.id + '.record.json');
  const record = JSON.parse(fs.readFileSync(recordPath));
  const summaryPath = path.join(directory, item.id + '.raster-summary.json');
  const summary = {
    diagnosticOnly: record.limitations.some((p) => p.startsWith('DIAGNOSTIC ONLY')),
    comparison: 'exact',
    tolerance: null,
    viewer: file(viewerPath),
    native: file(nativePath),
    probes,
  };
  fs.writeFileSync(summaryPath, JSON.stringify(summary, null, 2) + '\n');
  record.artifacts = record.artifacts
    .filter((p) => p.path !== relative(summaryPath))
    .concat(file(summaryPath));
  record.commands = record.commands.filter(
    (p) => !p.command.startsWith('node tooling/dotnet-parity/fixtures/summarize-rasters.mjs'),
  );
  record.commands.push({
    command: 'node tooling/dotnet-parity/fixtures/summarize-rasters.mjs ' + relative(directory),
    cwd: process.cwd(),
    exitCode: 0,
    results: JSON.stringify({
      diagnosticOnly: summary.diagnosticOnly,
      renders: probes.length,
      differingChannels: probes.reduce((n, p) => n + p.differingChannels, 0),
    }),
  });
  fs.writeFileSync(recordPath, JSON.stringify(record, null, 2) + '\n');
}
console.log(JSON.stringify({ renders, totalBytes, differingChannels, tolerance: null }));
